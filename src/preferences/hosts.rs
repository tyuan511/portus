mod model;

use gpui_kit::{
    App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, EventEmitter, FontWeight,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _,
    PathPromptOptions, Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, anchored, deferred,
    component::{
        IndexPath, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::InputState,
        select::{Select, SelectEvent, SelectState},
    },
    div, px, rgb, transparent_black,
};

use crate::icons::Icon;
use crate::settings::{
    HostIcon, HostProfile, HostProtocol, KeychainEntry, KeychainKind, Language, SettingsStore,
    group_hosts,
};
use crate::ssh::supports_sftp;
use crate::theme::Colors;
use crate::vault::{decrypt_secret, encrypt_secret};
use model::{copy_command, merge_candidates, read_configs, validate_profile_for};

use super::{SelectChoice, danger_button, input_field, primary_button, secondary_button};

pub enum HostsEvent {
    Connect(HostProfile),
    OpenSftp(HostProfile),
}

impl EventEmitter<HostsEvent> for HostsPage {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HostViewMode {
    Cards,
    List,
}

#[derive(Clone, PartialEq, Eq)]
enum KeychainChoice {
    None,
    Entry(u64),
}

#[derive(Clone, PartialEq, Eq)]
enum JumpChoice {
    None,
    Host(u64),
}

struct HostEditor {
    edit_id: Option<u64>,
    protocol: HostProtocol,
    icon: HostIcon,
    keychain_choice: KeychainChoice,
    jump_choice: JumpChoice,
    jump_relay_keychain_choice: KeychainChoice,
    name: Entity<InputState>,
    group: Entity<InputState>,
    tags: Entity<InputState>,
    address: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    identity_file: Entity<InputState>,
    password: Entity<InputState>,
    jump_relay_command: Entity<InputState>,
    jump_relay_password: Entity<InputState>,
    keychain_select: Entity<SelectState<Vec<SelectChoice<KeychainChoice>>>>,
    jump_select: Entity<SelectState<Vec<SelectChoice<JumpChoice>>>>,
    jump_relay_keychain_select: Entity<SelectState<Vec<SelectChoice<KeychainChoice>>>>,
    _subs: Vec<Subscription>,
}

impl HostEditor {
    fn new(
        host: Option<&HostProfile>,
        all_hosts: &[HostProfile],
        keychain: &[KeychainEntry],
        language: Language,
        window: &mut Window,
        cx: &mut Context<HostsPage>,
    ) -> Self {
        let edit_id = host.map(|h| h.id);
        let protocol = host.map(|h| h.protocol).unwrap_or(HostProtocol::Ssh);
        let icon = host.map(|h| h.icon).unwrap_or(HostIcon::Server);
        let name_v = host.map(|h| h.name.clone()).unwrap_or_default();
        let group_v = host
            .map(|h| h.group.clone())
            .unwrap_or_else(|| "Production".into());
        let tags_v = host.map(|h| h.tags.join(", ")).unwrap_or_default();
        let addr_v = host.map(|h| h.address.clone()).unwrap_or_default();
        let port_v = match host {
            Some(h) if h.protocol == HostProtocol::Serial => h.baud_rate.to_string(),
            Some(h) => h.port.to_string(),
            None => "22".into(),
        };
        let user_v = host
            .map(|h| h.username.clone())
            .unwrap_or_else(|| "root".into());
        let id_file_v = host.map(|h| h.identity_file.clone()).unwrap_or_default();
        let pwd_v = host
            .and_then(|h| decrypt_secret(&h.password))
            .unwrap_or_default();
        let relay_cmd_v = host.map(|h| h.jump_relay_command.clone()).unwrap_or_default();
        let relay_pwd_v = host
            .and_then(|h| decrypt_secret(&h.jump_relay_password))
            .unwrap_or_default();

        let keychain_choice = host
            .and_then(|h| h.keychain_id)
            .filter(|id| keychain.iter().any(|k| k.id == *id))
            .map(KeychainChoice::Entry)
            .unwrap_or(KeychainChoice::None);

        let jump_choice = host
            .and_then(|h| h.jump_host_id)
            .filter(|id| all_hosts.iter().any(|candidate| candidate.id == *id))
            .map(JumpChoice::Host)
            .unwrap_or(JumpChoice::None);

        let jump_relay_keychain_choice = host
            .and_then(|h| h.jump_relay_keychain_id)
            .filter(|id| {
                keychain
                    .iter()
                    .any(|k| k.id == *id && k.kind == KeychainKind::Password)
            })
            .map(KeychainChoice::Entry)
            .unwrap_or(KeychainChoice::None);

        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("e.g. prod-api-01", "例如 prod-api-01"))
                .default_value(name_v)
        });
        let group = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Production", "Production"))
                .default_value(group_v)
        });
        let tags = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("api, hkg, docker", "api, hkg, docker"))
                .default_value(tags_v)
        });
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(match protocol {
                    HostProtocol::Serial => "/dev/cu.usbserial-110",
                    _ => "10.0.14.21",
                })
                .default_value(addr_v)
        });
        let port = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(match protocol {
                    HostProtocol::Serial => "115200",
                    HostProtocol::Telnet => "23",
                    _ => "22",
                })
                .default_value(port_v)
        });
        let username = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("root")
                .default_value(user_v)
        });
        let identity_file = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("~/.ssh/id_ed25519")
                .default_value(id_file_v)
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(language.text("Optional direct password", "可选直填密码"))
                .default_value(pwd_v)
        });
        let jump_relay_command = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text(
                    "Optional, e.g. su - admin (separate steps with ;)",
                    "可选，如 su - admin（多条命令用 ; 分隔）",
                ))
                .default_value(relay_cmd_v)
        });
        let jump_relay_password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(language.text(
                    "Optional relay password (encrypted locally)",
                    "可选中继命令密码（本地加密保存）",
                ))
                .default_value(relay_pwd_v)
        });

        let mut kc_items = vec![SelectChoice::new(
            language.text("Direct / Auto (~/.ssh)", "不使用密钥库（直填或自动扫描 ~/.ssh）"),
            KeychainChoice::None,
        )];
        for entry in keychain {
            let kind = match entry.kind {
                KeychainKind::SshKey => "SSH Key",
                KeychainKind::Password => "Password",
            };
            kc_items.push(SelectChoice::new(
                format!("{} ({kind})", entry.name),
                KeychainChoice::Entry(entry.id),
            ));
        }
        let kc_idx = kc_items
            .iter()
            .position(|c| c.value == keychain_choice)
            .unwrap_or(0);
        let keychain_select =
            cx.new(|cx| SelectState::new(kc_items, Some(IndexPath::new(kc_idx)), window, cx));

        let mut jump_items = vec![SelectChoice::new(
            language.text("Direct (No Jump Host)", "直连（无跳板机）"),
            JumpChoice::None,
        )];
        for h in all_hosts {
            if Some(h.id) != edit_id && h.protocol == HostProtocol::Ssh {
                jump_items.push(SelectChoice::new(
                    format!("{} ({})", h.name, h.endpoint()),
                    JumpChoice::Host(h.id),
                ));
            }
        }
        let jump_idx = jump_items
            .iter()
            .position(|c| c.value == jump_choice)
            .unwrap_or(0);
        let jump_select =
            cx.new(|cx| SelectState::new(jump_items, Some(IndexPath::new(jump_idx)), window, cx));

        let mut relay_kc_items = vec![SelectChoice::new(
            language.text("Direct / Ask at connect", "不使用密钥库（直填或连接时询问）"),
            KeychainChoice::None,
        )];
        for entry in keychain.iter().filter(|k| k.kind == KeychainKind::Password) {
            relay_kc_items.push(SelectChoice::new(
                entry.name.clone(),
                KeychainChoice::Entry(entry.id),
            ));
        }
        let relay_kc_idx = relay_kc_items
            .iter()
            .position(|c| c.value == jump_relay_keychain_choice)
            .unwrap_or(0);
        let jump_relay_keychain_select = cx.new(|cx| {
            SelectState::new(relay_kc_items, Some(IndexPath::new(relay_kc_idx)), window, cx)
        });

        let kc_sub = cx.subscribe(&keychain_select, |this, _, ev: &SelectEvent<Vec<SelectChoice<KeychainChoice>>>, cx| {
            if let SelectEvent::Confirm(Some(choice)) = ev
                && let Some(editor) = &mut this.editor
            {
                editor.keychain_choice = choice.clone();
                cx.notify();
            }
        });
        let jump_sub = cx.subscribe(&jump_select, |this, _, ev: &SelectEvent<Vec<SelectChoice<JumpChoice>>>, cx| {
            if let SelectEvent::Confirm(Some(choice)) = ev
                && let Some(editor) = &mut this.editor
            {
                editor.jump_choice = choice.clone();
                cx.notify();
            }
        });
        let relay_kc_sub = cx.subscribe(&jump_relay_keychain_select, |this, _, ev: &SelectEvent<Vec<SelectChoice<KeychainChoice>>>, cx| {
            if let SelectEvent::Confirm(Some(choice)) = ev
                && let Some(editor) = &mut this.editor
            {
                editor.jump_relay_keychain_choice = choice.clone();
                cx.notify();
            }
        });

        Self {
            edit_id,
            protocol,
            icon,
            keychain_choice,
            jump_choice,
            jump_relay_keychain_choice,
            name,
            group,
            tags,
            address,
            port,
            username,
            identity_file,
            password,
            jump_relay_command,
            jump_relay_password,
            keychain_select,
            jump_select,
            jump_relay_keychain_select,
            _subs: vec![kc_sub, jump_sub, relay_kc_sub],
        }
    }
}

pub struct HostsPage {
    store: Entity<SettingsStore>,
    view_mode: HostViewMode,
    proto_filter: Option<HostProtocol>,
    group_filter: Option<String>,
    search_query: String,
    selected_host_id: Option<u64>,
    context_menu: Option<(u64, Point<Pixels>)>,
    editor: Option<HostEditor>,
    pending_click_gen: u64,
    _updates: Subscription,
}

impl HostsPage {
    pub fn new(store: Entity<SettingsStore>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let updates = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            store,
            view_mode: HostViewMode::Cards,
            proto_filter: None,
            group_filter: None,
            search_query: String::new(),
            selected_host_id: None,
            context_menu: None,
            editor: None,
            pending_click_gen: 0,
            _updates: updates,
        }
    }

    fn handle_host_click(
        &mut self,
        host: HostProfile,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu(cx);
        self.pending_click_gen = self.pending_click_gen.wrapping_add(1);
        if click_count >= 2 {
            cx.emit(HostsEvent::Connect(host));
            return;
        }
        let click_gen = self.pending_click_gen;
        let id = host.id;
        cx.spawn_in(window, async move |page, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(220))
                .await;
            let _ = cx.update(|window, cx| {
                let _ = page.update(cx, |this, cx| {
                    if this.pending_click_gen == click_gen {
                        this.edit_host(id, window, cx);
                    }
                });
            });
        })
        .detach();
    }

    pub fn set_group_filter(&mut self, group: Option<String>, cx: &mut Context<Self>) {
        self.group_filter = group;
        cx.notify();
    }

    pub fn set_search_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.search_query = query;
        cx.notify();
    }

    pub fn open_new_host_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.store.read(cx).current.clone();
        self.pending_click_gen = self.pending_click_gen.wrapping_add(1);
        self.selected_host_id = None;
        self.context_menu = None;
        let editor = HostEditor::new(
            None,
            &saved.hosts,
            &saved.keychain,
            saved.language,
            window,
            cx,
        );
        editor.name.update(cx, |input, cx| input.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    pub fn edit_host(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.store.read(cx).current.clone();
        let Some(host) = saved.hosts.iter().find(|h| h.id == id).cloned() else {
            return;
        };
        self.pending_click_gen = self.pending_click_gen.wrapping_add(1);
        self.selected_host_id = Some(id);
        self.context_menu = None;
        let editor = HostEditor::new(
            Some(&host),
            &saved.hosts,
            &saved.keychain,
            saved.language,
            window,
            cx,
        );
        self.editor = Some(editor);
        cx.notify();
    }

    pub fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.pending_click_gen = self.pending_click_gen.wrapping_add(1);
        self.editor = None;
        self.selected_host_id = None;
        cx.notify();
    }

    fn set_editor_protocol(&mut self, protocol: HostProtocol, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.editor {
            editor.protocol = protocol;
            let default_port = match protocol {
                HostProtocol::Serial => "115200",
                HostProtocol::Telnet => "23",
                HostProtocol::Ssh | HostProtocol::Mosh => "22",
            };
            editor.port.update(cx, |input, cx| {
                input.set_value(default_port, window, cx);
            });
            cx.notify();
        }
    }

    fn browse_identity_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select SSH Identity File".into()),
        });
        cx.spawn_in(window, async move |page, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let display = path.to_string_lossy().into_owned();
            let _ = cx.update(|window, cx| {
                let _ = page.update(cx, |this, cx| {
                    if let Some(editor) = &this.editor {
                        editor.identity_file.update(cx, |input, cx| {
                            input.set_value(display, window, cx);
                        });
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn save_editor(&mut self, connect_after: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            return;
        };
        let saved = self.store.read(cx).current.clone();
        let language = saved.language;
        let edit_id = editor.edit_id;
        let protocol = editor.protocol;
        let icon = editor.icon;
        let keychain_id = match editor.keychain_choice {
            KeychainChoice::Entry(id) => Some(id),
            KeychainChoice::None => None,
        };
        let (jump_host_id, jump_host_str) = match editor.jump_choice {
            JumpChoice::Host(jid) => {
                let target = saved
                    .hosts
                    .iter()
                    .find(|h| h.id == jid)
                    .map(HostProfile::ssh_jump_target)
                    .unwrap_or_default();
                (Some(jid), target)
            }
            JumpChoice::None => (None, String::new()),
        };

        let values = [
            editor.name.read(cx).value().to_string(),
            editor.address.read(cx).value().to_string(),
            editor.username.read(cx).value().to_string(),
            editor.port.read(cx).value().to_string(),
            editor.identity_file.read(cx).value().to_string(),
            jump_host_str,
            editor.group.read(cx).value().to_string(),
            editor.tags.read(cx).value().to_string(),
        ];

        let raw_pw = editor.password.read(cx).value().to_string();
        let encrypted_pw = if !raw_pw.is_empty() {
            match encrypt_secret(&raw_pw) {
                Ok(enc) => enc,
                Err(err) => {
                    crate::notify::error(err, window, cx);
                    return;
                }
            }
        } else {
            String::new()
        };

        let relay_keychain_id = match editor.jump_relay_keychain_choice {
            KeychainChoice::Entry(id) if jump_host_id.is_some() => Some(id),
            _ => None,
        };
        let raw_relay_cmd = editor.jump_relay_command.read(cx).value().to_string();
        let raw_relay_pw = editor.jump_relay_password.read(cx).value().to_string();
        let encrypted_relay_pw = if relay_keychain_id.is_none() && !raw_relay_pw.is_empty() {
            match encrypt_secret(&raw_relay_pw) {
                Ok(enc) => enc,
                Err(err) => {
                    crate::notify::error(err, window, cx);
                    return;
                }
            }
        } else {
            String::new()
        };

        let temp_id = edit_id.unwrap_or(0);
        let mut profile = match validate_profile_for(
            temp_id,
            protocol,
            icon,
            values,
            encrypted_pw,
            jump_host_id,
            raw_relay_cmd,
            encrypted_relay_pw,
            language,
        ) {
            Ok(p) => p,
            Err(err) => {
                crate::notify::error(err, window, cx);
                return;
            }
        };
        profile.keychain_id = keychain_id;
        profile.jump_relay_keychain_id = if profile.jump_relay_command.is_empty() {
            None
        } else {
            relay_keychain_id
        };
        if let Some(id) = edit_id
            && let Some(prev) = saved.hosts.iter().find(|h| h.id == id)
        {
            profile.last_connected = prev.last_connected;
        }

        let mut final_profile = profile.clone();
        let result = self.store.update(cx, |store, cx| {
            let res = store.update(|settings| {
                if let Some(id) = edit_id {
                    if let Some(existing) = settings.hosts.iter_mut().find(|h| h.id == id) {
                        *existing = profile.clone();
                    }
                } else if let Some(ids) = settings.host_ids(1) {
                    profile.id = ids.start;
                    final_profile.id = ids.start;
                    settings.next_host_id = ids.end;
                    settings.hosts.push(profile);
                }
            });
            cx.notify();
            res
        });

        match result {
            Ok(()) => {
                self.editor = None;
                self.selected_host_id = None;
                crate::notify::success(
                    language.text("Host saved.", "主机已保存。"),
                    window,
                    cx,
                );
                if connect_after {
                    cx.emit(HostsEvent::Connect(final_profile));
                }
                cx.notify();
            }
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn open_context_menu(&mut self, host_id: u64, pos: Point<Pixels>, cx: &mut Context<Self>) {
        self.context_menu = Some((host_id, pos));
        self.selected_host_id = Some(host_id);
        cx.notify();
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            if self.editor.is_none() {
                self.selected_host_id = None;
            }
            cx.notify();
        }
    }

    fn duplicate_host(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let suffix = language.text(" (Copy)", " (副本)");
        let result = self.store.update(cx, |store, cx| {
            let res = store.update(|settings| {
                if let Some(orig) = settings.hosts.iter().find(|h| h.id == id).cloned()
                    && let Some(ids) = settings.host_ids(1)
                {
                    let mut dup = orig;
                    dup.id = ids.start;
                    dup.name = format!("{}{suffix}", dup.name);
                    dup.last_connected = None;
                    settings.next_host_id = ids.end;
                    settings.hosts.push(dup);
                }
            });
            cx.notify();
            res
        });
        match result {
            Ok(()) => crate::notify::success(
                language.text("Host duplicated.", "已复制主机配置。"),
                window,
                cx,
            ),
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn copy_host_command(&self, host: &HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let text = match host.protocol {
            HostProtocol::Ssh => copy_command(host),
            _ => host.endpoint(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        crate::notify::success(
            language.text("Copied connection command.", "已复制连接命令。"),
            window,
            cx,
        );
    }

    fn copy_host_address(&self, host: &HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        cx.write_to_clipboard(ClipboardItem::new_string(host.address.clone()));
        crate::notify::success(
            language.text("Copied host address.", "已复制主机地址。"),
            window,
            cx,
        );
    }

    fn render_context_menu(
        &self,
        host_id: u64,
        pos: Point<Pixels>,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let hosts = &self.store.read(cx).current.hosts;
        let Some(host) = hosts.iter().find(|h| h.id == host_id) else {
            return div().into_any_element();
        };
        let can_sftp = supports_sftp(host, hosts);
        let host_conn = host.clone();
        let host_sftp = host.clone();
        let host_cmd = host.clone();
        let host_addr = host.clone();

        let menu_item = |id: &'static str,
                         icon: Icon,
                         label: &'static str,
                         danger: bool| {
            let text_color = if danger {
                colors.danger
            } else {
                colors.foreground
            };
            let icon_color = if danger {
                colors.danger
            } else {
                colors.secondary
            };
            div()
                .id(id)
                .h(px(28.))
                .px(px(9.))
                .rounded(px(6.))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.5))
                .text_color(rgb(text_color))
                .hover(|s| {
                    s.bg(if danger {
                        colors.danger_soft()
                    } else {
                        colors.accent_soft()
                    })
                })
                .child(icon.render(px(13.), icon_color))
                .child(label)
        };

        let menu_card = div()
            .id("host-context-menu")
            .w(px(196.))
            .p(px(5.))
            .rounded(px(10.))
            .bg(colors.popover_bg())
            .border_1()
            .border_color(colors.hairline_strong())
            .shadow_xl()
            .flex()
            .flex_col()
            .gap(px(1.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                menu_item("ctx-connect", Icon::Play, language.text("Connect", "连接"), false)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_context_menu(cx);
                        cx.emit(HostsEvent::Connect(host_conn.clone()));
                    })),
            )
            .children(can_sftp.then(|| {
                menu_item("ctx-sftp", Icon::Folder, language.text("Open SFTP", "打开文件传输"), false)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_context_menu(cx);
                        cx.emit(HostsEvent::OpenSftp(host_sftp.clone()));
                    }))
            }))
            .child(div().my(px(4.)).h(px(1.)).bg(colors.hairline()))
            .child(
                menu_item("ctx-edit", Icon::Edit, language.text("Edit", "编辑"), false)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_context_menu(cx);
                        this.edit_host(host_id, window, cx);
                    })),
            )
            .child(
                menu_item("ctx-dup", Icon::Duplicate, language.text("Duplicate", "复制主机"), false)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_context_menu(cx);
                        this.duplicate_host(host_id, window, cx);
                    })),
            )
            .child(
                menu_item(
                    "ctx-copy-cmd",
                    Icon::Terminal,
                    language.text("Copy SSH Command", "复制连接命令"),
                    false,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_context_menu(cx);
                    this.copy_host_command(&host_cmd, window, cx);
                })),
            )
            .child(
                menu_item(
                    "ctx-copy-addr",
                    Icon::Globe,
                    language.text("Copy Address", "复制地址"),
                    false,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_context_menu(cx);
                    this.copy_host_address(&host_addr, window, cx);
                })),
            )
            .child(div().my(px(4.)).h(px(1.)).bg(colors.hairline()))
            .child(
                menu_item("ctx-delete", Icon::Trash, language.text("Delete", "删除"), true)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_context_menu(cx);
                        this.delete_host(host_id, window, cx);
                    })),
            );

        div()
            .id("host-context-menu-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_context_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.close_context_menu(cx)),
            )
            .child(deferred(
                anchored()
                    .position(pos)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu_card),
            ))
            .into_any_element()
    }

    fn delete_host(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let result = self.store.update(cx, |store, cx| {
            let res = store.update(|settings| {
                settings.hosts.retain(|h| h.id != id);
            });
            cx.notify();
            res
        });
        match result {
            Ok(()) => {
                if self.selected_host_id == Some(id) {
                    self.selected_host_id = None;
                    self.editor = None;
                }
                crate::notify::info(language.text("Host deleted.", "主机已删除。"), window, cx);
                cx.notify();
            }
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn import_ssh_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let Some(config_path) = dirs::home_dir().map(|h| h.join(".ssh").join("config")) else {
            crate::notify::error(
                language.text("Could not locate ~/.ssh/config.", "无法定位 ~/.ssh/config。"),
                window,
                cx,
            );
            return;
        };
        let batch = match read_configs(vec![config_path], language) {
            Ok(b) => b,
            Err(err) => {
                crate::notify::error(err, window, cx);
                return;
            }
        };
        if batch.profiles.is_empty() {
            crate::notify::info(
                language.text(
                    "No importable hosts found in ~/.ssh/config.",
                    "在 ~/.ssh/config 中未找到可导入的主机。",
                ),
                window,
                cx,
            );
            return;
        }
        let mut added = 0usize;
        let result = self.store.update(cx, |store, cx| {
            let res = store.update(|settings| {
                let (mut imported, _) = merge_candidates(&settings.hosts, &batch);
                if let Some(ids) = settings.host_ids(imported.len()) {
                    for (host, id) in imported.iter_mut().zip(ids.clone()) {
                        host.id = id;
                    }
                    settings.next_host_id = ids.end;
                }
                added = imported.len();
                settings.hosts.extend(imported);
            });
            cx.notify();
            res
        });
        match result {
            Ok(()) => {
                let msg = if added > 0 {
                    format!(
                        "{} {added} {}",
                        language.text("Imported", "已导入"),
                        language.text("hosts from ~/.ssh/config.", "台主机（来自 ~/.ssh/config）。")
                    )
                } else {
                    language
                        .text("All ~/.ssh/config hosts are already imported.", "~/.ssh/config 中的主机已全部导入。")
                        .to_owned()
                };
                crate::notify::success(msg, window, cx);
            }
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn filtered_hosts(&self, cx: &App) -> Vec<HostProfile> {
        let hosts = &self.store.read(cx).current.hosts;
        let q = self.search_query.trim().to_lowercase();
        hosts
            .iter()
            .filter(|h| {
                if let Some(group) = &self.group_filter
                    && h.group.trim() != group.as_str()
                {
                    return false;
                }
                if let Some(proto) = self.proto_filter
                    && h.protocol != proto
                {
                    return false;
                }
                if !q.is_empty() {
                    let hay = format!(
                        "{} {} {} {}",
                        h.name,
                        h.endpoint(),
                        h.group,
                        h.tags.join(" ")
                    )
                    .to_lowercase();
                    if !q.split_whitespace().all(|term| hay.contains(term)) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect()
    }

    fn render_editor_fields(
        &self,
        editor: &HostEditor,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let proto = editor.protocol;
        let is_serial = proto == HostProtocol::Serial;
        let is_telnet = proto == HostProtocol::Telnet;
        let using_keychain = !matches!(editor.keychain_choice, KeychainChoice::None);

        let proto_seg = div()
            .flex()
            .gap(px(3.))
            .p(px(2.))
            .rounded(px(7.))
            .bg(colors.surface_glass())
            .border_1()
            .border_color(colors.hairline())
            .children(
                [
                    (HostProtocol::Ssh, "SSH"),
                    (HostProtocol::Mosh, "Mosh"),
                    (HostProtocol::Telnet, "Telnet"),
                    (HostProtocol::Serial, "Serial"),
                ]
                .into_iter()
                .enumerate()
                .map(|(idx, (p, label))| {
                    let active = proto == p;
                    div()
                        .id(("proto-seg", idx))
                        .flex_1()
                        .h(px(24.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.5))
                        .font_weight(if active {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .bg(if active {
                            colors.surface_active_glass()
                        } else {
                            transparent_black().into()
                        })
                        .text_color(rgb(if active {
                            colors.foreground
                        } else {
                            colors.secondary
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_editor_protocol(p, window, cx);
                        }))
                        .child(label)
                }),
            );

        let general_section = div()
            .p(px(12.))
            .rounded(px(10.))
            .bg(colors.surface_glass())
            .border_1()
            .border_color(colors.hairline())
            .flex()
            .flex_col()
            .gap(px(9.))
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.muted))
                    .child(language.text("IDENTITY", "协议与标识")),
            )
            .child(proto_seg)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text("Host Name", "主机名称")),
                            )
                            .child(input_field(&editor.name, None, colors)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text("Group", "所属分组")),
                            )
                            .child(input_field(&editor.group, None, colors)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors.secondary))
                            .child(language.text("Tags (comma separated)", "标签（逗号分隔）")),
                    )
                    .child(input_field(&editor.tags, None, colors)),
            );

        let conn_section = div()
            .p(px(12.))
            .rounded(px(10.))
            .bg(colors.surface_glass())
            .border_1()
            .border_color(colors.hairline())
            .flex()
            .flex_col()
            .gap(px(9.))
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.muted))
                    .child(if is_serial {
                        language.text("SERIAL PORT", "串口参数")
                    } else {
                        language.text("CONNECTION", "连接目标")
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .child(if is_serial {
                                        language.text("Device Path", "串口设备路径")
                                    } else {
                                        language.text("Address / IP", "主机地址 / IP")
                                    }),
                            )
                            .child(input_field(&editor.address, None, colors)),
                    )
                    .child(
                        div()
                            .w(px(92.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .child(if is_serial {
                                        language.text("Baud Rate", "波特率")
                                    } else {
                                        language.text("Port", "端口")
                                    }),
                            )
                            .child(input_field(&editor.port, None, colors)),
                    ),
            )
            .children((!is_serial && !is_telnet).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors.secondary))
                            .child(language.text("Username", "用户名")),
                    )
                    .child(input_field(&editor.username, None, colors))
            }));

        let auth_section = (!is_serial && !is_telnet).then(|| {
            div()
                .p(px(12.))
                .rounded(px(10.))
                .bg(colors.surface_glass())
                .border_1()
                .border_color(colors.hairline())
                .flex()
                .flex_col()
                .gap(px(9.))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.muted))
                        .child(language.text("AUTH & BASTION", "认证与跳板机")),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(colors.secondary))
                                .child(language.text("Keychain Credential", "密钥库凭证")),
                        )
                        .child(Select::new(&editor.keychain_select).text_xs()),
                )
                .children((!using_keychain).then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.secondary))
                                        .child(language.text("Identity File (optional)", "私钥路径（可选）")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .child(input_field(&editor.identity_file, None, colors)),
                                        )
                                        .child(
                                            secondary_button(
                                                "host-browse-id",
                                                Some(Icon::Folder),
                                                language.text("Browse", "浏览"),
                                                colors,
                                            )
                                            .h(px(28.))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.browse_identity_file(window, cx);
                                            })),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.secondary))
                                        .child(language.text("Password (optional)", "密码（可选，本地加密）")),
                                )
                                .child(input_field(&editor.password, Some(Icon::Lock), colors)),
                        )
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(colors.secondary))
                                .child(language.text("Jump Host / Bastion", "跳板机")),
                        )
                        .child(Select::new(&editor.jump_select).text_xs()),
                )
                .children((!matches!(editor.jump_choice, JumpChoice::None)).then(|| {
                    let using_relay_kc =
                        !matches!(editor.jump_relay_keychain_choice, KeychainChoice::None);
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.secondary))
                                        .child(language.text(
                                            "Jump Relay Command (optional)",
                                            "跳板机中继命令（可选）",
                                        )),
                                )
                                .child(input_field(&editor.jump_relay_command, Some(Icon::Terminal), colors)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.secondary))
                                        .child(language.text(
                                            "Relay Password Credential (optional)",
                                            "中继密码凭证（密钥库）",
                                        )),
                                )
                                .child(Select::new(&editor.jump_relay_keychain_select).text_xs()),
                        )
                        .children((!using_relay_kc).then(|| {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.secondary))
                                        .child(language.text(
                                            "Relay Password (optional)",
                                            "中继命令密码（可选直填）",
                                        )),
                                )
                                .child(input_field(&editor.jump_relay_password, Some(Icon::Lock), colors))
                        }))
                }))
        });

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(general_section)
            .child(conn_section)
            .children(auth_section)
    }

    fn render_inspector(
        &self,
        editor: &HostEditor,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let edit_id = editor.edit_id;
        let is_edit = edit_id.is_some();
        let raw_name = editor.name.read(cx).value().trim().to_owned();
        let display_title = if is_edit {
            if raw_name.is_empty() {
                language.text("Edit Host", "编辑主机").to_owned()
            } else {
                raw_name
            }
        } else {
            language.text("New Host", "新建主机").to_owned()
        };

        div()
            .id("inspector-drawer-panel")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(340.))
            .bg(colors.popover_bg())
            .border_l_1()
            .border_color(colors.hairline_strong())
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(54.))
                    .px(px(16.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .truncate()
                            .child(display_title),
                    )
                    .child(
                        Button::new("inspector-close")
                            .ghost()
                            .xsmall()
                            .icon(Icon::Close)
                            .on_click(cx.listener(|this, _, _, cx| this.close_editor(cx))),
                    ),
            )
            .child(
                div()
                    .id("inspector-body-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(14.))
                    .child(self.render_editor_fields(editor, colors, language, cx)),
            )
            .child(
                div()
                    .px(px(14.))
                    .py(px(11.))
                    .border_t_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(if let Some(id) = edit_id {
                        danger_button(
                            "insp-del",
                            Some(Icon::Trash),
                            language.text("Delete", "删除"),
                            false,
                            colors,
                        )
                        .h(px(28.))
                        .px(px(9.))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.delete_host(id, window, cx);
                        }))
                        .into_any_element()
                    } else {
                        secondary_button(
                            "insp-cancel",
                            None,
                            language.text("Cancel", "取消"),
                            colors,
                        )
                        .h(px(28.))
                        .px(px(10.))
                        .on_click(cx.listener(|this, _, _, cx| this.close_editor(cx)))
                        .into_any_element()
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                secondary_button(
                                    "insp-save",
                                    None,
                                    if is_edit {
                                        language.text("Save", "保存")
                                    } else {
                                        language.text("Save Host", "保存主机")
                                    },
                                    colors,
                                )
                                .h(px(28.))
                                .px(px(10.))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_editor(false, window, cx);
                                })),
                            )
                            .child(
                                primary_button(
                                    "insp-connect",
                                    Some(Icon::Play),
                                    language.text("Save & Connect", "保存并连接"),
                                    colors,
                                )
                                .h(px(28.))
                                .px(px(10.))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_editor(true, window, cx);
                                })),
                            ),
                    ),
            )
    }

    fn protocol_label(proto: HostProtocol) -> &'static str {
        match proto {
            HostProtocol::Ssh => "SSH",
            HostProtocol::Mosh => "MOSH",
            HostProtocol::Telnet => "TELNET",
            HostProtocol::Serial => "SERIAL",
        }
    }

    fn protocol_color(proto: HostProtocol, colors: Colors) -> u32 {
        match proto {
            HostProtocol::Ssh => colors.accent,
            HostProtocol::Mosh => 0xbf5af2,
            HostProtocol::Telnet => colors.secondary,
            HostProtocol::Serial => colors.warning(),
        }
    }
}

impl Render for HostsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
        let vibrancy = saved.vibrancy;
        let opacity = saved.opacity;
        let language = saved.language;
        let keychain = saved.keychain.clone();
        let hosts = self.filtered_hosts(cx);
        let selected_id = self.selected_host_id;

        let title = self
            .group_filter
            .clone()
            .unwrap_or_else(|| language.text("All Hosts", "全部主机").to_owned());

        let header = div()
            .h(px(54.))
            .px(px(22.))
            .border_b_1()
            .border_color(colors.hairline())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.foreground))
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .p(px(2.))
                            .rounded(px(7.))
                            .bg(colors.surface_glass())
                            .border_1()
                            .border_color(colors.hairline())
                            .gap(px(2.))
                            .child(
                                div()
                                    .id("hosts-view-cards")
                                    .px(px(8.))
                                    .h(px(23.))
                                    .rounded(px(5.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(11.5))
                                    .bg(if self.view_mode == HostViewMode::Cards {
                                        colors.surface_active_glass()
                                    } else {
                                        transparent_black().into()
                                    })
                                    .text_color(rgb(if self.view_mode == HostViewMode::Cards {
                                        colors.foreground
                                    } else {
                                        colors.secondary
                                    }))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.view_mode = HostViewMode::Cards;
                                        cx.notify();
                                    }))
                                    .child(Icon::LayoutGrid.render(px(11.5), colors.secondary))
                                    .child(language.text("Cards", "卡片")),
                            )
                            .child(
                                div()
                                    .id("hosts-view-list")
                                    .px(px(8.))
                                    .h(px(23.))
                                    .rounded(px(5.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(11.5))
                                    .bg(if self.view_mode == HostViewMode::List {
                                        colors.surface_active_glass()
                                    } else {
                                        transparent_black().into()
                                    })
                                    .text_color(rgb(if self.view_mode == HostViewMode::List {
                                        colors.foreground
                                    } else {
                                        colors.secondary
                                    }))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.view_mode = HostViewMode::List;
                                        cx.notify();
                                    }))
                                    .child(Icon::List.render(px(11.5), colors.secondary))
                                    .child(language.text("List", "列表")),
                            ),
                    )
                    .child(
                        secondary_button(
                            "hosts-import-ssh",
                            Some(Icon::Import),
                            language.text("Import SSH Config", "导入 SSH Config"),
                            colors,
                        )
                        .h(px(28.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.import_ssh_config(window, cx);
                        })),
                    )
                    .child(
                        primary_button(
                            "hosts-new-btn",
                            Some(Icon::Plus),
                            language.text("New Host", "新建主机"),
                            colors,
                        )
                        .h(px(28.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_new_host_modal(window, cx);
                        })),
                    ),
            );

        let filter_bar = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .mb(px(16.))
            .children(
                [
                    (None, language.text("All Protocols", "全部协议")),
                    (Some(HostProtocol::Ssh), "SSH"),
                    (Some(HostProtocol::Mosh), "Mosh"),
                    (Some(HostProtocol::Telnet), "Telnet"),
                    (Some(HostProtocol::Serial), "Serial"),
                ]
                .into_iter()
                .enumerate()
                .map(|(idx, (proto, label))| {
                    let active = self.proto_filter == proto;
                    div()
                        .id(("proto-filter", idx))
                        .px(px(10.))
                        .py(px(3.))
                        .rounded_full()
                        .cursor_pointer()
                        .border_1()
                        .border_color(if active {
                            colors.accent_border()
                        } else {
                            colors.hairline()
                        })
                        .bg(if active {
                            colors.accent_soft()
                        } else {
                            transparent_black().into()
                        })
                        .text_size(px(11.5))
                        .font_weight(if active {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(rgb(if active {
                            colors.accent
                        } else {
                            colors.secondary
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.proto_filter = proto;
                            cx.notify();
                        }))
                        .child(label)
                }),
            );

        let content_body = if hosts.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .py(px(60.))
                .child(Icon::Server.badge(px(44.)))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.foreground))
                        .child(language.text("No matching hosts", "暂无主机")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(colors.secondary))
                        .child(language.text(
                            "Create a new host or import directly from ~/.ssh/config.",
                            "点击右上角「新建主机」，或从 ~/.ssh/config 一键导入。",
                        )),
                )
                .into_any_element()
        } else if self.view_mode == HostViewMode::List {
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .border_b_1()
                        .border_color(colors.hairline())
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors.muted))
                        .child(div().w(px(180.)).child(language.text("Name", "名称")))
                        .child(div().w(px(74.)).child(language.text("Protocol", "协议")))
                        .child(div().flex_1().child(language.text("Endpoint", "连接地址")))
                        .child(div().w(px(140.)).child(language.text("Auth", "密钥 / 凭证")))
                        .child(div().w(px(110.)).child(language.text("Group", "分组")))
                        .child(div().w(px(68.))),
                )
                .children(hosts.into_iter().map(|host| {
                    let id = host.id;
                    let is_sel = selected_id == Some(id);
                    let can_sftp = supports_sftp(&host, &saved.hosts);
                    let auth_label = host
                        .keychain_id
                        .and_then(|kid| keychain.iter().find(|k| k.id == kid))
                        .map(|k| k.name.clone())
                        .unwrap_or_else(|| {
                            if !host.identity_file.is_empty() {
                                host.identity_file.clone()
                            } else {
                                "Auto".into()
                            }
                        });
                    let host_click = host.clone();
                    let host_conn = host.clone();
                    let host_sftp = host.clone();

                    div()
                        .id(("host-list-row", id))
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .border_b_1()
                        .border_color(colors.hairline())
                        .bg(if is_sel {
                            colors.accent_soft()
                        } else {
                            transparent_black().into()
                        })
                        .hover(|s| s.bg(colors.surface_hover_glass()))
                        .flex()
                        .items_center()
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            this.handle_host_click(
                                host_click.clone(),
                                event.click_count(),
                                window,
                                cx,
                            );
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.open_context_menu(id, ev.position, cx);
                            }),
                        )
                        .child(
                            div()
                                .w(px(180.))
                                .pr(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(host.icon.icon().badge_with_color(
                                    px(22.),
                                    match host.protocol {
                                        HostProtocol::Ssh => host.icon.icon().macos_color(),
                                        HostProtocol::Mosh => 0xaf52de,
                                        HostProtocol::Telnet => 0x636366,
                                        HostProtocol::Serial => 0xff9500,
                                    },
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(colors.foreground))
                                        .child(host.name.clone()),
                                ),
                        )
                        .child(
                            div().w(px(74.)).flex().items_center().child(
                                div()
                                    .px(px(5.))
                                    .py(px(1.5))
                                    .rounded(px(4.))
                                    .bg(colors.surface_active_glass())
                                    .text_size(px(9.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(Self::protocol_color(host.protocol, colors)))
                                    .child(Self::protocol_label(host.protocol)),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .pr(px(8.))
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(rgb(colors.secondary))
                                .child(host.endpoint()),
                        )
                        .child(
                            div()
                                .w(px(140.))
                                .pr(px(8.))
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(rgb(colors.muted))
                                .child(auth_label),
                        )
                        .child(
                            div()
                                .w(px(110.))
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(rgb(colors.secondary))
                                .child(if host.group.is_empty() {
                                    "—".to_owned()
                                } else {
                                    host.group.clone()
                                }),
                        )
                        .child(
                            div()
                                .w(px(68.))
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(2.))
                                .children(can_sftp.then(|| {
                                    Button::new(("hl-sftp", id))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::Folder)
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.stop_propagation();
                                            cx.emit(HostsEvent::OpenSftp(host_sftp.clone()));
                                        }))
                                }))
                                .child(
                                    Button::new(("hl-conn", id))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::Play)
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.stop_propagation();
                                            cx.emit(HostsEvent::Connect(host_conn.clone()));
                                        })),
                                ),
                        )
                }))
                .into_any_element()
        } else {
            let grouped = group_hosts(hosts.iter());
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .children(grouped.into_iter().map(|(group_name, items)| {
                    let group_label = if group_name.is_empty() {
                        language.text("Ungrouped", "未分组").to_owned()
                    } else {
                        group_name
                    };
                    let count = items.len();
                    let owned_items: Vec<HostProfile> = items.into_iter().cloned().collect();

                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(11.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(colors.secondary))
                                .child(group_label)
                                .child(
                                    div()
                                        .text_color(rgb(colors.muted))
                                        .child(count.to_string()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(10.))
                                .children(owned_items.into_iter().map(|host| {
                                    let id = host.id;
                                    let is_sel = selected_id == Some(id);
                                    let can_sftp = supports_sftp(&host, &saved.hosts);
                                    let badge_bg = match host.protocol {
                                        HostProtocol::Ssh => host.icon.icon().macos_color(),
                                        HostProtocol::Mosh => 0xaf52de,
                                        HostProtocol::Telnet => 0x636366,
                                        HostProtocol::Serial => 0xff9500,
                                    };
                                    let card_group: gpui_kit::SharedString = format!("host-card-{id}").into();
                                    let host_click = host.clone();
                                    let host_conn = host.clone();
                                    let host_sftp = host.clone();

                                    div()
                                        .id(("host-card", id))
                                        .group(card_group.clone())
                                        .w(px(264.))
                                        .p(px(12.))
                                        .rounded(px(10.))
                                        .cursor_pointer()
                                        .bg(if is_sel {
                                            colors.accent_soft()
                                        } else {
                                            colors.surface_glass()
                                        })
                                        .border_1()
                                        .border_color(if is_sel {
                                            colors.accent_border()
                                        } else {
                                            colors.hairline()
                                        })
                                        .hover(|s| {
                                            s.bg(colors.surface_hover_glass())
                                                .border_color(colors.hairline_strong())
                                        })
                                        .flex()
                                        .flex_col()
                                        .gap(px(9.))
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, window, cx| {
                                                this.handle_host_click(
                                                    host_click.clone(),
                                                    event.click_count(),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ))
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                                cx.stop_propagation();
                                                this.open_context_menu(id, ev.position, cx);
                                            }),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(10.))
                                                .child(host.icon.icon().badge_with_color(px(32.), badge_bg))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(2.))
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .items_center()
                                                                .gap(px(6.))
                                                                .child(
                                                                    div()
                                                                        .truncate()
                                                                        .text_size(px(13.))
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(rgb(
                                                                            colors.foreground,
                                                                        ))
                                                                        .child(host.name.clone()),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex_shrink_0()
                                                                        .px(px(5.))
                                                                        .py(px(1.))
                                                                        .rounded(px(4.))
                                                                        .bg(colors
                                                                            .surface_active_glass())
                                                                        .text_size(px(9.5))
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(rgb(
                                                                            colors.secondary,
                                                                        ))
                                                                        .child(
                                                                            Self::protocol_label(
                                                                                host.protocol,
                                                                            ),
                                                                        ),
                                                                ),
                                                        )
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .text_size(px(11.))
                                                                .text_color(rgb(colors.secondary))
                                                                .child(host.endpoint()),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(6.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(4.))
                                                        .flex_wrap()
                                                        .children(host.tags.iter().take(3).map(
                                                            |t| {
                                                                div()
                                                                    .px(px(6.))
                                                                    .py(px(1.))
                                                                    .rounded(px(4.))
                                                                    .bg(colors.surface_glass())
                                                                    .border_1()
                                                                    .border_color(colors.hairline())
                                                                    .text_size(px(10.))
                                                                    .text_color(rgb(
                                                                        colors.secondary,
                                                                    ))
                                                                    .child(format!("#{t}"))
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(2.))
                                                        .opacity(0.0)
                                                        .group_hover(card_group, |s| s.opacity(1.0))
                                                        .children(can_sftp.then(|| {
                                                            Button::new(("hc-sftp", id))
                                                                .ghost()
                                                                .xsmall()
                                                                .icon(Icon::Folder)
                                                                .on_click(cx.listener(
                                                                    move |_, _, _, cx| {
                                                                        cx.stop_propagation();
                                                                        cx.emit(
                                                                            HostsEvent::OpenSftp(
                                                                                host_sftp.clone(),
                                                                            ),
                                                                        );
                                                                    },
                                                                ))
                                                        }))
                                                        .child(
                                                            Button::new(("hc-conn", id))
                                                                .ghost()
                                                                .xsmall()
                                                                .icon(Icon::Play)
                                                                .on_click(cx.listener(
                                                                    move |_, _, _, cx| {
                                                                        cx.stop_propagation();
                                                                        cx.emit(
                                                                            HostsEvent::Connect(
                                                                                host_conn.clone(),
                                                                            ),
                                                                        );
                                                                    },
                                                                )),
                                                        ),
                                                ),
                                        )
                                })),
                        )
                }))
                .into_any_element()
        };

        let inspector_el = if let Some(e) = &self.editor {
            Some(self.render_inspector(e, colors, language, cx))
        } else {
            None
        };
        let context_menu_el = if let Some((id, pos)) = self.context_menu {
            Some(self.render_context_menu(id, pos, colors, language, cx))
        } else {
            None
        };

        div()
            .relative()
            .size_full()
            .text_color(rgb(colors.foreground))
            .flex()
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(colors.content_bg(vibrancy, opacity))
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .id("hosts-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(22.))
                            .py(px(16.))
                            .flex()
                            .flex_col()
                            .child(filter_bar)
                            .child(content_body),
                    ),
            )
            .children(inspector_el)
            .children(context_menu_el)
    }
}
