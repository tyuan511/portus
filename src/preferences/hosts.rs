mod model;

use std::fs;
use std::io::Read as _;

use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    component::{
        Disableable as _, IndexPath, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{InputEvent, InputState},
        select::{Select, SelectEvent, SelectState},
    },
    div, px, rgb, transparent_black,
};

use crate::icons::Icon;
use crate::settings::{
    HostIcon, HostProfile, HostProtocol, Language, Settings, SettingsStore, group_hosts,
};
use crate::theme::Colors;
use crate::vault::{decrypt_secret, encrypt_secret};
use model::{
    ImportBatch, copy_command, host_matches, merge_candidates, read_configs,
    validate_profile_for,
};

use super::{SelectChoice, danger_button, input_field, primary_button, secondary_button};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SshAuthMethod {
    PrivateKey,
    Password,
}

#[derive(Clone, PartialEq, Eq)]
enum JumpChoice {
    None,
    Host(u64),
    Legacy(String),
}

fn discover_ssh_keys() -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let ssh_dir = home.join(".ssh");
    let Ok(entries) = fs::read_dir(&ssh_dir) else {
        return Vec::new();
    };
    let preferred = ["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"];
    let mut found: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.')
            || name.ends_with(".pub")
            || matches!(
                name,
                "config"
                    | "known_hosts"
                    | "known_hosts.old"
                    | "authorized_keys"
                    | "authorized_keys2"
                    | "environment"
                    | "rc"
            )
        {
            continue;
        }
        let is_preferred = preferred.contains(&name);
        let looks_like_key = if is_preferred {
            true
        } else {
            let mut buf = [0u8; 32];
            fs::File::open(&path)
                .and_then(|mut f| f.read(&mut buf))
                .ok()
                .is_some_and(|n| {
                    std::str::from_utf8(&buf[..n])
                        .ok()
                        .is_some_and(|s| s.starts_with("-----BEGIN "))
                })
        };
        if looks_like_key {
            found.push(name.to_owned());
        }
    }
    found.sort_by(|a, b| {
        let pa = preferred.iter().position(|p| p == a).unwrap_or(preferred.len());
        let pb = preferred.iter().position(|p| p == b).unwrap_or(preferred.len());
        pa.cmp(&pb).then_with(|| a.cmp(b))
    });
    found.into_iter().map(|name| format!("~/.ssh/{name}")).collect()
}

fn build_key_select_items(
    current: &str,
    discovered: &[String],
    language: Language,
) -> Vec<SelectChoice<String>> {
    let mut items = Vec::with_capacity(discovered.len() + 2);
    items.push(SelectChoice::new(
        language.text("Auto (scan ~/.ssh)", "自动（默认扫描 ~/.ssh）"),
        String::new(),
    ));
    for path in discovered {
        items.push(SelectChoice::new(path.clone(), path.clone()));
    }
    if !current.is_empty() && !discovered.iter().any(|p| p == current) {
        items.push(SelectChoice::new(current.to_owned(), current.to_owned()));
    }
    items
}

fn build_protocol_items() -> Vec<SelectChoice<HostProtocol>> {
    [
        (HostProtocol::Ssh, "SSH"),
        (HostProtocol::Telnet, "Telnet"),
        (HostProtocol::Serial, "Serial"),
        (HostProtocol::Mosh, "Mosh"),
    ]
    .into_iter()
    .map(|(proto, label)| SelectChoice::new(label, proto))
    .collect()
}

fn build_jump_items(
    existing_hosts: &[HostProfile],
    self_id: Option<u64>,
    legacy_jump: &str,
    language: Language,
) -> Vec<SelectChoice<JumpChoice>> {
    let mut items = vec![SelectChoice::new(
        language.text("None", "无"),
        JumpChoice::None,
    )];
    for host in existing_hosts {
        if host.protocol == HostProtocol::Ssh && Some(host.id) != self_id {
            items.push(SelectChoice::new(
                format!("{} ({})", host.name, host.endpoint()),
                JumpChoice::Host(host.id),
            ));
        }
    }
    if !legacy_jump.is_empty() {
        items.push(SelectChoice::new(
            legacy_jump.to_owned(),
            JumpChoice::Legacy(legacy_jump.to_owned()),
        ));
    }
    items
}

const FIELDS: [(&str, &str, &str); 7] = [
    ("Name", "名称", "Production API"),
    ("Host", "地址", "server.example.com"),
    ("Username", "用户名", "deploy"),
    ("Port", "端口", "22"),
    ("Identity file", "私钥文件", "~/.ssh/id_ed25519"),
    ("Group", "分组", "Production"),
    ("Tags", "标签（逗号分隔）", "prod, api"),
];

struct HostEditor {
    id: Option<u64>,
    protocol: HostProtocol,
    auth_method: SshAuthMethod,
    icon: HostIcon,
    fields: [Entity<InputState>; FIELDS.len()],
    password: Entity<InputState>,
    jump_host_id: Option<u64>,
    legacy_jump_host: String,
    jump_relay_command: Entity<InputState>,
    jump_relay_password: Entity<InputState>,
    discovered_keys: Vec<String>,
    protocol_select: Entity<SelectState<Vec<SelectChoice<HostProtocol>>>>,
    key_select: Entity<SelectState<Vec<SelectChoice<String>>>>,
    jump_select: Entity<SelectState<Vec<SelectChoice<JumpChoice>>>>,
    _select_subs: Vec<Subscription>,
}

impl HostEditor {
    fn new(
        profile: Option<&HostProfile>,
        id: Option<u64>,
        existing_hosts: &[HostProfile],
        language: Language,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let protocol = profile.map(|h| h.protocol).unwrap_or(HostProtocol::Ssh);
        let icon = profile.map(|h| h.icon).unwrap_or_default();
        let discovered_keys = discover_ssh_keys();
        let raw_password = profile
            .and_then(|host| decrypt_secret(&host.password))
            .unwrap_or_default();
        let auth_method = match profile {
            Some(host) if host.identity_file.trim().is_empty() && !raw_password.is_empty() => {
                SshAuthMethod::Password
            }
            _ => SshAuthMethod::PrivateKey,
        };
        let default_identity = match profile {
            Some(host) => host.identity_file.clone(),
            None => discovered_keys.first().cloned().unwrap_or_default(),
        };
        let values = match profile {
            Some(host) => [
                host.name.clone(),
                host.address.clone(),
                host.username.clone(),
                if host.protocol == HostProtocol::Serial {
                    host.baud_rate.to_string()
                } else {
                    host.port.to_string()
                },
                default_identity.clone(),
                host.group.clone(),
                host.tags.join(", "),
            ],
            None => [
                String::new(),
                String::new(),
                String::new(),
                "22".into(),
                default_identity.clone(),
                String::new(),
                String::new(),
            ],
        };
        let mut fields = FIELDS
            .into_iter()
            .zip(values)
            .map(|((_, _, placeholder), value)| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(placeholder)
                        .default_value(value)
                })
            })
            .collect::<Vec<_>>()
            .into_iter();
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("SSH password", "输入 SSH 密码"))
                .default_value(raw_password)
                .masked(true)
        });
        let mut jump_host_id = profile.and_then(|host| {
            host.jump_host_id
                .filter(|jid| Some(*jid) != id && existing_hosts.iter().any(|h| h.id == *jid))
        });
        if jump_host_id.is_none()
            && let Some(host) = profile
            && !host.jump_host.is_empty()
        {
            jump_host_id = existing_hosts
                .iter()
                .find(|h| {
                    Some(h.id) != id
                        && h.protocol == HostProtocol::Ssh
                        && (h.ssh_jump_target() == host.jump_host || h.name == host.jump_host)
                })
                .map(|h| h.id);
        }
        let legacy_jump_host = if jump_host_id.is_none() {
            profile.map(|h| h.jump_host.clone()).unwrap_or_default()
        } else {
            String::new()
        };
        let raw_relay_cmd = profile
            .map(|host| host.jump_relay_command.clone())
            .unwrap_or_default();
        let jump_relay_command = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text(
                    "Optional relay command (e.g. su admin)",
                    "可选中继命令（如 su admin）",
                ))
                .default_value(raw_relay_cmd)
        });
        let raw_relay_password = profile
            .and_then(|host| decrypt_secret(&host.jump_relay_password))
            .unwrap_or_default();
        let jump_relay_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text(
                    "Optional relay password (prompted if empty)",
                    "可选中继密码（留空则连接时询问）",
                ))
                .default_value(raw_relay_password)
                .masked(true)
        });

        let protocol_items = build_protocol_items();
        let protocol_ix = protocol_items
            .iter()
            .position(|item| item.value == protocol)
            .map(|row| IndexPath::default().row(row));
        let protocol_select =
            cx.new(|cx| SelectState::new(protocol_items, protocol_ix, window, cx));

        let key_items = build_key_select_items(&default_identity, &discovered_keys, language);
        let key_ix = key_items
            .iter()
            .position(|item| item.value == default_identity)
            .map(|row| IndexPath::default().row(row))
            .or_else(|| Some(IndexPath::default().row(0)));
        let key_select = cx.new(|cx| SelectState::new(key_items, key_ix, window, cx));

        let jump_items = build_jump_items(existing_hosts, id, &legacy_jump_host, language);
        let current_jump = match jump_host_id {
            Some(hid) => JumpChoice::Host(hid),
            None if !legacy_jump_host.is_empty() => JumpChoice::Legacy(legacy_jump_host.clone()),
            None => JumpChoice::None,
        };
        let jump_ix = jump_items
            .iter()
            .position(|item| item.value == current_jump)
            .map(|row| IndexPath::default().row(row))
            .or_else(|| Some(IndexPath::default().row(0)));
        let jump_select = cx.new(|cx| SelectState::new(jump_items, jump_ix, window, cx));

        Self {
            id,
            protocol,
            auth_method,
            icon,
            fields: std::array::from_fn(|_| {
                fields.next().expect("one initial value for each field")
            }),
            password,
            jump_host_id,
            legacy_jump_host,
            jump_relay_command,
            jump_relay_password,
            discovered_keys,
            protocol_select,
            key_select,
            jump_select,
            _select_subs: Vec::new(),
        }
    }

    fn profile(
        &self,
        id: u64,
        existing_hosts: &[HostProfile],
        language: Language,
        cx: &App,
    ) -> Result<HostProfile, &'static str> {
        let [name, address, username, port, raw_identity_file, group, tags] = self
            .fields
            .each_ref()
            .map(|field| field.read(cx).value().trim().to_owned());
        let jump_host = match self.jump_host_id {
            Some(jid) => existing_hosts
                .iter()
                .find(|h| h.id == jid)
                .map(HostProfile::ssh_jump_target)
                .unwrap_or_default(),
            None => self.legacy_jump_host.clone(),
        };
        let (identity_file, encrypted_password) = if self.protocol == HostProtocol::Ssh {
            match self.auth_method {
                SshAuthMethod::PrivateKey => (raw_identity_file, String::new()),
                SshAuthMethod::Password => {
                    let raw_password = self.password.read(cx).value().to_owned();
                    let encrypted = if !raw_password.is_empty() {
                        encrypt_secret(&raw_password).map_err(|_| {
                            language.text("Could not encrypt password.", "无法加密密码。")
                        })?
                    } else {
                        String::new()
                    };
                    (String::new(), encrypted)
                }
            }
        } else {
            (raw_identity_file, String::new())
        };
        let relay_command = self.jump_relay_command.read(cx).value().trim().to_owned();
        let raw_relay_password = self.jump_relay_password.read(cx).value().to_owned();
        let encrypted_relay_password = if self.protocol == HostProtocol::Ssh
            && !jump_host.is_empty()
            && !relay_command.is_empty()
            && !raw_relay_password.is_empty()
        {
            encrypt_secret(&raw_relay_password).map_err(|_| {
                language.text("Could not encrypt relay password.", "无法加密中继密码。")
            })?
        } else {
            String::new()
        };
        validate_profile_for(
            id,
            self.protocol,
            self.icon,
            [
                name,
                address,
                username,
                port,
                identity_file,
                jump_host,
                group,
                tags,
            ],
            encrypted_password,
            self.jump_host_id,
            relay_command,
            encrypted_relay_password,
            language,
        )
    }
}

pub enum HostsEvent {
    Connect(HostProfile),
}

impl EventEmitter<HostsEvent> for HostsPage {}

pub struct HostsPage {
    store: Entity<SettingsStore>,
    search: Entity<InputState>,
    selected_tag: Option<String>,
    editor: Option<HostEditor>,
    loading_import: bool,
    import: Option<ImportBatch>,
    _updates: [Subscription; 2],
}

impl HostsPage {
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = store.read(cx).current.language;
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Search hosts…", "搜索主机…"))
        });
        if let Some(err) = store.read(cx).load_error() {
            crate::notify::error(err.to_owned(), window, cx);
        }
        let updates = [
            cx.subscribe(&search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.observe(&store, |_, _, cx| cx.notify()),
        ];
        Self {
            store,
            search,
            selected_tag: None,
            editor: None,
            loading_import: false,
            import: None,
            _updates: updates,
        }
    }

    fn language(&self, cx: &App) -> Language {
        self.store.read(cx).current.language
    }

    fn saved_host(&self, id: u64, cx: &App) -> Option<HostProfile> {
        self.store
            .read(cx)
            .current
            .hosts
            .iter()
            .find(|host| host.id == id)
            .cloned()
    }

    pub(crate) fn edit_host(
        &mut self,
        id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile = id.and_then(|id| self.saved_host(id, cx));
        self.show_editor(
            profile.as_ref(),
            profile.as_ref().map(|host| host.id),
            window,
            cx,
        );
    }

    fn show_editor(
        &mut self,
        profile: Option<&HostProfile>,
        id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.language(cx);
        let existing_hosts = self.store.read(cx).current.hosts.clone();
        let mut editor = HostEditor::new(profile, id, &existing_hosts, language, window, cx);
        let proto_sub = cx.subscribe_in(
            &editor.protocol_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<HostProtocol>>>, window, cx| {
                if let SelectEvent::Confirm(Some(proto)) = event
                    && let Some(editor) = &mut this.editor
                {
                    editor.protocol = *proto;
                    let default_port = match proto {
                        HostProtocol::Ssh | HostProtocol::Mosh => "22",
                        HostProtocol::Telnet => "23",
                        HostProtocol::Serial => "115200",
                    };
                    editor.fields[3].update(cx, |input, cx| {
                        input.set_value(default_port, window, cx);
                    });
                    cx.notify();
                }
            },
        );
        let key_sub = cx.subscribe_in(
            &editor.key_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<String>>>, window, cx| {
                if let SelectEvent::Confirm(Some(path)) = event
                    && let Some(editor) = &mut this.editor
                {
                    editor.fields[4].update(cx, |input, cx| {
                        input.set_value(path.clone(), window, cx);
                    });
                    cx.notify();
                }
            },
        );
        let jump_sub = cx.subscribe_in(
            &editor.jump_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<JumpChoice>>>, _, cx| {
                if let SelectEvent::Confirm(Some(choice)) = event
                    && let Some(editor) = &mut this.editor
                {
                    match choice {
                        JumpChoice::None => {
                            editor.jump_host_id = None;
                            editor.legacy_jump_host.clear();
                        }
                        JumpChoice::Host(hid) => {
                            editor.jump_host_id = Some(*hid);
                            editor.legacy_jump_host.clear();
                        }
                        JumpChoice::Legacy(raw) => {
                            editor.jump_host_id = None;
                            editor.legacy_jump_host = raw.clone();
                        }
                    }
                    cx.notify();
                }
            },
        );
        editor._select_subs = vec![proto_sub, key_sub, jump_sub];
        editor.fields[0].update(cx, |field, cx| field.focus(window, cx));
        self.editor = Some(editor);
        self.import = None;
        cx.notify();
    }

    fn duplicate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut profile) = self
            .editor
            .as_ref()
            .and_then(|editor| editor.id)
            .and_then(|id| self.saved_host(id, cx))
        else {
            return;
        };
        profile
            .name
            .push_str(self.language(cx).text(" copy", " 副本"));
        self.show_editor(Some(&profile), None, window, cx);
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.editor = None;
        cx.notify();
    }

    fn save(
        &mut self,
        change: impl FnOnce(&mut Settings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = self.store.update(cx, |store, cx| {
            let result = store.update(change);
            if result.is_ok() {
                cx.notify();
            }
            result
        });
        if let Err(err) = result {
            crate::notify::error(err, window, cx);
            false
        } else {
            true
        }
    }

    fn save_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else { return };
        let language = self.language(cx);
        if editor
            .id
            .is_some_and(|id| self.saved_host(id, cx).is_none())
        {
            crate::notify::error(
                language.text("This host has been removed.", "这台主机已被删除。"),
                window,
                cx,
            );
            return;
        }
        let allocation = self.store.read(cx).current.host_ids(1);
        let Some(id) = editor
            .id
            .or_else(|| allocation.as_ref().map(|ids| ids.start))
        else {
            crate::notify::error(
                language.text("Could not allocate a host ID.", "无法分配主机 ID。"),
                window,
                cx,
            );
            return;
        };
        let existing_hosts = self.store.read(cx).current.hosts.clone();
        let mut profile = match editor.profile(id, &existing_hosts, language, cx) {
            Ok(profile) => profile,
            Err(error) => {
                crate::notify::error(error, window, cx);
                return;
            }
        };
        if self.save(
            |settings| {
                if let Some(existing) = settings.hosts.iter_mut().find(|host| host.id == id) {
                    profile.last_connected = existing.last_connected;
                    *existing = profile;
                } else {
                    settings.hosts.push(profile);
                    settings.next_host_id = id + 1;
                }
            },
            window,
            cx,
        ) {
            if let Some(editor) = &mut self.editor {
                editor.id = Some(id);
            }
            self.search
                .update(cx, |search, cx| search.set_value("", window, cx));
            crate::notify::success(language.text("Host saved.", "主机已保存。"), window, cx);
        }
    }

    fn remove_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editor.as_ref().and_then(|editor| editor.id) else {
            return;
        };
        let language = self.language(cx);
        if self.save(
            |settings| settings.hosts.retain(|host| host.id != id),
            window,
            cx,
        ) {
            self.cancel_edit(cx);
            crate::notify::success(
                language.text("Host removed.", "主机已删除。"),
                window,
                cx,
            );
        }
    }

    fn connect_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(host) = self
            .editor
            .as_ref()
            .and_then(|editor| editor.id)
            .and_then(|id| self.saved_host(id, cx))
        {
            cx.emit(HostsEvent::Connect(host));
        }
    }

    fn copy_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self
            .editor
            .as_ref()
            .and_then(|editor| editor.id)
            .and_then(|id| self.saved_host(id, cx))
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(copy_command(&host)));
        let language = self.language(cx);
        crate::notify::success(
            language.text("SSH command copied to clipboard.", "已复制 SSH 命令到剪贴板。"),
            window,
            cx,
        );
    }

    fn choose_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading_import {
            return;
        }
        let language = self.language(cx);
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(
                language
                    .text("Choose an OpenSSH config", "选择 OpenSSH 配置文件")
                    .into(),
            ),
        });
        self.loading_import = true;
        cx.notify();
        let executor = cx.background_executor().clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |view, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    executor
                        .spawn(async move { read_configs(paths, language).map(Some) })
                        .await
                }
                Ok(Ok(None)) | Err(_) => Ok(None),
                Ok(Err(error)) => Err(error.to_string()),
            };
            let _ = view.update(cx, |view, cx| {
                view.loading_import = false;
                match result {
                    Ok(Some(import)) => {
                        view.import = Some(import);
                        cx.notify();
                    }
                    Ok(None) => cx.notify(),
                    Err(error) => {
                        let msg = format!(
                            "{} {error}",
                            language.text("Could not import the config.", "无法导入配置。")
                        );
                        let _ = window_handle.update(cx, |_, window, cx| {
                            crate::notify::error(msg, window, cx);
                        });
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn import_hosts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(batch) = &self.import else { return };
        let current = &self.store.read(cx).current;
        let (mut candidates, skipped) = merge_candidates(&current.hosts, batch);
        let count = candidates.len();
        if count == 0 {
            self.import = None;
            cx.notify();
            return;
        }
        let Some(ids) = current.host_ids(count) else {
            let language = self.language(cx);
            crate::notify::error(
                language.text("Could not allocate host IDs.", "无法分配主机 ID。"),
                window,
                cx,
            );
            return;
        };
        let start = ids.start;
        let next = ids.end;
        for (host, id) in candidates.iter_mut().zip(ids) {
            host.id = id;
        }
        if self.save(
            |settings| {
                settings.hosts.extend(candidates);
                settings.next_host_id = next;
            },
            window,
            cx,
        ) {
            self.edit_host(Some(start), window, cx);
            self.search
                .update(cx, |search, cx| search.set_value("", window, cx));
            crate::notify::success(
                import_summary(count, skipped, self.language(cx), true),
                window,
                cx,
            );
        }
    }

    fn choose_key(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else { return };
        let field = editor.fields[4].clone();
        let language = self.language(cx);
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                language
                    .text("Choose an SSH identity file", "选择 SSH 私钥文件")
                    .into(),
            ),
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = receiver.await;
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |view, cx| {
                    if !view
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.fields[4] == field)
                    {
                        return;
                    }
                    match result {
                        Ok(Ok(Some(paths))) => {
                            if let Some(path) = paths.first() {
                                let value = path.to_string_lossy().into_owned();
                                field.update(cx, |input, cx| {
                                    input.set_value(value.clone(), window, cx)
                                });
                                if let Some(editor) = &view.editor {
                                    let items = build_key_select_items(
                                        &value,
                                        &editor.discovered_keys,
                                        language,
                                    );
                                    editor.key_select.update(cx, |select, cx| {
                                        select.set_items(items, window, cx);
                                        select.set_selected_value(&value, window, cx);
                                    });
                                }
                            }
                        }
                        Ok(Err(error)) => crate::notify::error(
                            format!(
                                "{} {error}",
                                language.text(
                                    "Could not select the identity file.",
                                    "无法选择私钥文件。",
                                )
                            ),
                            window,
                            cx,
                        ),
                        _ => {}
                    }
                });
            });
        })
        .detach();
    }

    fn toggle_group(&mut self, group: String, window: &mut Window, cx: &mut Context<Self>) {
        self.save(
            |settings| {
                if let Some(pos) = settings
                    .collapsed_groups
                    .iter()
                    .position(|existing| existing == &group)
                {
                    settings.collapsed_groups.remove(pos);
                } else {
                    settings.collapsed_groups.push(group);
                }
            },
            window,
            cx,
        );
    }

    fn host_list(
        &self,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let selected = self.editor.as_ref().and_then(|editor| editor.id);
        let query = self.search.read(cx).value().trim().to_lowercase();
        let settings = &self.store.read(cx).current;
        let all_hosts = &settings.hosts;
        let collapsed = &settings.collapsed_groups;
        let total = all_hosts.len();

        let mut all_tags: Vec<String> = all_hosts
            .iter()
            .flat_map(|host| host.tags.iter().cloned())
            .collect();
        all_tags.sort_by_key(|t| t.to_lowercase());
        all_tags.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        let filtered: Vec<_> = all_hosts
            .iter()
            .filter(|host| {
                host_matches(host, &query)
                    && self.selected_tag.as_ref().is_none_or(|tag| {
                        host.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
                    })
            })
            .collect();
        let grouped = group_hosts(filtered.iter().copied());

        let mut entries = div()
            .id("host-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.));

        if filtered.is_empty() {
            entries = entries.child(
                div()
                    .px(px(10.))
                    .py(px(18.))
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(rgb(colors.muted))
                    .child(if query.is_empty() && self.selected_tag.is_none() {
                        language.text("No saved hosts yet", "暂无已保存的主机")
                    } else {
                        language.text("No matching hosts", "没有匹配的主机")
                    }),
            );
        }

        for (group_index, (group_name, items)) in grouped.into_iter().enumerate() {
            let is_collapsed = !group_name.is_empty()
                && collapsed.iter().any(|g| g == &group_name)
                && query.is_empty()
                && self.selected_tag.is_none();

            if !group_name.is_empty() {
                let count = items.len();
                let toggle_name = group_name.clone();
                let chevron = if is_collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                };
                entries = entries.child(
                    Button::new(("host-group", group_index))
                        .ghost()
                        .w_full()
                        .h(px(26.))
                        .px(px(6.))
                        .mt(px(4.))
                        .rounded_md()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .min_w_0()
                                .child(chevron.render(px(11.), colors.muted))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(colors.secondary))
                                        .truncate()
                                        .child(group_name),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(rgb(colors.muted))
                                .child(count.to_string()),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_group(toggle_name.clone(), window, cx);
                        })),
                );
            }

            if is_collapsed {
                continue;
            }

            entries = entries.children(items.into_iter().map(|host| {
                let id = host.id;
                let is_selected = selected == Some(id);
                let tags_line = if host.tags.is_empty() {
                    None
                } else {
                    Some(
                        host.tags
                            .iter()
                            .take(3)
                            .map(|t| format!("#{t}"))
                            .collect::<Vec<_>>()
                            .join(" "),
                    )
                };
                Button::new(("host", id))
                    .ghost()
                    .relative()
                    .w_full()
                    .h(if tags_line.is_some() { px(62.) } else { px(52.) })
                    .px(px(10.))
                    .rounded_lg()
                    .border_1()
                    .border_color(if is_selected {
                        colors.accent_border()
                    } else {
                        transparent_black().into()
                    })
                    .bg(if is_selected {
                        colors.accent_subtle()
                    } else {
                        transparent_black().into()
                    })
                    .justify_start()
                    .gap(px(10.))
                    .children(is_selected.then(|| {
                        div()
                            .absolute()
                            .left_0()
                            .top(px(10.))
                            .bottom(px(10.))
                            .w(px(2.5))
                            .rounded_r_full()
                            .bg(rgb(colors.accent))
                    }))
                    .child(
                        div()
                            .size(px(30.))
                            .rounded_lg()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if is_selected {
                                colors.accent_soft()
                            } else {
                                rgb(colors.surface)
                            })
                            .child(host.icon.icon().render(
                                px(14.),
                                if is_selected {
                                    colors.accent
                                } else {
                                    colors.muted
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(1.))
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(host.name.clone()),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.muted))
                                    .child(host.endpoint()),
                            )
                            .children(tags_line.map(|line| {
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(px(10.5))
                                    .text_color(rgb(colors.accent))
                                    .child(line)
                            })),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.edit_host(Some(id), window, cx)),
                    )
            }));
        }

        let mut header = div()
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("SSH Hosts", "SSH 主机")),
                    )
                    .child(
                        div()
                            .px(px(7.))
                            .py(px(2.))
                            .rounded_full()
                            .bg(rgb(colors.surface))
                            .text_size(px(11.))
                            .text_color(rgb(colors.muted))
                            .child(total.to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        primary_button(
                            "add-host",
                            Some(Icon::Plus),
                            language.text("New", "新建"),
                            colors,
                        )
                        .flex_1()
                        .disabled(self.loading_import)
                        .on_click(cx.listener(
                            |this, _, window, cx| this.edit_host(None, window, cx),
                        )),
                    )
                    .child(
                        secondary_button(
                            "import-hosts",
                            Some(Icon::Import),
                            language.text("Import", "导入"),
                            colors,
                        )
                        .flex_1()
                        .disabled(self.loading_import)
                        .on_click(cx.listener(|this, _, window, cx| this.choose_config(window, cx))),
                    ),
            )
            .child(input_field(&self.search, Some(Icon::Search), colors));

        if !all_tags.is_empty() {
            header = header.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .children(all_tags.into_iter().enumerate().map(|(ix, tag)| {
                        let active = self
                            .selected_tag
                            .as_ref()
                            .is_some_and(|t| t.eq_ignore_ascii_case(&tag));
                        let tag_click = tag.clone();
                        Button::new(("tag-filter", ix))
                            .xsmall()
                            .h(px(22.))
                            .px(px(8.))
                            .rounded_full()
                            .bg(if active {
                                colors.accent_soft()
                            } else {
                                rgb(colors.surface).into()
                            })
                            .text_color(rgb(if active {
                                colors.accent
                            } else {
                                colors.secondary
                            }))
                            .cursor_pointer()
                            .child(format!("#{tag}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if active {
                                    this.selected_tag = None;
                                } else {
                                    this.selected_tag = Some(tag_click.clone());
                                }
                                cx.notify();
                            }))
                    })),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(260.))
            .bg(rgb(colors.panel))
            .border_r_1()
            .border_color(rgb(colors.line))
            .child(header)
            .child(entries)
    }

    fn empty_state(
        &self,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p(px(32.))
            .gap(px(12.))
            .child(
                div()
                    .size(px(52.))
                    .rounded_2xl()
                    .bg(colors.accent_soft())
                    .border_1()
                    .border_color(colors.accent_border())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::Server.render(px(24.), colors.accent)),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.foreground))
                    .child(language.text("Manage SSH Hosts", "管理 SSH 主机")),
            )
            .child(
                div()
                    .max_w(px(340.))
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(rgb(colors.muted))
                    .child(language.text(
                        "Select a host from the list to inspect or edit, create a new profile, or import from your OpenSSH config.",
                        "从左侧选择一台主机进行编辑，或新建主机配置，也可以直接从 OpenSSH 配置文件导入。",
                    )),
            )
            .child(
                div()
                    .pt(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        primary_button(
                            "empty-add-host",
                            Some(Icon::Plus),
                            language.text("New Host", "新建主机"),
                            colors,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.edit_host(None, window, cx)),
                        ),
                    )
                    .child(
                        secondary_button(
                            "empty-import-hosts",
                            Some(Icon::Import),
                            language.text("Import OpenSSH…", "导入 OpenSSH…"),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.choose_config(window, cx))),
                    ),
            )
    }

    fn host_form(
        &self,
        editor: &HostEditor,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let field = |index: usize| {
            let (english, chinese, _) = FIELDS[index];
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .flex_1()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors.secondary))
                        .child(language.text(english, chinese)),
                )
                .child(if index == 4 {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Select::new(&editor.key_select)),
                        )
                        .child(
                            secondary_button(
                                "pick-identity",
                                Some(Icon::Folder),
                                language.text("Browse…", "浏览…"),
                                colors,
                            )
                            .flex_shrink_0()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_key(window, cx)),
                            ),
                        )
                } else {
                    div().child(input_field(&editor.fields[index], None, colors))
                })
        };

        let saved = editor.id.and_then(|id| self.saved_host(id, cx));
        let title = if saved.is_some() {
            language.text("Edit Host", "编辑主机")
        } else {
            language.text("New Host", "新建主机")
        };

        let mut actions = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(8.));

        let mut left = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.))
            .child(
                primary_button(
                    "save-host",
                    Some(Icon::Check),
                    language.text("Save Host", "保存主机"),
                    colors,
                )
                .on_click(cx.listener(|this, _, window, cx| this.save_host(window, cx))),
            )
            .child(
                secondary_button(
                    "cancel-host",
                    None,
                    language.text("Cancel", "取消"),
                    colors,
                )
                .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
            );

        if saved.is_some() {
            left = left.child(
                Button::new("connect-host")
                    .ghost()
                    .small()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded_lg()
                    .bg(colors.success_soft())
                    .border_1()
                    .border_color(colors.success_border())
                    .text_color(rgb(colors.success))
                    .icon(Icon::Play)
                    .label(language.text("Connect", "连接"))
                    .on_click(cx.listener(|this, _, _, cx| this.connect_selected(cx))),
            );
            left = left
                .child(
                    secondary_button(
                        "copy-host-command",
                        Some(Icon::Copy),
                        language.text("Copy SSH", "复制 SSH"),
                        colors,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.copy_selected(window, cx))),
                )
                .child(
                    secondary_button(
                        "duplicate-host",
                        Some(Icon::Duplicate),
                        language.text("Duplicate", "副本"),
                        colors,
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.duplicate_selected(window, cx)),
                    ),
                );
        }

        actions = actions.child(left);
        if saved.is_some() {
            let page = cx.entity().downgrade();
            actions = actions.child(
                danger_button(
                    "remove-host",
                    Some(Icon::Trash),
                    language.text("Remove", "删除"),
                    false,
                    colors,
                )
                .on_click(move |_, window, cx| {
                    let page = page.clone();
                    crate::notify::confirm(
                        language.text("Remove Host", "删除主机"),
                        language.text(
                            "Remove this host permanently?",
                            "确定要删除这台主机吗？",
                        ),
                        language.text("Remove", "确认删除"),
                        move |window, cx| {
                            let _ = page.update(cx, |this, cx| this.remove_host(window, cx));
                        },
                        window,
                        cx,
                    );
                }),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("host-fields")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(24.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .max_w(px(580.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(18.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(title),
                                    )
                                    .children(saved.map(|host| {
                                        div()
                                            .px(px(10.))
                                            .py(px(4.))
                                            .rounded_full()
                                            .bg(rgb(colors.surface))
                                            .border_1()
                                            .border_color(rgb(colors.line))
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(host.endpoint())
                                    })),
                            )
                            .child(
                                div()
                                    .p(px(18.))
                                    .rounded_xl()
                                    .bg(rgb(colors.panel))
                                    .border_1()
                                    .border_color(rgb(colors.line))
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(5.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.secondary))
                                                    .child(language.text("Protocol", "连接协议")),
                                            )
                                            .child(Select::new(&editor.protocol_select)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(5.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.secondary))
                                                    .child(language.text("Icon", "图标")),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_wrap()
                                                    .gap(px(6.))
                                                    .children(HostIcon::ALL.into_iter().enumerate().map(
                                                        |(ix, host_icon)| {
                                                            let active = editor.icon == host_icon;
                                                            Button::new(("host-icon", ix))
                                                                .size(px(34.))
                                                                .rounded_lg()
                                                                .border_1()
                                                                .border_color(if active {
                                                                    colors.accent_border()
                                                                } else {
                                                                    rgb(colors.line).into()
                                                                })
                                                                .bg(if active {
                                                                    colors.accent_soft()
                                                                } else {
                                                                    rgb(colors.surface).into()
                                                                })
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .cursor_pointer()
                                                                .child(host_icon.icon().render(
                                                                    px(16.),
                                                                    if active {
                                                                        colors.accent
                                                                    } else {
                                                                        colors.secondary
                                                                    },
                                                                ))
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        if let Some(editor) =
                                                                            &mut this.editor
                                                                        {
                                                                            editor.icon = host_icon;
                                                                        }
                                                                        cx.notify();
                                                                    },
                                                                ))
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(12.))
                                            .child(field(0))
                                            .child(field(1)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(12.))
                                            .child(field(2))
                                            .child(div().w(px(110.)).child(field(3))),
                                    )
                                    .children((editor.protocol == HostProtocol::Ssh).then(|| {
                                        let auth_toggle = div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .p(px(3.))
                                            .rounded_lg()
                                            .bg(rgb(colors.background))
                                            .border_1()
                                            .border_color(rgb(colors.line))
                                            .children(
                                                [
                                                    (
                                                        SshAuthMethod::PrivateKey,
                                                        Icon::Lock,
                                                        language.text("Private Key", "私钥认证"),
                                                    ),
                                                    (
                                                        SshAuthMethod::Password,
                                                        Icon::Edit,
                                                        language.text("Password", "密码认证"),
                                                    ),
                                                ]
                                                .into_iter()
                                                .enumerate()
                                                .map(|(ix, (method, icon, label))| {
                                                    let active = editor.auth_method == method;
                                                    Button::new(("ssh-auth-method", ix))
                                                        .flex_1()
                                                        .h(px(28.))
                                                        .with_variant(if active {
                                                            gpui_kit::component::button::ButtonVariant::Primary
                                                        } else {
                                                            gpui_kit::component::button::ButtonVariant::Ghost
                                                        })
                                                        .small()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.))
                                                        .child(icon.render(
                                                            px(13.),
                                                            if active {
                                                                colors.accent_fg
                                                            } else {
                                                                colors.muted
                                                            },
                                                        ))
                                                        .child(label)
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if let Some(editor) =
                                                                    &mut this.editor
                                                                {
                                                                    editor.auth_method = method;
                                                                }
                                                                cx.notify();
                                                            },
                                                        ))
                                                }),
                                            );
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(10.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(5.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(colors.secondary))
                                                            .child(language.text(
                                                                "Authentication",
                                                                "认证方式",
                                                            )),
                                                    )
                                                    .child(auth_toggle),
                                            )
                                            .child(match editor.auth_method {
                                                SshAuthMethod::PrivateKey => {
                                                    field(4).into_any_element()
                                                }
                                                SshAuthMethod::Password => div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(5.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(colors.secondary))
                                                            .child(language.text(
                                                                "Password (encrypted at rest)",
                                                                "密码（本地加密存储）",
                                                            )),
                                                    )
                                                    .child(input_field(
                                                        &editor.password,
                                                        Some(Icon::Lock),
                                                        colors,
                                                    ))
                                                    .into_any_element(),
                                            })
                                    }))
                                    .children(
                                        (editor.protocol == HostProtocol::Mosh)
                                            .then(|| field(4)),
                                    )
                                    .children(
                                        matches!(
                                            editor.protocol,
                                            HostProtocol::Ssh | HostProtocol::Mosh
                                        )
                                        .then(|| {
                                            let has_jump = editor.jump_host_id.is_some()
                                                || !editor.legacy_jump_host.is_empty();
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(10.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(5.))
                                                        .child(
                                                            div()
                                                                .text_size(px(12.))
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .text_color(rgb(colors.secondary))
                                                                .child(language.text(
                                                                    "Jump host",
                                                                    "跳板机",
                                                                )),
                                                        )
                                                        .child(Select::new(&editor.jump_select)),
                                                )
                                                .children(
                                                    (editor.protocol == HostProtocol::Ssh
                                                        && has_jump)
                                                        .then(|| {
                                                            div()
                                                                .flex()
                                                                .flex_col()
                                                                .gap(px(10.))
                                                                .child(
                                                                    div()
                                                                        .flex()
                                                                        .flex_col()
                                                                        .gap(px(5.))
                                                                        .child(
                                                                            div()
                                                                                .text_size(px(12.))
                                                                                .font_weight(
                                                                                    FontWeight::MEDIUM,
                                                                                )
                                                                                .text_color(rgb(
                                                                                    colors.secondary,
                                                                                ))
                                                                                .child(
                                                                                    language.text(
                                                                                        "Jump relay command",
                                                                                        "跳板机中继命令",
                                                                                    ),
                                                                                ),
                                                                        )
                                                                        .child(input_field(
                                                                            &editor
                                                                                .jump_relay_command,
                                                                            Some(Icon::Terminal),
                                                                            colors,
                                                                        )),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex()
                                                                        .flex_col()
                                                                        .gap(px(5.))
                                                                        .child(
                                                                            div()
                                                                                .text_size(px(12.))
                                                                                .font_weight(
                                                                                    FontWeight::MEDIUM,
                                                                                )
                                                                                .text_color(rgb(
                                                                                    colors.secondary,
                                                                                ))
                                                                                .child(
                                                                                    language.text(
                                                                                        "Relay password (encrypted at rest)",
                                                                                        "中继密码（本地加密存储）",
                                                                                    ),
                                                                                ),
                                                                        )
                                                                        .child(input_field(
                                                                            &editor
                                                                                .jump_relay_password,
                                                                            Some(Icon::Lock),
                                                                            colors,
                                                                        )),
                                                                )
                                                        }),
                                                )
                                        }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(12.))
                                            .child(field(5))
                                            .child(field(6)),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(12.))
                    .bg(rgb(colors.panel))
                    .border_t_1()
                    .border_color(rgb(colors.line))
                    .child(actions),
            )
    }

    fn import_preview(
        &self,
        batch: &ImportBatch,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let (profiles, skipped) = merge_candidates(&self.store.read(cx).current.hosts, batch);
        let count = profiles.len();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(22.))
                    .pb(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("Import OpenSSH Hosts", "导入 OpenSSH 主机")),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(colors.muted))
                            .child(import_summary(count, skipped, language, false)),
                    ),
            )
            .child(
                div()
                    .id("import-preview")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(profiles.into_iter().map(|host| {
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded_lg()
                            .bg(rgb(colors.panel))
                            .border_1()
                            .border_color(rgb(colors.line))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(28.))
                                    .rounded_md()
                                    .bg(colors.accent_soft())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(host.icon.icon().render(px(13.), colors.accent)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .truncate()
                                            .child(host.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .truncate()
                                            .text_color(rgb(colors.muted))
                                            .child(host.endpoint()),
                                    ),
                            )
                    })),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(12.))
                    .bg(rgb(colors.panel))
                    .flex()
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(rgb(colors.line))
                    .child(
                        primary_button(
                            "commit-import",
                            Some(Icon::Import),
                            language.text("Import Hosts", "确认导入"),
                            colors,
                        )
                        .disabled(count == 0)
                        .on_click(cx.listener(|this, _, window, cx| this.import_hosts(window, cx))),
                    )
                    .child(
                        secondary_button(
                            "cancel-import",
                            None,
                            language.text("Cancel", "取消"),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.import = None;
                            cx.notify();
                        })),
                    ),
            )
    }
}

impl Render for HostsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
        let language = saved.language;
        let mut detail = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        if self.loading_import {
            detail = detail.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(colors.muted))
                    .child(language.text("Reading OpenSSH config…", "正在读取 OpenSSH 配置…")),
            );
        } else if let Some(import) = &self.import {
            detail = detail.child(self.import_preview(import, colors, language, cx));
        } else if let Some(editor) = &self.editor {
            detail = detail.child(self.host_form(editor, colors, language, cx));
        } else {
            detail = detail.child(self.empty_state(colors, language, cx));
        }
        div()
            .flex()
            .size_full()
            .min_h_0()
            .bg(rgb(colors.background))
            .text_color(rgb(colors.foreground))
            .child(self.host_list(colors, language, cx))
            .child(detail)
    }
}

fn import_summary(count: usize, skipped: usize, language: Language, saved: bool) -> String {
    match (language, saved) {
        (Language::English, true) => format!("{count} imported · {skipped} skipped"),
        (Language::English, false) => format!("{count} ready to import · {skipped} skipped"),
        (Language::ChineseSimplified, true) => format!("已导入 {count} 台 · 跳过 {skipped} 台"),
        (Language::ChineseSimplified, false) => format!("可导入 {count} 台 · 跳过 {skipped} 台"),
    }
}
