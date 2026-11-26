use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::net::Ipv6Addr;
use std::path::PathBuf;

use ssh2_config::{ParseRule, SshConfig};

use crate::settings::{HostIcon, HostProfile, HostProtocol, Language};

pub(super) fn validate_profile(
    id: u64,
    values: [String; 8],
    language: Language,
) -> Result<HostProfile, &'static str> {
    validate_profile_for(
        id,
        HostProtocol::Ssh,
        HostIcon::Server,
        values,
        String::new(),
        None,
        String::new(),
        String::new(),
        language,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_profile_for(
    id: u64,
    protocol: HostProtocol,
    icon: HostIcon,
    values: [String; 8],
    password: String,
    jump_host_id: Option<u64>,
    jump_relay_command: String,
    jump_relay_password: String,
    language: Language,
) -> Result<HostProfile, &'static str> {
    let [
        name,
        address,
        username,
        port,
        identity_file,
        jump_host,
        group,
        tags,
    ] = values;
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        return Err(language.text("Enter a name for this host.", "请输入主机名称。"));
    }

    let (address, username, port, baud_rate, jump_host) = match protocol {
        HostProtocol::Serial => {
            if address.trim().is_empty() || address.chars().any(char::is_control) {
                return Err(language.text(
                    "Enter a serial device path (e.g. /dev/tty.usbserial or COM3).",
                    "请输入串口设备路径（如 /dev/tty.usbserial 或 COM3）。",
                ));
            }
            let baud = port
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|b| *b > 0)
                .ok_or_else(|| {
                    language.text("Enter a valid baud rate (e.g. 115200).", "请输入有效的波特率（如 115200）。")
                })?;
            (address.trim().to_owned(), String::new(), 22, baud, String::new())
        }
        HostProtocol::Telnet => {
            let addr = host_address(&address).ok_or_else(|| {
                language.text(
                    "Enter a hostname or IP address.",
                    "请输入主机名或 IP 地址。",
                )
            })?;
            let port = valid_port(&port).ok_or_else(|| {
                language.text(
                    "Enter a port between 1 and 65535.",
                    "请输入 1 到 65535 之间的端口。",
                )
            })?;
            (addr.to_owned(), username.trim().to_owned(), port, 115200, String::new())
        }
        HostProtocol::Ssh | HostProtocol::Mosh => {
            let addr = host_address(&address).ok_or_else(|| {
                language.text(
                    "Enter a hostname or IP address. Use the Port field for the port.",
                    "请输入主机名或 IP 地址，端口请填在端口栏。",
                )
            })?;
            if !valid_user(&username) {
                return Err(language.text("Enter an SSH username.", "请输入 SSH 用户名。"));
            }
            let port = valid_port(&port).ok_or_else(|| {
                language.text(
                    "Enter a port between 1 and 65535.",
                    "请输入 1 到 65535 之间的端口。",
                )
            })?;
            let jump_host = normalize_jumps(&jump_host).ok_or_else(|| {
                language.text(
                    "Enter jump hosts as user@host:port, separated by commas.",
                    "请输入形如 user@host:port 的跳板机，多台用逗号分隔。",
                )
            })?;
            (addr.to_owned(), username, port, 115200, jump_host)
        }
    };

    if identity_file.chars().any(char::is_control) {
        return Err(language.text(
            "Enter a valid identity file path.",
            "请输入有效的私钥文件路径。",
        ));
    }
    let group = group.trim();
    if group.chars().any(char::is_control) {
        return Err(language.text("Enter a valid group name.", "请输入有效的分组名称。"));
    }
    let jump_relay_command = if protocol == HostProtocol::Ssh && !jump_host.is_empty() {
        let trimmed = jump_relay_command.trim();
        if trimmed
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
        {
            return Err(language.text(
                "Enter a valid jump relay command.",
                "请输入有效的中继命令。",
            ));
        }
        trimmed.to_owned()
    } else {
        String::new()
    };
    let jump_relay_password = if !jump_relay_command.is_empty() {
        jump_relay_password
    } else {
        String::new()
    };
    Ok(HostProfile {
        id,
        name: name.trim().into(),
        protocol,
        icon,
        address,
        username,
        port,
        baud_rate,
        keychain_id: None,
        identity_file,
        password,
        jump_host,
        jump_host_id,
        jump_relay_command,
        jump_relay_keychain_id: None,
        jump_relay_password,
        group: group.into(),
        tags: parse_tags(&tags),
        last_connected: None,
    })
}

pub(super) fn parse_tags(raw: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    raw.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty() && !tag.chars().any(char::is_control))
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .map(str::to_owned)
        .collect()
}

fn host_address(value: &str) -> Option<&str> {
    let bare = if let Some(rest) = value.strip_prefix('[') {
        let inner = rest.strip_suffix(']')?;
        if !inner.contains(':') {
            return None;
        }
        inner
    } else {
        value
    };
    if bare.contains(':') {
        let (address, valid_scope) = match bare.split_once('%') {
            Some((address, scope)) => (
                address,
                !scope.is_empty() && scope.chars().all(host_character),
            ),
            None => (bare, true),
        };
        (valid_scope && address.parse::<Ipv6Addr>().is_ok()).then_some(bare)
    } else {
        let first = bare.chars().next()?;
        ((first.is_ascii_alphanumeric() || first == '_') && bare.chars().all(host_character))
            .then_some(bare)
    }
}

fn host_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || "._-".contains(character)
}

fn valid_user(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|character| {
            character.is_whitespace() || character.is_control() || character == '@'
        })
}

fn valid_port(value: &str) -> Option<u16> {
    value.parse::<u16>().ok().filter(|port| *port != 0)
}

fn normalize_jumps(value: &str) -> Option<String> {
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        return Some(String::new());
    }
    value
        .split(',')
        .map(str::trim)
        .map(|item| {
            let destination = item.strip_prefix("ssh://").unwrap_or(item);
            let endpoint = if let Some((user, endpoint)) = destination.split_once('@') {
                if !valid_user(user) {
                    return None;
                }
                endpoint
            } else {
                destination
            };
            if host_address(endpoint).is_some() {
                return Some(item);
            }
            let (address, port) = endpoint.rsplit_once(':')?;
            // An IPv6 address needs brackets when a port is given.
            if address.contains(':') && !address.starts_with('[') {
                return None;
            }
            host_address(address)?;
            valid_port(port)?;
            Some(item)
        })
        .collect::<Option<Vec<_>>>()
        .map(|items| items.join(","))
}

pub(super) fn copy_command(host: &HostProfile) -> String {
    let mut args = vec![
        "ssh".into(),
        "-l".into(),
        host.username.clone(),
        "-p".into(),
        host.port.to_string(),
    ];
    if !host.identity_file.is_empty() {
        args.extend(["-i".into(), host.identity_file.clone()]);
    }
    if !host.jump_host.is_empty() {
        args.extend(["-J".into(), host.jump_host.clone()]);
    }
    args.extend(["--".into(), host.address.clone()]);
    shell_words::join(args)
}

#[cfg(test)]
pub(super) fn host_matches(host: &HostProfile, query: &str) -> bool {
    let name = host.name.to_lowercase();
    let endpoint = host.endpoint().to_lowercase();
    let jump = host.jump_host.to_lowercase();
    let group = host.group.to_lowercase();
    let tags: Vec<_> = host.tags.iter().map(|t| t.to_lowercase()).collect();
    query.split_whitespace().all(|term| {
        name.contains(term)
            || endpoint.contains(term)
            || jump.contains(term)
            || group.contains(term)
            || tags.iter().any(|t| t.contains(term))
    })
}

pub(super) struct ImportBatch {
    pub profiles: Vec<HostProfile>,
    pub skipped: usize,
}

pub(super) fn read_configs(paths: Vec<PathBuf>, language: Language) -> Result<ImportBatch, String> {
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let mut batch = ImportBatch {
        profiles: Vec::new(),
        skipped: 0,
    };
    let mut aliases = HashSet::new();
    for path in paths {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        let metadata = path
            .metadata()
            .map_err(|error| format!("{name}: {error}"))?;
        if !metadata.is_file() {
            return Err(format!(
                "{name}: {}",
                language.text("Select a regular file.", "请选择普通文件。")
            ));
        }
        let file = File::open(&path).map_err(|error| format!("{name}: {error}"))?;
        let config = SshConfig::default()
            .parse(
                &mut BufReader::new(file),
                ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS,
            )
            .map_err(|error| format!("{name}: {error}"))?;
        if config.get_hosts().iter().any(|host| {
            host.params
                .ignored_fields
                .keys()
                .any(|key| key.eq_ignore_ascii_case("match"))
        }) {
            return Err(format!(
                "{name}: {}",
                language.text(
                    "Import requires static host entries. This file contains a Match section.",
                    "此文件包含 Match 条件段，请选择只含静态主机条目的配置。",
                )
            ));
        }
        collect_profiles(&config, &username, language, &mut aliases, &mut batch);
    }
    Ok(batch)
}

fn collect_profiles(
    config: &SshConfig,
    username: &str,
    language: Language,
    aliases: &mut HashSet<String>,
    batch: &mut ImportBatch,
) {
    for alias in config.get_hosts().iter().flat_map(|host| &host.pattern) {
        if alias.negated || alias.pattern.contains(['*', '?', '!']) {
            continue;
        }
        let alias = &alias.pattern;
        if !aliases.insert(alias.to_ascii_lowercase()) {
            continue;
        }
        let params = config.query(alias);
        // A ProxyCommand is a program, and a profile only represents ProxyJump.
        if params
            .unsupported_fields
            .get("proxycommand")
            .is_some_and(|args| {
                !args
                    .first()
                    .is_some_and(|value| value.eq_ignore_ascii_case("none"))
            })
        {
            batch.skipped += 1;
            continue;
        }
        let address = params
            .host_name
            .as_deref()
            .unwrap_or(alias)
            .replace("%h", alias);
        let identity = params
            .identity_file
            .as_ref()
            .and_then(|files| files.first())
            .map(|path| path.to_string_lossy().into_owned())
            .filter(|path| !path.eq_ignore_ascii_case("none"))
            .unwrap_or_default();
        let values = [
            alias.clone(),
            address,
            params.user.unwrap_or_else(|| username.into()),
            params.port.unwrap_or(22).to_string(),
            identity,
            params
                .proxy_jump
                .map(|jumps| jumps.join(","))
                .unwrap_or_default(),
            String::new(),
            String::new(),
        ];
        match validate_profile(0, values, language) {
            Ok(host) => batch.profiles.push(host),
            Err(_) => batch.skipped += 1,
        }
    }
}

pub(super) fn merge_candidates(
    existing: &[HostProfile],
    batch: &ImportBatch,
) -> (Vec<HostProfile>, usize) {
    let names: HashSet<_> = existing
        .iter()
        .map(|host| host.name.to_lowercase())
        .collect();
    let candidates: Vec<_> = batch
        .profiles
        .iter()
        .filter(|host| !names.contains(&host.name.to_lowercase()))
        .cloned()
        .collect();
    let skipped = batch.skipped + batch.profiles.len() - candidates.len();
    (candidates, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Language::English;

    fn values(address: &str, port: &str, jumps: &str) -> [String; 8] {
        [
            "Host",
            address,
            "dev",
            port,
            "~/.ssh/with space",
            jumps,
            "Production",
            "prod, api, PROD",
        ]
        .map(str::to_owned)
    }

    #[test]
    fn validates_ipv6_ports_and_jump_chain() {
        let host = validate_profile(
            1,
            values("[fe80::1%en0]", "2222", "user@edge, [::1]:22"),
            English,
        )
        .unwrap();
        assert_eq!(host.address, "fe80::1%en0");
        assert_eq!(host.jump_host, "user@edge,[::1]:22");
        assert_eq!(host.group, "Production");
        assert_eq!(host.tags, ["prod", "api"]);
        assert!(host_matches(&host, "production api"));
        assert_eq!(host.endpoint(), "dev@[fe80::1%en0]:2222");
        assert!(validate_profile(1, values("https://example.com", "22", ""), English).is_err());
        assert!(validate_profile(1, values("example.com", "0", ""), English).is_err());
        assert!(validate_profile(1, values("example.com", "65536", ""), English).is_err());
        assert!(validate_profile(1, values("example.com", "22", "edge:0"), English).is_err());
        assert_eq!(
            shell_words::split(&copy_command(&host)).unwrap(),
            [
                "ssh",
                "-l",
                "dev",
                "-p",
                "2222",
                "-i",
                "~/.ssh/with space",
                "-J",
                "user@edge,[::1]:22",
                "--",
                "fe80::1%en0",
            ]
        );
    }

    #[test]
    fn imports_named_aliases_and_skips_existing_names() {
        let mut input = "\
Host app mirror
  HostName %h.example.com
  Port 2202
  ProxyJump gateway
Host *
  User ops
"
        .as_bytes();
        let config = SshConfig::default()
            .parse(&mut input, ParseRule::STRICT)
            .unwrap();
        let mut batch = ImportBatch {
            profiles: Vec::new(),
            skipped: 0,
        };
        collect_profiles(
            &config,
            "fallback",
            English,
            &mut HashSet::new(),
            &mut batch,
        );
        assert_eq!(batch.profiles.len(), 2);
        assert_eq!(batch.profiles[0].endpoint(), "ops@app.example.com:2202");
        assert_eq!(batch.profiles[0].jump_host, "gateway");
        let (candidates, skipped) = merge_candidates(&batch.profiles[..1], &batch);
        assert_eq!(skipped, 1);
        assert_eq!(candidates[0].name, "mirror");
    }

    #[test]
    fn validates_jump_relay_command_and_clears_without_jump_host() {
        let with_jump = validate_profile_for(
            1,
            HostProtocol::Ssh,
            HostIcon::Server,
            values("10.0.0.10", "22", "ops@jump.example.com:22"),
            String::new(),
            Some(99),
            "  su - admin  ".into(),
            "enc:v1:relaypw".into(),
            English,
        )
        .unwrap();
        assert_eq!(with_jump.jump_relay_command, "su - admin");
        assert_eq!(with_jump.jump_relay_password, "enc:v1:relaypw");

        let without_jump = validate_profile_for(
            2,
            HostProtocol::Ssh,
            HostIcon::Server,
            values("10.0.0.10", "22", ""),
            String::new(),
            None,
            "su - admin".into(),
            "enc:v1:relaypw".into(),
            English,
        )
        .unwrap();
        assert!(without_jump.jump_relay_command.is_empty());
        assert!(without_jump.jump_relay_password.is_empty());
    }
}
