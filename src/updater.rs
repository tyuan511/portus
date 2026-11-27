use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering as AtomicOrdering},
};
use std::time::Duration;

use serde::Deserialize;

pub const DEFAULT_GITHUB_REPO: &str = "tyuan511/portus";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let trimmed = trimmed
            .strip_prefix('v')
            .or_else(|| trimmed.strip_prefix('V'))
            .unwrap_or(trimmed);
        let without_build = trimmed.split('+').next().unwrap_or(trimmed);
        let (core, pre) = match without_build.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_owned())),
            Some((c, _)) => (c, None),
            None => (without_build, None),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub fn current() -> Self {
        Self::parse(CURRENT_VERSION).unwrap_or(Self {
            major: 0,
            minor: 0,
            patch: 0,
            pre: None,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => compare_prerelease(a, b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prerelease(a: &str, b: &str) -> Ordering {
    let mut a_parts = a.split('.');
    let mut b_parts = b.split('.');
    loop {
        match (a_parts.next(), b_parts.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ap), Some(bp)) => {
                let ord = match (ap.parse::<u64>(), bp.parse::<u64>()) {
                    (Ok(an), Ok(bn)) => an.cmp(&bn),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => ap.cmp(bp),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub name: String,
    pub notes: String,
    pub html_url: String,
    pub published_at: Option<String>,
    pub asset: Option<ReleaseAsset>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Downloading {
        release: ReleaseInfo,
        downloaded: u64,
        total: u64,
    },
    Preparing {
        release: ReleaseInfo,
    },
    ReadyToRestart {
        release: ReleaseInfo,
        staged_app: PathBuf,
        target_app: PathBuf,
        staging_root: PathBuf,
    },
    DownloadedInstaller {
        release: ReleaseInfo,
        installer_path: PathBuf,
    },
    Error(String),
}

impl UpdateStatus {
    pub fn has_pending_update(&self) -> bool {
        matches!(
            self,
            Self::Available(_)
                | Self::Downloading { .. }
                | Self::Preparing { .. }
                | Self::ReadyToRestart { .. }
                | Self::DownloadedInstaller { .. }
        )
    }

    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Preparing { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub enum UpdateUiEvent {
    Changed,
    UpdateFound {
        release: ReleaseInfo,
        silent: bool,
    },
    UpToDate {
        silent: bool,
    },
    ReadyToRestart {
        release: ReleaseInfo,
    },
    InstallerDownloaded {
        release: ReleaseInfo,
    },
    CheckFailed {
        message: String,
        silent: bool,
    },
    InstallFailed {
        message: String,
    },
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

pub struct UpdateManager {
    status: Mutex<UpdateStatus>,
    ui_tx: Option<async_channel::Sender<UpdateUiEvent>>,
    cancel_download: Arc<AtomicBool>,
}

impl UpdateManager {
    pub fn new(ui_tx: Option<async_channel::Sender<UpdateUiEvent>>) -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(UpdateStatus::Idle),
            ui_tx,
            cancel_download: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, next: UpdateStatus) {
        *self.status.lock().unwrap() = next;
        self.emit(UpdateUiEvent::Changed);
    }

    fn emit(&self, event: UpdateUiEvent) {
        if let Some(tx) = &self.ui_tx {
            let _ = tx.try_send(event);
        }
    }

    pub fn check_for_updates(self: &Arc<Self>, silent: bool) {
        {
            let mut guard = self.status.lock().unwrap();
            if guard.is_busy() {
                return;
            }
            if !silent || matches!(*guard, UpdateStatus::Idle | UpdateStatus::Error(_)) {
                *guard = UpdateStatus::Checking;
            } else {
                // Keep existing state if silent check is triggered while already UpToDate/Available.
                if matches!(*guard, UpdateStatus::Available(_) | UpdateStatus::ReadyToRestart { .. }) {
                    return;
                }
                *guard = UpdateStatus::Checking;
            }
        }
        self.emit(UpdateUiEvent::Changed);

        let manager = Arc::clone(self);
        std::thread::Builder::new()
            .name("portus-update-check".into())
            .spawn(move || {
                match fetch_latest_release() {
                    Ok(Some(release)) => {
                        let current = Version::current();
                        let latest = Version::parse(&release.tag_name);
                        if latest.is_some_and(|v| v > current) {
                            manager.set_status(UpdateStatus::Available(release.clone()));
                            manager.emit(UpdateUiEvent::UpdateFound { release, silent });
                        } else {
                            manager.set_status(UpdateStatus::UpToDate);
                            manager.emit(UpdateUiEvent::UpToDate { silent });
                        }
                    }
                    Ok(None) => {
                        manager.set_status(UpdateStatus::UpToDate);
                        manager.emit(UpdateUiEvent::UpToDate { silent });
                    }
                    Err(err) => {
                        if silent {
                            manager.set_status(UpdateStatus::Idle);
                        } else {
                            manager.set_status(UpdateStatus::Error(err.clone()));
                        }
                        manager.emit(UpdateUiEvent::CheckFailed {
                            message: err,
                            silent,
                        });
                    }
                }
            })
            .ok();
    }

    pub fn download_and_prepare(self: &Arc<Self>, release: ReleaseInfo) {
        let Some(asset) = release.asset.clone() else {
            let _ = open_external_url(&release.html_url);
            return;
        };

        {
            let mut guard = self.status.lock().unwrap();
            if guard.is_busy() {
                return;
            }
            *guard = UpdateStatus::Downloading {
                release: release.clone(),
                downloaded: 0,
                total: asset.size,
            };
        }
        self.cancel_download.store(false, AtomicOrdering::SeqCst);
        self.emit(UpdateUiEvent::Changed);

        let manager = Arc::clone(self);
        let cancel_flag = Arc::clone(&self.cancel_download);

        std::thread::Builder::new()
            .name("portus-update-download".into())
            .spawn(move || {
                let result = download_and_stage_asset(&manager, &release, &asset, &cancel_flag);
                match result {
                    Ok(StageOutcome::Cancelled) => {
                        manager.set_status(UpdateStatus::Available(release));
                    }
                    Ok(StageOutcome::ReadyToRestart {
                        staged_app,
                        target_app,
                        staging_root,
                    }) => {
                        manager.set_status(UpdateStatus::ReadyToRestart {
                            release: release.clone(),
                            staged_app,
                            target_app,
                            staging_root,
                        });
                        manager.emit(UpdateUiEvent::ReadyToRestart { release });
                    }
                    Ok(StageOutcome::DownloadedInstaller { installer_path }) => {
                        let _ = open_external_path(&installer_path);
                        manager.set_status(UpdateStatus::DownloadedInstaller {
                            release: release.clone(),
                            installer_path,
                        });
                        manager.emit(UpdateUiEvent::InstallerDownloaded { release });
                    }
                    Err(err) => {
                        manager.set_status(UpdateStatus::Error(err.clone()));
                        manager.emit(UpdateUiEvent::InstallFailed { message: err });
                    }
                }
            })
            .ok();
    }

    pub fn cancel_download(&self) {
        self.cancel_download.store(true, AtomicOrdering::SeqCst);
    }

    pub fn launch_installer_and_prepare_quit(&self) -> Result<(), String> {
        let status = self.status();
        match status {
            UpdateStatus::ReadyToRestart {
                staged_app,
                target_app,
                staging_root,
                ..
            } => spawn_macos_relaunch_helper(&staged_app, &target_app, &staging_root),
            UpdateStatus::DownloadedInstaller { installer_path, .. } => {
                open_external_path(&installer_path)
            }
            _ => Err("No staged update ready to install.".to_owned()),
        }
    }
}

enum StageOutcome {
    Cancelled,
    ReadyToRestart {
        staged_app: PathBuf,
        target_app: PathBuf,
        staging_root: PathBuf,
    },
    DownloadedInstaller {
        installer_path: PathBuf,
    },
}

fn github_latest_release_url() -> String {
    if let Ok(url) = std::env::var("PORTUS_UPDATE_API_URL")
        && !url.trim().is_empty()
    {
        return url;
    }
    let repo = std::env::var("PORTUS_UPDATE_REPO")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_GITHUB_REPO.to_owned());
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

fn fetch_latest_release() -> Result<Option<ReleaseInfo>, String> {
    let url = github_latest_release_url();
    let user_agent = format!("Portus/{CURRENT_VERSION}");
    let output = Command::new("curl")
        .args([
            "-sS",
            "-L",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "X-GitHub-Api-Version: 2022-11-28",
            "-H",
            &format!("User-Agent: {user_agent}"),
            "-w",
            "\n%{http_code}",
            &url,
        ])
        .output()
        .map_err(|err| format!("Failed to invoke curl: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "Network request failed while checking for updates.".to_owned()
        } else {
            stderr
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let (body, code_str) = raw
        .rsplit_once('\n')
        .ok_or_else(|| "Unexpected response from GitHub API.".to_owned())?;
    let status_code: u16 = code_str
        .trim()
        .parse()
        .map_err(|_| "Invalid HTTP status code from GitHub API.".to_owned())?;

    if status_code == 404 {
        // Repository has no published releases yet.
        return Ok(None);
    }
    if status_code == 403 {
        return Err("GitHub API rate limit reached. Please try again later.".to_owned());
    }
    if !(200..300).contains(&status_code) {
        return Err(format!("GitHub API returned HTTP {status_code}."));
    }

    parse_github_release_json(body)
}

fn parse_github_release_json(json_str: &str) -> Result<Option<ReleaseInfo>, String> {
    let release: GithubRelease =
        serde_json::from_str(json_str).map_err(|err| format!("Failed to parse release JSON: {err}"))?;
    if release.draft {
        return Ok(None);
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .or_else(|| release.tag_name.strip_prefix('V'))
        .unwrap_or(&release.tag_name)
        .to_owned();
    let asset = select_platform_asset(&release.assets, std::env::consts::OS, std::env::consts::ARCH);
    let name = release
        .name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("Portus {}", release.tag_name));
    Ok(Some(ReleaseInfo {
        tag_name: release.tag_name,
        version,
        name,
        notes: release.body.unwrap_or_default().trim().to_owned(),
        html_url: release.html_url,
        published_at: release.published_at,
        asset,
    }))
}

fn select_platform_asset(assets: &[GithubAsset], os: &str, arch: &str) -> Option<ReleaseAsset> {
    let arch_tokens: &[&str] = match arch {
        "aarch64" => &["arm64", "aarch64"],
        "x86_64" => &["x86_64", "x64", "amd64"],
        other => &[other],
    };

    if os == "macos" {
        let is_dmg = |a: &&GithubAsset| a.name.to_ascii_lowercase().ends_with(".dmg");
        let matches_arch = |a: &&GithubAsset| {
            let lower = a.name.to_ascii_lowercase();
            arch_tokens.iter().any(|tok| lower.contains(tok))
        };
        let picked = assets
            .iter()
            .find(|a| is_dmg(a) && matches_arch(a))
            .or_else(|| assets.iter().find(|a| is_dmg(a) && a.name.to_ascii_lowercase().contains("universal")))
            .or_else(|| {
                let dmgs: Vec<_> = assets.iter().filter(|a| is_dmg(a)).collect();
                (dmgs.len() == 1).then(|| dmgs[0])
            });
        return picked.map(|a| ReleaseAsset {
            name: a.name.clone(),
            download_url: a.browser_download_url.clone(),
            size: a.size,
        });
    }

    // Fallback for non-macOS: prefer an asset matching OS + arch.
    assets
        .iter()
        .find(|a| {
            let lower = a.name.to_ascii_lowercase();
            lower.contains(os) && arch_tokens.iter().any(|tok| lower.contains(tok))
        })
        .map(|a| ReleaseAsset {
            name: a.name.clone(),
            download_url: a.browser_download_url.clone(),
            size: a.size,
        })
}

fn download_and_stage_asset(
    manager: &UpdateManager,
    release: &ReleaseInfo,
    asset: &ReleaseAsset,
    cancel_flag: &AtomicBool,
) -> Result<StageOutcome, String> {
    let base_dir = std::env::temp_dir().join("portus-updates");
    fs::create_dir_all(&base_dir)
        .map_err(|err| format!("Could not create update temp directory: {err}"))?;
    let staging_dir = tempfile::Builder::new()
        .prefix("portus-update-")
        .tempdir_in(&base_dir)
        .map_err(|err| format!("Could not create staging directory: {err}"))?;
    let staging_root = staging_dir.keep();

    let safe_name = Path::new(&asset.name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Portus-update.dmg");
    let dmg_path = staging_root.join(safe_name);

    let user_agent = format!("Portus/{CURRENT_VERSION}");
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "-L",
            "--fail",
            "--connect-timeout",
            "15",
            "-H",
            &format!("User-Agent: {user_agent}"),
            "-o",
        ])
        .arg(&dmg_path)
        .arg(&asset.download_url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to start download: {err}"))?;

    let mut last_reported = 0u64;
    let exit_status = loop {
        if cancel_flag.load(AtomicOrdering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(&staging_root);
            return Ok(StageOutcome::Cancelled);
        }
        if let Ok(meta) = fs::metadata(&dmg_path) {
            let len = meta.len();
            if len != last_reported {
                last_reported = len;
                manager.set_status(UpdateStatus::Downloading {
                    release: release.clone(),
                    downloaded: len,
                    total: asset.size.max(len),
                });
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(120)),
            Err(err) => {
                let _ = fs::remove_dir_all(&staging_root);
                return Err(format!("Download interrupted: {err}"));
            }
        }
    };

    if !exit_status.success() {
        let mut err_buf = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut stderr, &mut err_buf);
        }
        let _ = fs::remove_dir_all(&staging_root);
        let msg = err_buf.trim();
        return Err(if msg.is_empty() {
            "Failed to download update package.".to_owned()
        } else {
            format!("Download failed: {msg}")
        });
    }

    if let Ok(meta) = fs::metadata(&dmg_path) {
        manager.set_status(UpdateStatus::Downloading {
            release: release.clone(),
            downloaded: meta.len(),
            total: asset.size.max(meta.len()),
        });
    }

    #[cfg(target_os = "macos")]
    {
        if dmg_path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("dmg"))
            && let Some(target_app) = current_app_bundle_path()
            && is_parent_writable(&target_app)
        {
            manager.set_status(UpdateStatus::Preparing {
                release: release.clone(),
            });
            match extract_app_from_dmg(&dmg_path, &staging_root) {
                Ok(staged_app) => {
                    let _ = fs::remove_file(&dmg_path);
                    return Ok(StageOutcome::ReadyToRestart {
                        staged_app,
                        target_app,
                        staging_root,
                    });
                }
                Err(err) => {
                    let _ = fs::remove_dir_all(&staging_root);
                    return Err(err);
                }
            }
        }
    }

    Ok(StageOutcome::DownloadedInstaller {
        installer_path: dmg_path,
    })
}

pub fn current_app_bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    app_bundle_from_exe(&exe)
}

fn app_bundle_from_exe(exe: &Path) -> Option<PathBuf> {
    // Expected layout: /Applications/Portus.app/Contents/MacOS/portus
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    if app_dir.extension()?.to_str()? != "app" {
        return None;
    }
    Some(app_dir.to_path_buf())
}

#[cfg(target_os = "macos")]
fn is_parent_writable(target_app: &Path) -> bool {
    let Some(parent) = target_app.parent() else {
        return false;
    };
    tempfile::NamedTempFile::new_in(parent).is_ok()
}

#[cfg(target_os = "macos")]
fn extract_app_from_dmg(dmg_path: &Path, staging_root: &Path) -> Result<PathBuf, String> {
    let mount_dir = staging_root.join("mnt");
    fs::create_dir_all(&mount_dir)
        .map_err(|err| format!("Failed to create mount point: {err}"))?;

    let attach = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-quiet", "-mountpoint"])
        .arg(&mount_dir)
        .arg(dmg_path)
        .output()
        .map_err(|err| format!("Failed to run hdiutil: {err}"))?;
    if !attach.status.success() {
        let stderr = String::from_utf8_lossy(&attach.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "Failed to mount downloaded DMG.".to_owned()
        } else {
            format!("Failed to mount DMG: {stderr}")
        });
    }

    let copy_result = (|| -> Result<PathBuf, String> {
        let source_app = find_app_in_dir(&mount_dir)
            .ok_or_else(|| "No .app bundle found inside downloaded DMG.".to_owned())?;
        let app_name = source_app
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Portus.app");
        let staged_app = staging_root.join(app_name);

        let ditto = Command::new("ditto")
            .arg(&source_app)
            .arg(&staged_app)
            .output()
            .map_err(|err| format!("Failed to run ditto: {err}"))?;
        if !ditto.status.success() {
            let stderr = String::from_utf8_lossy(&ditto.stderr).trim().to_owned();
            return Err(format!("Failed to copy app bundle: {stderr}"));
        }

        // Best-effort removal of quarantine attribute so relaunch is smooth.
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&staged_app)
            .status();

        Ok(staged_app)
    })();

    // Always detach the DMG before returning.
    let detach = Command::new("hdiutil")
        .args(["detach", "-quiet"])
        .arg(&mount_dir)
        .status();
    if !detach.is_ok_and(|s| s.success()) {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force", "-quiet"])
            .arg(&mount_dir)
            .status();
    }

    copy_result
}

#[cfg(target_os = "macos")]
fn find_app_in_dir(dir: &Path) -> Option<PathBuf> {
    let preferred = dir.join("Portus.app");
    if preferred.is_dir() {
        return Some(preferred);
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        {
            return Some(path);
        }
    }
    None
}

fn spawn_macos_relaunch_helper(
    staged_app: &Path,
    target_app: &Path,
    staging_root: &Path,
) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (staged_app, target_app, staging_root);
        Err("Automatic .app replacement is only supported on macOS.".to_owned())
    }
    #[cfg(target_os = "macos")]
    {
        let pid = std::process::id();
        let backup_app = staging_root.join("Portus.app.bak");
        let script = r#"
pid="$1"
staged="$2"
target="$3"
backup="$4"
staging_root="$5"

# Wait for the running app to exit (up to 20s)
i=0
while kill -0 "$pid" 2>/dev/null; do
  if [ "$i" -ge 100 ]; then
    break
  fi
  sleep 0.2
  i=$((i + 1))
done

rm -rf "$backup"
if [ -e "$target" ]; then
  mv "$target" "$backup" || exit 1
fi

if mv "$staged" "$target"; then
  rm -rf "$backup" "$staging_root"
  open -n "$target" >/dev/null 2>&1 || open "$target" >/dev/null 2>&1
else
  if [ -e "$backup" ] && [ ! -e "$target" ]; then
    mv "$backup" "$target"
  fi
  exit 1
fi
"#;
        Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg("portus-updater")
            .arg(pid.to_string())
            .arg(staged_app)
            .arg(target_app)
            .arg(&backup_app)
            .arg(staging_root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("Failed to launch update helper: {err}"))?;
        Ok(())
    }
}

pub fn open_external_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let res = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let res = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let res = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    res.map(|_| ()).map_err(|err| err.to_string())
}

pub fn open_external_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let res = Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let res = Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let res = Command::new("explorer").arg(path).spawn();
    res.map(|_| ()).map_err(|err| err.to_string())
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_compares_semver() {
        let v010 = Version::parse("v0.1.0").unwrap();
        let v011 = Version::parse("0.1.1").unwrap();
        let v020_alpha = Version::parse("v0.2.0-alpha.1").unwrap();
        let v020_beta = Version::parse("v0.2.0-beta.2").unwrap();
        let v020_beta10 = Version::parse("v0.2.0-beta.10").unwrap();
        let v020 = Version::parse("v0.2.0").unwrap();
        let v100 = Version::parse("V1.0.0+build.42").unwrap();

        assert!(v010 < v011);
        assert!(v011 < v020_alpha);
        assert!(v020_alpha < v020_beta);
        assert!(v020_beta < v020_beta10);
        assert!(v020_beta10 < v020);
        assert!(v020 < v100);
        assert_eq!(Version::parse("v0.1.0"), Version::parse("0.1.0"));
        assert!(Version::parse("invalid").is_none());
        assert!(Version::parse("1.2").is_none());
    }

    #[test]
    fn parses_github_release_and_picks_macos_arm64_dmg() {
        let json = r###"{
            "tag_name": "v0.2.0",
            "name": "Portus v0.2.0",
            "body": "## What's New\n- In-app updates",
            "html_url": "https://github.com/tyuan511/portus/releases/tag/v0.2.0",
            "published_at": "2026-11-27T00:00:00Z",
            "draft": false,
            "assets": [
                {
                    "name": "Portus-v0.2.0-macos-x86_64.dmg",
                    "browser_download_url": "https://example.com/x86_64.dmg",
                    "size": 1000
                },
                {
                    "name": "Portus-v0.2.0-macos-arm64.dmg",
                    "browser_download_url": "https://example.com/arm64.dmg",
                    "size": 2048
                }
            ]
        }"###;
        let parsed = parse_github_release_json(json).unwrap().unwrap();
        assert_eq!(parsed.tag_name, "v0.2.0");
        assert_eq!(parsed.version, "0.2.0");
        assert_eq!(parsed.name, "Portus v0.2.0");
        assert!(parsed.notes.contains("In-app updates"));

        let release: GithubRelease = serde_json::from_str(json).unwrap();
        let arm = select_platform_asset(&release.assets, "macos", "aarch64").unwrap();
        assert_eq!(arm.name, "Portus-v0.2.0-macos-arm64.dmg");
        assert_eq!(arm.size, 2048);

        let intel = select_platform_asset(&release.assets, "macos", "x86_64").unwrap();
        assert_eq!(intel.name, "Portus-v0.2.0-macos-x86_64.dmg");
    }

    #[test]
    fn detects_app_bundle_from_exe_path() {
        let bundled = Path::new("/Applications/Portus.app/Contents/MacOS/portus");
        assert_eq!(
            app_bundle_from_exe(bundled),
            Some(PathBuf::from("/Applications/Portus.app"))
        );
        let dev = Path::new("/Users/dev/portus/target/debug/Portus");
        assert_eq!(app_bundle_from_exe(dev), None);
    }

    #[test]
    fn formats_byte_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(15 * 1024 * 1024), "15.0 MB");
    }
}
