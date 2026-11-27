use std::collections::BTreeMap;
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

const SETTINGS_FILE: &str = "settings.json";
const SESSION_FILE: &str = "session.json";
pub const LOCAL_KEY_FILE: &str = ".host_key";

pub fn config_dir() -> io::Result<PathBuf> {
    let parent = dirs::config_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not locate the application config directory.",
        )
    })?;
    migrate_config_dir(&parent)
}

fn migrate_config_dir(parent: &Path) -> io::Result<PathBuf> {
    let path = parent.join("Portus");
    let legacy = parent.join("Bulwark");
    if path.try_exists()? || !legacy.try_exists()? {
        return Ok(path);
    }

    // Publish settings and the key together so either entry point can start
    // safely while another process is migrating. Keep the old files intact.
    let staging = tempfile::tempdir_in(parent)?;
    for name in [SETTINGS_FILE, LOCAL_KEY_FILE] {
        match fs::copy(legacy.join(name), staging.path().join(name)) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    match fs::rename(staging.path(), &path) {
        Ok(()) => Ok(path),
        Err(_) if path.is_dir() => Ok(path),
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineHeightPreset {
    Compact,
    #[default]
    Standard,
    Relaxed,
}

impl LineHeightPreset {
    pub fn factor(self) -> f32 {
        match self {
            Self::Compact => 1.4,
            Self::Standard => 1.6,
            Self::Relaxed => 1.8,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

impl CursorStyle {
    pub fn to_terminal(self) -> gpui_terminal::TerminalCursorStyle {
        match self {
            Self::Block => gpui_terminal::TerminalCursorStyle::Block,
            Self::Bar => gpui_terminal::TerminalCursorStyle::Bar,
            Self::Underline => gpui_terminal::TerminalCursorStyle::Underline,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BellMode {
    Off,
    #[default]
    Visual,
    Sound,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollbackLimit {
    Lines5000,
    #[default]
    Lines10000,
    Lines50000,
    Unlimited,
}

impl ScrollbackLimit {
    pub fn lines(self) -> usize {
        match self {
            Self::Lines5000 => 5_000,
            Self::Lines10000 => 10_000,
            Self::Lines50000 => 50_000,
            // Dynamically allocated by alacritty_terminal; capped safely below i32::MAX.
            Self::Unlimited => 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyPolicy {
    #[default]
    Ask,
    Strict,
    Accept,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SftpConflictPolicy {
    #[default]
    Ask,
    Overwrite,
    Skip,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ShortcutBinding {
    pub id: String,
    pub keys: String,
}

pub fn default_shortcuts() -> Vec<ShortcutBinding> {
    [
        ("cmdk", "⌘K"),
        ("new_tab", "⌘T"),
        ("split_v", "⌘D"),
        ("split_h", "⇧⌘D"),
        ("close_tab", "⌘W"),
        ("drawer_sftp", "⌘E"),
        ("drawer_snippets", "⇧⌘S"),
        ("settings", "⌘,"),
    ]
    .into_iter()
    .map(|(id, keys)| ShortcutBinding {
        id: id.to_owned(),
        keys: keys.to_owned(),
    })
    .collect()
}

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

impl HostProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Telnet => "Telnet",
            Self::Serial => "Serial",
            Self::Mosh => "Mosh",
        }
    }
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
}

fn default_baud_rate() -> u32 {
    115200
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeychainKind {
    #[default]
    SshKey,
    Password,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeychainEntry {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub kind: KeychainKind,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub notes: String,
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
    pub keychain_id: Option<u64>,
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
    pub jump_relay_keychain_id: Option<u64>,
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

    pub fn references_keychain(&self, keychain_id: u64) -> bool {
        self.keychain_id == Some(keychain_id)
            || self.jump_relay_keychain_id == Some(keychain_id)
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

/// Snippets available when Portus is first configured.
///
/// They live in the regular settings collection so users can edit, duplicate,
/// or remove them just like any other snippet. Existing settings files keep
/// their own snippet list when they already contain one.
pub fn default_snippets() -> Vec<Snippet> {
    [
        (
            "Docker resource usage",
            "docker stats --no-stream --format \"table {{.Name}}\\t{{.CPUPerc}}\\t{{.MemUsage}}\"",
            "Docker",
        ),
        ("Follow service logs", "journalctl -f -n 50", "Logs"),
        ("Listening ports", "ss -tulpn | grep LISTEN", "Diagnostics"),
        (
            "Disk usage top 10",
            "du -sh * | sort -rh | head -n 10",
            "Diagnostics",
        ),
        (
            "Validate and reload Nginx",
            "sudo nginx -t && sudo systemctl reload nginx",
            "Web",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, command, group))| Snippet {
        id: (index + 1) as u64,
        name: name.to_owned(),
        command: command.to_owned(),
        group: group.to_owned(),
        add_newline: true,
    })
    .collect()
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

pub const DEFAULT_MCP_PORT: u16 = 7331;
pub const MIN_MCP_PORT: u16 = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    #[default]
    Http,
    Stdio,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpHostGrant {
    #[default]
    Deny,
    Readonly,
    Approval,
    Allow,
}

impl McpHostGrant {
    pub const ALL: [Self; 4] = [Self::Deny, Self::Readonly, Self::Approval, Self::Allow];

    pub fn label(self, language: Language) -> &'static str {
        match self {
            Self::Deny => language.text("Deny", "禁止"),
            Self::Readonly => language.text("Read-only", "只读"),
            Self::Approval => language.text("Approval", "需审批"),
            Self::Allow => language.text("Allow", "允许"),
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::Readonly => "readonly",
            Self::Approval => "approval",
            Self::Allow => "allow",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpGuardAction {
    Deny,
    #[default]
    Approval,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct McpGuardRule {
    pub id: u64,
    pub pattern: String,
    #[serde(default)]
    pub action: McpGuardAction,
    pub label: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

pub fn default_mcp_guard_rules() -> Vec<McpGuardRule> {
    vec![
        McpGuardRule {
            id: 1,
            pattern: "rm -rf|mkfs|dd if=".into(),
            action: McpGuardAction::Deny,
            label: "破坏性磁盘 / 删除命令".into(),
            enabled: true,
        },
        McpGuardRule {
            id: 2,
            pattern: "systemctl (stop|restart|reload)".into(),
            action: McpGuardAction::Approval,
            label: "服务启停与重载".into(),
            enabled: true,
        },
        McpGuardRule {
            id: 3,
            pattern: "reboot|shutdown|poweroff".into(),
            action: McpGuardAction::Deny,
            label: "系统关机 / 重启".into(),
            enabled: true,
        },
        McpGuardRule {
            id: 4,
            pattern: "chmod 777|chown -R".into(),
            action: McpGuardAction::Approval,
            label: "大范围权限变更".into(),
            enabled: true,
        },
        McpGuardRule {
            id: 5,
            pattern: "DROP (TABLE|DATABASE)|TRUNCATE".into(),
            action: McpGuardAction::Deny,
            label: "高危数据库变更".into(),
            enabled: true,
        },
    ]
}

pub fn default_mcp_disabled_tools() -> Vec<String> {
    vec!["sftp_write".to_owned()]
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct McpSettings {
    pub enabled: bool,
    pub transport: McpTransport,
    pub port: u16,
    pub session_ttl_mins: u16,
    pub mask_secrets: bool,
    pub disabled_tools: Vec<String>,
    pub host_grants: BTreeMap<u64, McpHostGrant>,
    pub guard_rules: Vec<McpGuardRule>,
    pub next_guard_rule_id: u64,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            transport: McpTransport::Http,
            port: DEFAULT_MCP_PORT,
            session_ttl_mins: 30,
            mask_secrets: true,
            disabled_tools: default_mcp_disabled_tools(),
            host_grants: BTreeMap::new(),
            guard_rules: default_mcp_guard_rules(),
            next_guard_rule_id: 6,
        }
    }
}

impl McpSettings {
    pub fn is_tool_enabled(&self, tool_id: &str) -> bool {
        !self.disabled_tools.iter().any(|t| t == tool_id)
    }

    pub fn set_tool_enabled(&mut self, tool_id: &str, enabled: bool) {
        if enabled {
            self.disabled_tools.retain(|t| t != tool_id);
        } else if !self.disabled_tools.iter().any(|t| t == tool_id) {
            self.disabled_tools.push(tool_id.to_owned());
        }
    }

    pub fn host_grant(&self, host_id: u64) -> McpHostGrant {
        self.host_grants
            .get(&host_id)
            .copied()
            .unwrap_or(McpHostGrant::Deny)
    }

    pub fn next_rule_id(&mut self) -> u64 {
        let after = self
            .guard_rules
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let id = self.next_guard_rule_id.max(after);
        self.next_guard_rule_id = id.saturating_add(1);
        id
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncProvider {
    #[default]
    None,
    S3,
    WebDav,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncConflictStrategy {
    #[default]
    Overwrite,
    Merge,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct S3SyncConfig {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    /// Encrypted locally via `vault::encrypt_secret`.
    pub secret_access_key: String,
    pub object_key: String,
    pub path_style: bool,
}

impl Default for S3SyncConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            region: "us-east-1".to_owned(),
            bucket: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            object_key: "portus/config.json".to_owned(),
            path_style: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct WebDavSyncConfig {
    pub endpoint: String,
    pub username: String,
    /// Encrypted locally via `vault::encrypt_secret`.
    pub password: String,
    pub remote_path: String,
}

impl Default for WebDavSyncConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            username: String::new(),
            password: String::new(),
            remote_path: "portus-config.json".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct SyncSettings {
    pub provider: SyncProvider,
    pub auto_sync: bool,
    pub conflict_strategy: SyncConflictStrategy,
    /// Optional E2EE passphrase (encrypted locally via `vault::encrypt_secret`).
    pub encryption_passphrase: String,
    pub s3: S3SyncConfig,
    pub webdav: WebDavSyncConfig,
    pub last_sync_at: Option<u64>,
    pub last_sync_hash: String,
    pub device_id: String,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            provider: SyncProvider::None,
            auto_sync: false,
            conflict_strategy: SyncConflictStrategy::Overwrite,
            encryption_passphrase: String::new(),
            s3: S3SyncConfig::default(),
            webdav: WebDavSyncConfig::default(),
            last_sync_at: None,
            last_sync_hash: String::new(),
            device_id: String::new(),
        }
    }
}

impl SyncSettings {
    pub fn is_configured(&self) -> bool {
        match self.provider {
            SyncProvider::None => false,
            SyncProvider::S3 => {
                !self.s3.endpoint.trim().is_empty()
                    && !self.s3.bucket.trim().is_empty()
                    && !self.s3.access_key_id.trim().is_empty()
                    && !self.s3.secret_access_key.is_empty()
            }
            SyncProvider::WebDav => !self.webdav.endpoint.trim().is_empty(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub opacity: u8,
    pub font_family: String,
    pub font_size: u8,
    pub line_height: LineHeightPreset,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub copy_on_select: bool,
    pub option_as_meta: bool,
    pub bell: BellMode,
    pub scrollback: ScrollbackLimit,
    pub keep_alive_secs: u16,
    pub connect_timeout_secs: u16,
    pub auto_reconnect: bool,
    pub host_key_policy: HostKeyPolicy,
    pub agent_forward: bool,
    pub sftp_download_dir: String,
    pub sftp_conflict: SftpConflictPolicy,
    pub sftp_concurrency: u8,
    pub sftp_show_hidden: bool,
    pub touch_id_unlock: bool,
    pub auto_lock_mins: u16,
    pub hide_secrets_on_blur: bool,
    pub shortcuts: Vec<ShortcutBinding>,
    pub auto_check_updates: bool,
    pub hosts: Vec<HostProfile>,
    pub next_host_id: u64,
    pub keychain: Vec<KeychainEntry>,
    pub next_keychain_id: u64,
    pub snippets: Vec<Snippet>,
    pub next_snippet_id: u64,
    pub tunnels: Vec<PortForwardRule>,
    pub next_tunnel_id: u64,
    pub shell: ShellSettings,
    pub language: Language,
    pub collapsed_groups: Vec<String>,
    pub sidebar_width: f32,
    pub mcp: McpSettings,
    pub sync: SyncSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            opacity: 100,
            font_family: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            line_height: LineHeightPreset::default(),
            cursor_style: CursorStyle::default(),
            cursor_blink: true,
            copy_on_select: true,
            option_as_meta: true,
            bell: BellMode::default(),
            scrollback: ScrollbackLimit::default(),
            keep_alive_secs: 30,
            connect_timeout_secs: 15,
            auto_reconnect: true,
            host_key_policy: HostKeyPolicy::default(),
            agent_forward: false,
            sftp_download_dir: "~/Downloads".to_owned(),
            sftp_conflict: SftpConflictPolicy::default(),
            sftp_concurrency: 4,
            sftp_show_hidden: true,
            touch_id_unlock: true,
            auto_lock_mins: 15,
            hide_secrets_on_blur: true,
            shortcuts: default_shortcuts(),
            auto_check_updates: true,
            hosts: Vec::new(),
            next_host_id: 1,
            keychain: Vec::new(),
            next_keychain_id: 1,
            snippets: default_snippets(),
            next_snippet_id: 6,
            tunnels: Vec::new(),
            next_tunnel_id: 1,
            shell: ShellSettings::default(),
            language: Language::default(),
            collapsed_groups: Vec::new(),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            mcp: McpSettings::default(),
            sync: SyncSettings::default(),
        }
    }
}

impl Settings {
    pub fn normalize(&mut self) {
        self.opacity = self.opacity.clamp(MIN_OPACITY, 100);
        self.font_size = self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.sidebar_width = clamp_sidebar_width(self.sidebar_width);
        if self.mcp.port < MIN_MCP_PORT {
            self.mcp.port = DEFAULT_MCP_PORT;
        }
    }
    pub fn resolve_host_auth(&self, host: &HostProfile) -> HostProfile {
        let mut resolved = host.clone();
        if let Some(kc_id) = host.keychain_id
            && let Some(entry) = self.keychain.iter().find(|k| k.id == kc_id)
        {
            match entry.kind {
                KeychainKind::SshKey => {
                    if !entry.key_path.trim().is_empty() {
                        resolved.identity_file = entry.key_path.clone();
                    }
                }
                KeychainKind::Password => {
                    if !entry.secret.is_empty() {
                        resolved.password = entry.secret.clone();
                    }
                }
            }
        }
        if let Some(relay_kc_id) = host.jump_relay_keychain_id
            && let Some(entry) = self.keychain.iter().find(|k| k.id == relay_kc_id)
            && entry.kind == KeychainKind::Password
            && !entry.secret.is_empty()
        {
            resolved.jump_relay_password = entry.secret.clone();
        }
        resolved
    }

    pub fn keychain_ids(&self, count: usize) -> Option<Range<u64>> {
        let count = u64::try_from(count).ok()?;
        let after_existing = self
            .keychain
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        let start = self.next_keychain_id.max(after_existing);
        Some(start..start.checked_add(count)?)
    }
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
        match config_dir().and_then(|path| Self::load(path.join(SETTINGS_FILE))) {
            Ok(store) => store,
            Err(error) => Self::unavailable(format!("Could not read settings. {error}")),
        }
    }

    fn unavailable(error: impl Into<String>) -> Self {
        Self {
            path: Err(error.into()),
            current: Settings::default(),
        }
    }

    fn load(path: PathBuf) -> io::Result<Self> {
        let (mut current, had_next_snippet_id): (Settings, bool) = match File::open(&path) {
            Ok(file) => {
                let document: serde_json::Value = serde_json::from_reader(file)?;
                let had_next_snippet_id = document.get("next_snippet_id").is_some();
                (serde_json::from_value(document)?, had_next_snippet_id)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Settings::default(), false),
            Err(error) => return Err(error),
        };
        current.normalize();
        // Seed installations created before built-in snippets existed. Once a
        // snippet ID has been allocated, an intentionally empty list remains
        // empty so deleting all snippets is respected.
        if current.snippets.is_empty() && (!had_next_snippet_id || current.next_snippet_id <= 1) {
            current.snippets = default_snippets();
            current.next_snippet_id = current
                .snippets
                .iter()
                .map(|snippet| snippet.id)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
        }
        Ok(Self {
            path: Ok(path),
            current,
        })
    }

    pub fn load_error(&self) -> Option<&str> {
        self.path.as_ref().err().map(String::as_str)
    }

    pub fn settings_path(&self) -> Option<&Path> {
        self.path.as_deref().ok()
    }

    pub fn config_dir(&self) -> Option<&Path> {
        self.path.as_deref().ok().and_then(|p| p.parent())
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SavedHostRef {
    Saved { id: u64 },
    AdHoc { host: HostProfile },
}

impl SavedHostRef {
    pub fn from_host(host: &HostProfile) -> Self {
        if host.id > 0 {
            Self::Saved { id: host.id }
        } else {
            let mut sanitized = host.clone();
            sanitized.password.clear();
            sanitized.jump_relay_password.clear();
            Self::AdHoc { host: sanitized }
        }
    }

    pub fn resolve(&self, hosts: &[HostProfile]) -> Option<HostProfile> {
        match self {
            Self::Saved { id } => hosts.iter().find(|h| h.id == *id).cloned(),
            Self::AdHoc { host } => Some(host.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SavedPane {
    Local,
    Host { host: SavedHostRef },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SavedTab {
    Terminal {
        panes: Vec<SavedPane>,
        #[serde(default)]
        active_pane: usize,
    },
    Sftp {
        host: SavedHostRef,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSession {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
    pub in_vault: bool,
}

impl SavedSession {
    pub fn load_from_dir(dir: &Path) -> Option<Self> {
        let file = File::open(dir.join(SESSION_FILE)).ok()?;
        serde_json::from_reader(file).ok()
    }

    pub fn save_to_dir(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let path = dir.join(SESSION_FILE);
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer_pretty(file.as_file_mut(), self)?;
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
    fn migrates_legacy_settings_with_their_key_and_preserves_portus_changes() {
        use crate::vault::{decrypt_secret_with_key, encrypt_secret_with_key};

        let directory = tempfile::tempdir().unwrap();
        let legacy = directory.path().join("Bulwark");
        fs::create_dir(&legacy).unwrap();
        let key = [0x42; 32];
        let sealed = encrypt_secret_with_key("test password", &key).unwrap();
        fs::write(legacy.join(LOCAL_KEY_FILE), key).unwrap();
        fs::write(
            legacy.join(SETTINGS_FILE),
            serde_json::to_vec(&serde_json::json!({
                "opacity": 70,
                "hosts": [{
                    "id": 1, "name": "Web", "address": "web.example.com",
                    "username": "ops", "port": 22, "password": sealed
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let portus = migrate_config_dir(directory.path()).unwrap();
        assert_eq!(portus, directory.path().join("Portus"));
        let path = portus.join(SETTINGS_FILE);
        let mut store = SettingsStore::load(path.clone()).unwrap();
        let migrated_key = fs::read(portus.join(LOCAL_KEY_FILE))
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(store.current.opacity, 70);
        assert_eq!(
            decrypt_secret_with_key(&store.current.hosts[0].password, &migrated_key).as_deref(),
            Some("test password")
        );

        store.update(|settings| settings.opacity = 90).unwrap();
        assert_eq!(migrate_config_dir(directory.path()).unwrap(), portus);
        assert_eq!(SettingsStore::load(path).unwrap().current.opacity, 90);
        assert_eq!(
            SettingsStore::load(legacy.join(SETTINGS_FILE))
                .unwrap()
                .current
                .opacity,
            70
        );
        assert_eq!(fs::read(legacy.join(LOCAL_KEY_FILE)).unwrap(), key);
    }

    #[test]
    fn fresh_settings_include_common_snippets() {
        let settings = Settings::default();

        assert_eq!(settings.snippets, default_snippets());
        assert_eq!(settings.next_snippet_id, 6);
        assert!(settings
            .snippets
            .iter()
            .any(|snippet| snippet.name == "Listening ports"));
    }

    #[test]
    fn old_empty_snippet_lists_are_seeded_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, r#"{"snippets":[]}"#).unwrap();

        let mut store = SettingsStore::load(path.clone()).unwrap();
        assert_eq!(store.current.snippets, default_snippets());
        assert_eq!(store.current.next_snippet_id, 6);

        store
            .update(|settings| {
                settings.snippets.clear();
            })
            .unwrap();
        assert!(SettingsStore::load(path).unwrap().current.snippets.is_empty());
    }

    #[test]
    fn hosts_and_appearance_survive_reopening_the_store() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Portus").join("settings.json");
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
            keychain_id: None,
            identity_file: "~/.ssh/build".into(),
            password: String::new(),
            jump_host: "gateway.example.com".into(),
            jump_host_id: None,
            jump_relay_command: "su - admin".into(),
            jump_relay_keychain_id: None,
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

    #[test]
    fn saved_session_roundtrips_and_sanitizes_adhoc_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let saved_host = HostProfile {
            id: 42,
            name: "prod".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: "10.0.0.1".into(),
            username: "root".into(),
            port: 22,
            baud_rate: 115200,
            keychain_id: None,
            identity_file: "~/.ssh/id_ed25519".into(),
            password: "enc:secret".into(),
            jump_host: String::new(),
            jump_host_id: None,
            jump_relay_command: String::new(),
            jump_relay_keychain_id: None,
            jump_relay_password: String::new(),
            group: String::new(),
            tags: Vec::new(),
            last_connected: None,
        };
        let adhoc_host = HostProfile {
            id: 0,
            name: "quick".into(),
            password: "should-not-be-saved".into(),
            jump_relay_password: "also-not-saved".into(),
            ..saved_host.clone()
        };

        let session = SavedSession {
            tabs: vec![
                SavedTab::Terminal {
                    panes: vec![
                        SavedPane::Local,
                        SavedPane::Host {
                            host: SavedHostRef::from_host(&saved_host),
                        },
                    ],
                    active_pane: 1,
                },
                SavedTab::Terminal {
                    panes: vec![SavedPane::Host {
                        host: SavedHostRef::from_host(&adhoc_host),
                    }],
                    active_pane: 0,
                },
                SavedTab::Sftp {
                    host: SavedHostRef::from_host(&saved_host),
                },
            ],
            active: 2,
            in_vault: false,
        };

        session.save_to_dir(dir.path()).unwrap();
        let loaded = SavedSession::load_from_dir(dir.path()).unwrap();
        assert_eq!(loaded.tabs.len(), 3);
        assert_eq!(loaded.active, 2);
        assert!(!loaded.in_vault);

        // Saved host resolves from current settings; deleted host returns None.
        assert_eq!(
            SavedHostRef::Saved { id: 42 }.resolve(&[saved_host.clone()]),
            Some(saved_host)
        );
        assert_eq!(SavedHostRef::Saved { id: 999 }.resolve(&[]), None);

        // Ad-hoc host must not persist passwords in session.json.
        let SavedTab::Terminal { panes, .. } = &loaded.tabs[1] else {
            panic!("expected terminal tab");
        };
        let SavedPane::Host { host } = &panes[0] else {
            panic!("expected host pane");
        };
        let resolved_adhoc = host.resolve(&[]).unwrap();
        assert!(resolved_adhoc.password.is_empty());
        assert!(resolved_adhoc.jump_relay_password.is_empty());
    }
}
