use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::theme::Theme;

pub const MIN_OPACITY: u8 = 50;
pub const MIN_FONT_SIZE: u8 = 10;
pub const MAX_FONT_SIZE: u8 = 22;
pub const DEFAULT_FONT_SIZE: u8 = 13;
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 210.0;
pub const MIN_SIDEBAR_WIDTH: f32 = 160.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 420.0;

pub fn clamp_sidebar_width(width: f32) -> f32 {
    if !width.is_finite() {
        return DEFAULT_SIDEBAR_WIDTH;
    }
    width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH).round()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    ChineseSimplified,
}

impl Language {
    pub fn text(self, english: &'static str, chinese: &'static str) -> &'static str {
        match self {
            Self::English => english,
            Self::ChineseSimplified => chinese,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShellSettings {
    pub executable: String,
    pub login: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            executable: String::new(),
            login: true,
        }
    }
}

impl ShellSettings {
    pub fn system_executable() -> OsString {
        if cfg!(windows) {
            std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into())
        } else {
            std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into())
        }
    }

    pub fn executable(&self) -> OsString {
        if self.executable.is_empty() {
            Self::system_executable()
        } else {
            self.custom_path().into_os_string()
        }
    }

    pub fn custom_path(&self) -> PathBuf {
        if let Some(relative) = self.executable.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(relative);
        }
        PathBuf::from(&self.executable)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelKind {
    Local,
    Remote,
    Dynamic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortForwardRule {
    pub id: u64,
    pub name: String,
    pub kind: TunnelKind,
    pub host_id: u64,
    pub bind_address: String,
    pub bind_port: u16,
    #[serde(default)]
    pub target_host: String,
    #[serde(default)]
    pub target_port: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostProtocol {
    #[default]
    Ssh,
    Telnet,
    Serial,
    Mosh,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostIcon {
    #[default]
    Server,
    Cloud,
    Database,
    Terminal,
    Cpu,
    HardDrive,
    Monitor,
    Globe,
    Shield,
    Network,
    Code,
    Lock,
}

impl HostIcon {
    pub const ALL: [Self; 12] = [
        Self::Server,
        Self::Cloud,
        Self::Database,
        Self::Terminal,
        Self::Cpu,
        Self::HardDrive,
        Self::Monitor,
        Self::Globe,
        Self::Shield,
        Self::Network,
        Self::Code,
        Self::Lock,
    ];

    pub const fn icon(self) -> crate::icons::Icon {
        match self {
            Self::Server => crate::icons::Icon::Server,
            Self::Cloud => crate::icons::Icon::Cloud,
            Self::Database => crate::icons::Icon::Database,
            Self::Terminal => crate::icons::Icon::Terminal,
            Self::Cpu => crate::icons::Icon::Cpu,
            Self::HardDrive => crate::icons::Icon::HardDrive,
            Self::Monitor => crate::icons::Icon::Monitor,
            Self::Globe => crate::icons::Icon::Globe,
            Self::Shield => crate::icons::Icon::Shield,
            Self::Network => crate::icons::Icon::Network,
            Self::Code => crate::icons::Icon::Code,
            Self::Lock => crate::icons::Icon::Lock,
        }
    }

    pub fn label(self, language: Language) -> &'static str {
        match self {
            Self::Server => language.text("Server", "服务器"),
            Self::Cloud => language.text("Cloud", "云服务器"),
            Self::Database => language.text("Database", "数据库"),
            Self::Terminal => language.text("Terminal", "终端"),
            Self::Cpu => language.text("Compute", "计算节点"),
            Self::HardDrive => language.text("Storage", "存储"),
            Self::Monitor => language.text("Workstation", "工作站"),
            Self::Globe => language.text("Web / Edge", "公网节点"),
            Self::Shield => language.text("Bastion", "堡垒机"),
            Self::Network => language.text("Network", "网络设备"),
            Self::Code => language.text("Dev", "开发机"),
            Self::Lock => language.text("Security", "安全节点"),
        }
    }
}

fn default_baud_rate() -> u32 {
    115200
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProfile {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub protocol: HostProtocol,
    #[serde(default)]
    pub icon: HostIcon,
    pub address: String,
    pub username: String,
    pub port: u16,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default)]
    pub identity_file: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub jump_host: String,
    #[serde(default)]
    pub jump_host_id: Option<u64>,
    #[serde(default)]
    pub jump_relay_command: String,
    #[serde(default)]
    pub jump_relay_password: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub last_connected: Option<u64>,
}

impl HostProfile {
    pub fn endpoint(&self) -> String {
        match self.protocol {
            HostProtocol::Ssh => {
                let address = if self.address.contains(':') {
                    format!("[{}]", self.address)
                } else {
                    self.address.clone()
                };
                format!("{}@{}:{}", self.username, address, self.port)
            }
            HostProtocol::Telnet => format!("telnet://{}:{}", self.address, self.port),
            HostProtocol::Serial => format!("{} @ {}", self.address, self.baud_rate),
            HostProtocol::Mosh => format!("mosh://{}@{}:{}", self.username, self.address, self.port),
        }
    }

    pub fn ssh_jump_target(&self) -> String {
        let address = if self.address.contains(':') {
            format!("[{}]", self.address)
        } else {
            self.address.clone()
        };
        if self.username.is_empty() {
            format!("{address}:{}", self.port)
        } else {
            format!("{}@{address}:{}", self.username, self.port)
        }
    }
}

pub fn group_hosts<'a>(hosts: impl IntoIterator<Item = &'a HostProfile>) -> Vec<(String, Vec<&'a HostProfile>)> {
    let mut by_group: std::collections::BTreeMap<String, Vec<&'a HostProfile>> =
        std::collections::BTreeMap::new();
    for host in hosts {
        by_group
            .entry(host.group.trim().to_owned())
            .or_default()
            .push(host);
    }
    let mut groups: Vec<_> = by_group.into_iter().collect();
    for (_, items) in &mut groups {
        items.sort_by_cached_key(|host| host.name.to_lowercase());
    }
    // Put ungrouped hosts ("") first, then named groups alphabetically.
    groups.sort_by(|(a, _), (b, _)| match (a.is_empty(), b.is_empty()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    });
    groups
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub id: u64,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub group: String,
    #[serde(default = "default_true")]
    pub add_newline: bool,
}

fn default_true() -> bool {
    true
}

impl Snippet {
    pub fn payload(&self) -> Vec<u8> {
        let mut bytes = self.command.as_bytes().to_vec();
        if self.add_newline && !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        bytes
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub opacity: u8,
    pub font_family: String,
    pub font_size: u8,
    pub hosts: Vec<HostProfile>,
    pub next_host_id: u64,
    pub snippets: Vec<Snippet>,
    pub next_snippet_id: u64,
    pub tunnels: Vec<PortForwardRule>,
    pub next_tunnel_id: u64,
    pub shell: ShellSettings,
    pub language: Language,
    pub collapsed_groups: Vec<String>,
    pub sidebar_width: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            opacity: 100,
            font_family: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            hosts: Vec::new(),
            next_host_id: 1,
            snippets: Vec::new(),
            next_snippet_id: 1,
            tunnels: Vec::new(),
            next_tunnel_id: 1,
            shell: ShellSettings::default(),
            language: Language::default(),
            collapsed_groups: Vec::new(),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        }
    }
}

impl Settings {
    pub fn host_ids(&self, count: usize) -> Option<Range<u64>> {
        let count = u64::try_from(count).ok()?;
        let after_existing = self
            .hosts
            .iter()
            .map(|host| host.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        let start = self.next_host_id.max(after_existing);
        Some(start..start.checked_add(count)?)
    }

    pub fn snippet_ids(&self, count: usize) -> Option<Range<u64>> {
        let count = u64::try_from(count).ok()?;
        let after_existing = self
            .snippets
            .iter()
            .map(|snippet| snippet.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        let start = self.next_snippet_id.max(after_existing);
        Some(start..start.checked_add(count)?)
    }

    pub fn tunnel_ids(&self, count: usize) -> Option<Range<u64>> {
        let count = u64::try_from(count).ok()?;
        let after_existing = self
            .tunnels
            .iter()
            .map(|rule| rule.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        let start = self.next_tunnel_id.max(after_existing);
        Some(start..start.checked_add(count)?)
    }
}

pub struct SettingsStore {
    path: Result<PathBuf, String>,
    pub current: Settings,
}

impl SettingsStore {
    pub fn open() -> Self {
        let path = dirs::config_dir().map(|parent| parent.join("Bulwark").join("settings.json"));
        match path.map(Self::load) {
            Some(Ok(store)) => store,
            Some(Err(error)) => Self::unavailable(format!("Could not read settings. {error}")),
            None => Self::unavailable("Could not locate the application settings folder."),
        }
    }

    fn unavailable(error: impl Into<String>) -> Self {
        Self {
            path: Err(error.into()),
            current: Settings::default(),
        }
    }

    fn load(path: PathBuf) -> io::Result<Self> {
        let mut current: Settings = match File::open(&path) {
            Ok(file) => serde_json::from_reader(file)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(error) => return Err(error),
        };
        current.opacity = current.opacity.clamp(MIN_OPACITY, 100);
        current.font_size = current.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        current.sidebar_width = clamp_sidebar_width(current.sidebar_width);
        Ok(Self {
            path: Ok(path),
            current,
        })
    }

    pub fn load_error(&self) -> Option<&str> {
        self.path.as_ref().err().map(String::as_str)
    }

    pub fn update(&mut self, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
        let path = self.path.as_ref().map_err(Clone::clone)?;
        let mut next = self.current.clone();
        change(&mut next);
        Self::write(path, &next).map_err(|error| format!("Could not save settings. {error}"))?;
        self.current = next;
        Ok(())
    }

    fn write(path: &Path, next: &Settings) -> io::Result<()> {
        let parent = path.parent().expect("settings path includes its directory");
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(file.as_file_mut(), next)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_appearance_survive_reopening_the_store() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Bulwark").join("settings.json");
        let mut store = SettingsStore::load(path.clone()).unwrap();
        let mut host = HostProfile {
            id: 1,
            name: "Build host".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: "build.example.com".into(),
            username: "builder".into(),
            port: 2222,
            baud_rate: 115200,
            identity_file: "~/.ssh/build".into(),
            password: String::new(),
            jump_host: "gateway.example.com".into(),
            jump_host_id: None,
            jump_relay_command: "su - admin".into(),
            jump_relay_password: String::new(),
            group: "Production".into(),
            tags: vec!["build".into(), "ci".into()],
            last_connected: None,
        };
        store
            .update(|settings| {
                settings.theme = Theme::Midnight;
                settings.opacity = 70;
                settings.hosts.push(host.clone());
            })
            .unwrap();

        let mut reopened = SettingsStore::load(path.clone()).unwrap();
        assert_eq!(reopened.current.theme, Theme::Midnight);
        assert_eq!(reopened.current.opacity, 70);
        assert_eq!(reopened.current.hosts, [host.clone()]);
        host.port = 22;
        reopened
            .update(|settings| settings.hosts[0] = host.clone())
            .unwrap();
        assert_eq!(
            SettingsStore::load(path.clone()).unwrap().current.hosts,
            [host]
        );
        reopened.update(|settings| settings.hosts.clear()).unwrap();
        assert!(SettingsStore::load(path).unwrap().current.hosts.is_empty());
    }

    #[test]
    fn failed_write_preserves_live_settings() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("file");
        File::create(&parent).unwrap();
        let mut store = SettingsStore {
            path: Ok(parent.join("settings.json")),
            current: Settings::default(),
        };

        assert!(store.update(|settings| settings.opacity = 50).is_err());
        assert_eq!(store.current.opacity, 100);
    }

    #[test]
    fn settings_without_shell_and_language_use_the_original_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, r#"{"theme":"paper","opacity":80,"hosts":[]}"#).unwrap();
        let mut store = SettingsStore::load(path.clone()).unwrap();
        assert_eq!(store.current.theme, Theme::Paper);
        assert_eq!(store.current.shell, ShellSettings::default());
        assert_eq!(store.current.language, Language::English);

        store
            .update(|settings| {
                settings.shell = ShellSettings {
                    executable: "/bin/sh".into(),
                    login: false,
                };
                settings.language = Language::ChineseSimplified;
            })
            .unwrap();
        let reopened = SettingsStore::load(path).unwrap();
        assert_eq!(reopened.current.shell, store.current.shell);
        assert_eq!(reopened.current.language, Language::ChineseSimplified);
    }

    #[test]
    fn sidebar_width_is_persisted_and_clamped() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, r#"{"theme":"graphite"}"#).unwrap();

        let mut store = SettingsStore::load(path.clone()).unwrap();
        assert_eq!(store.current.sidebar_width, DEFAULT_SIDEBAR_WIDTH);

        store
            .update(|settings| settings.sidebar_width = 275.4)
            .unwrap();
        let reopened = SettingsStore::load(path.clone()).unwrap();
        assert_eq!(reopened.current.sidebar_width, 275.0);

        fs::write(&path, r#"{"sidebar_width": 40.0}"#).unwrap();
        assert_eq!(
            SettingsStore::load(path.clone()).unwrap().current.sidebar_width,
            MIN_SIDEBAR_WIDTH
        );

        fs::write(&path, r#"{"sidebar_width": 999.0}"#).unwrap();
        assert_eq!(
            SettingsStore::load(path).unwrap().current.sidebar_width,
            MAX_SIDEBAR_WIDTH
        );
        assert_eq!(clamp_sidebar_width(f32::NAN), DEFAULT_SIDEBAR_WIDTH);
    }
}
