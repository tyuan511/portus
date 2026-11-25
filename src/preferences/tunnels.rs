use std::collections::HashMap;

use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window,
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
    HostProfile, Language, PortForwardRule, Settings, SettingsStore, TunnelKind,
};
use crate::theme::Colors;
use crate::tunnel::{TunnelEvent, TunnelHandle, start_tunnel};

use super::{SelectChoice, danger_button, input_field, primary_button, secondary_button};

fn build_tunnel_kind_items(language: Language) -> Vec<SelectChoice<TunnelKind>> {
    [
        (TunnelKind::Local, language.text("Local (-L)", "本地转发 (-L)")),
        (TunnelKind::Remote, language.text("Remote (-R)", "远程转发 (-R)")),
        (
            TunnelKind::Dynamic,
            language.text("Dynamic SOCKS5 (-D)", "动态 SOCKS5 (-D)"),
        ),
    ]
    .into_iter()
    .map(|(kind, label)| SelectChoice::new(label, kind))
    .collect()
}

fn build_tunnel_host_items(hosts: &[HostProfile]) -> Vec<SelectChoice<u64>> {
    hosts
        .iter()
        .map(|h| SelectChoice::new(format!("{} ({})", h.name, h.endpoint()), h.id))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TunnelState {
    Starting,
    Active,
}

struct TunnelEditor {
    id: Option<u64>,
    kind: TunnelKind,
    host_id: u64,
    name: Entity<InputState>,
    bind_address: Entity<InputState>,
    bind_port: Entity<InputState>,
    target_host: Entity<InputState>,
    target_port: Entity<InputState>,
    kind_select: Entity<SelectState<Vec<SelectChoice<TunnelKind>>>>,
    host_select: Entity<SelectState<Vec<SelectChoice<u64>>>>,
    _select_subs: Vec<Subscription>,
}

impl TunnelEditor {
    fn new(
        rule: Option<&PortForwardRule>,
        default_host_id: u64,
        hosts: &[HostProfile],
        language: Language,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let (id, kind, host_id, name_v, bind_addr_v, bind_port_v, target_h_v, target_p_v) =
            match rule {
                Some(r) => (
                    Some(r.id),
                    r.kind,
                    r.host_id,
                    r.name.clone(),
                    r.bind_address.clone(),
                    r.bind_port.to_string(),
                    r.target_host.clone(),
                    if r.target_port == 0 {
                        String::new()
                    } else {
                        r.target_port.to_string()
                    },
                ),
                None => (
                    None,
                    TunnelKind::Local,
                    default_host_id,
                    String::new(),
                    "127.0.0.1".into(),
                    "8080".into(),
                    "127.0.0.1".into(),
                    "80".into(),
                ),
            };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Postgres Tunnel")
                .default_value(name_v)
        });
        let bind_address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("127.0.0.1")
                .default_value(bind_addr_v)
        });
        let bind_port = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("8080")
                .default_value(bind_port_v)
        });
        let target_host = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("127.0.0.1")
                .default_value(target_h_v)
        });
        let target_port = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("5432")
                .default_value(target_p_v)
        });
        let kind_items = build_tunnel_kind_items(language);
        let kind_ix = kind_items
            .iter()
            .position(|item| item.value == kind)
            .map(|row| IndexPath::default().row(row));
        let kind_select = cx.new(|cx| SelectState::new(kind_items, kind_ix, window, cx));

        let host_items = build_tunnel_host_items(hosts);
        let host_ix = host_items
            .iter()
            .position(|item| item.value == host_id)
            .map(|row| IndexPath::default().row(row));
        let host_select = cx.new(|cx| SelectState::new(host_items, host_ix, window, cx));

        Self {
            id,
            kind,
            host_id,
            name,
            bind_address,
            bind_port,
            target_host,
            target_port,
            kind_select,
            host_select,
            _select_subs: Vec::new(),
        }
    }
}

pub struct TunnelsPage {
    store: Entity<SettingsStore>,
    editor: Option<TunnelEditor>,
    running: HashMap<u64, (TunnelState, TunnelHandle)>,
    prompt_sub: Option<Subscription>,
    _updates: Subscription,
}

impl TunnelsPage {
    pub fn new(store: Entity<SettingsStore>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let updates = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            store,
            editor: None,
            running: HashMap::new(),
            prompt_sub: None,
            _updates: updates,
        }
    }

    pub fn active_count_for_host(&self, host_id: u64, cx: &App) -> usize {
        let rules = &self.store.read(cx).current.tunnels;
        self.running
            .keys()
            .filter(|id| rules.iter().any(|r| r.id == **id && r.host_id == host_id))
            .count()
    }

    fn open_editor(
        &mut self,
        rule: Option<&PortForwardRule>,
        default_host_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hosts = self.store.read(cx).current.hosts.clone();
        let language = self.language(cx);
        let mut editor = TunnelEditor::new(rule, default_host_id, &hosts, language, window, cx);
        let kind_sub = cx.subscribe_in(
            &editor.kind_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<TunnelKind>>>, _, cx| {
                if let SelectEvent::Confirm(Some(kind)) = event
                    && let Some(editor) = &mut this.editor
                {
                    editor.kind = *kind;
                    cx.notify();
                }
            },
        );
        let host_sub = cx.subscribe_in(
            &editor.host_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<u64>>>, _, cx| {
                if let SelectEvent::Confirm(Some(hid)) = event
                    && let Some(editor) = &mut this.editor
                {
                    editor.host_id = *hid;
                    cx.notify();
                }
            },
        );
        editor._select_subs = vec![kind_sub, host_sub];
        editor.name.update(cx, |f, cx| f.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    pub fn edit_rule_for_host(
        &mut self,
        id: Option<u64>,
        host_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = id.and_then(|id| self.saved_rule(id, cx));
        self.open_editor(existing.as_ref(), host_id, window, cx);
    }

    pub fn shutdown_all(&mut self) {
        self.prompt_sub = None;
        for (_, (_, mut handle)) in self.running.drain() {
            handle.stop();
        }
    }

    fn language(&self, cx: &App) -> Language {
        self.store.read(cx).current.language
    }

    fn saved_rule(&self, id: u64, cx: &App) -> Option<PortForwardRule> {
        self.store
            .read(cx)
            .current
            .tunnels
            .iter()
            .find(|r| r.id == id)
            .cloned()
    }

    fn edit_rule(&mut self, id: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let existing = id.and_then(|id| self.saved_rule(id, cx));
        let default_host = self
            .store
            .read(cx)
            .current
            .hosts
            .first()
            .map(|h| h.id)
            .unwrap_or(0);
        self.open_editor(existing.as_ref(), default_host, window, cx);
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
        if let Err(error) = result {
            crate::notify::error(error, window, cx);
            false
        } else {
            true
        }
    }

    fn save_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(editor) = &self.editor else {
            return false;
        };
        let language = self.language(cx);
        let name = editor.name.read(cx).value().trim().to_owned();
        let bind_address = editor.bind_address.read(cx).value().trim().to_owned();
        let bind_port = editor
            .bind_port
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0);
        let target_host = editor.target_host.read(cx).value().trim().to_owned();
        let target_port = editor
            .target_port
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0);

        if name.is_empty() {
            crate::notify::error(
                language.text("Enter a tunnel name.", "请输入隧道名称。"),
                window,
                cx,
            );
            return false;
        }
        if editor.host_id == 0 {
            crate::notify::error(
                language.text("Select an SSH host.", "请选择一台 SSH 主机。"),
                window,
                cx,
            );
            return false;
        }
        let Some(bind_port) = bind_port else {
            crate::notify::error(
                language.text(
                    "Enter a valid bind port (1-65535).",
                    "请输入有效的监听端口（1-65535）。",
                ),
                window,
                cx,
            );
            return false;
        };
        let (target_host, target_port) = if editor.kind == TunnelKind::Dynamic {
            (String::new(), 0)
        } else {
            let Some(tp) = target_port else {
                crate::notify::error(
                    language.text(
                        "Enter a valid target port (1-65535).",
                        "请输入有效的目标端口（1-65535）。",
                    ),
                    window,
                    cx,
                );
                return false;
            };
            if target_host.is_empty() {
                crate::notify::error(
                    language.text("Enter a target host.", "请输入目标主机地址。"),
                    window,
                    cx,
                );
                return false;
            }
            (target_host, tp)
        };

        let allocation = self.store.read(cx).current.tunnel_ids(1);
        let Some(id) = editor.id.or_else(|| allocation.map(|r| r.start)) else {
            return false;
        };
        let rule = PortForwardRule {
            id,
            name,
            kind: editor.kind,
            host_id: editor.host_id,
            bind_address: if bind_address.is_empty() {
                "127.0.0.1".into()
            } else {
                bind_address
            },
            bind_port,
            target_host,
            target_port,
        };
        if self.save(
            |settings| {
                if let Some(existing) = settings.tunnels.iter_mut().find(|r| r.id == id) {
                    *existing = rule;
                } else {
                    settings.tunnels.push(rule);
                    settings.next_tunnel_id = id + 1;
                }
            },
            window,
            cx,
        ) {
            if let Some(editor) = &mut self.editor {
                editor.id = Some(id);
            }
            crate::notify::success(
                language.text("Tunnel rule saved.", "端口转发规则已保存。"),
                window,
                cx,
            );
            cx.notify();
            true
        } else {
            false
        }
    }

    fn toggle_rule(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, mut handle)) = self.running.remove(&id) {
            handle.stop();
            cx.notify();
            return;
        }
        let Some(rule) = self.saved_rule(id, cx) else {
            return;
        };
        let current = &self.store.read(cx).current;
        let Some(host) = current.hosts.iter().find(|h| h.id == rule.host_id).cloned() else {
            crate::notify::error(
                self.language(cx)
                    .text("Bound SSH host no longer exists.", "绑定的 SSH 主机不存在。"),
                window,
                cx,
            );
            return;
        };
        let all_hosts = current.hosts.clone();
        let (tx, rx) = async_channel::unbounded::<TunnelEvent>();
        let handle = start_tunnel(rule, host, all_hosts, tx);
        self.running.insert(id, (TunnelState::Starting, handle));

        cx.spawn_in(window, async move |view, cx| {
            while let Ok(ev) = rx.recv().await {
                let keep = cx.update(|window, cx| {
                    view.update(cx, |this, cx| this.handle_tunnel_event(ev, window, cx))
                        .is_ok()
                });
                if !keep.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn handle_tunnel_event(
        &mut self,
        event: TunnelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TunnelEvent::Starting(id) => {
                if let Some((state, _)) = self.running.get_mut(&id) {
                    *state = TunnelState::Starting;
                }
            }
            TunnelEvent::Active(id) => {
                if let Some((state, _)) = self.running.get_mut(&id) {
                    *state = TunnelState::Active;
                }
                self.prompt_sub = None;
            }
            TunnelEvent::Stopped(id) => {
                self.running.remove(&id);
            }
            TunnelEvent::Prompt(_id, prompt) => {
                let language = self.language(cx);
                self.prompt_sub = crate::notify::push_ssh_prompt(prompt, language, window, cx);
            }
            TunnelEvent::Error(id, err) => {
                self.running.remove(&id);
                self.prompt_sub = None;
                crate::notify::error(err, window, cx);
            }
        }
        cx.notify();
    }

    fn remove_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editor.as_ref().and_then(|e| e.id) else {
            return;
        };
        let language = self.language(cx);
        if let Some((_, mut handle)) = self.running.remove(&id) {
            handle.stop();
        }
        if self.save(
            |settings| settings.tunnels.retain(|r| r.id != id),
            window,
            cx,
        ) {
            self.editor = None;
            crate::notify::success(
                language.text("Tunnel rule removed.", "端口转发规则已删除。"),
                window,
                cx,
            );
            cx.notify();
        }
    }

    fn list(&self, colors: Colors, language: Language, cx: &Context<Self>) -> impl IntoElement {
        let selected = self.editor.as_ref().and_then(|e| e.id);
        let rules = &self.store.read(cx).current.tunnels;
        let total = rules.len();

        let mut entries = div()
            .id("tunnels-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.));

        if rules.is_empty() {
            entries = entries.child(
                div()
                    .px(px(10.))
                    .py(px(18.))
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(rgb(colors.muted))
                    .child(language.text("No port forwarding rules yet", "暂无端口转发规则")),
            );
        }

        entries = entries.children(rules.iter().map(|rule| {
            let id = rule.id;
            let is_selected = selected == Some(id);
            let running_state = self.running.get(&id).map(|(s, _)| *s);
            let badge = match rule.kind {
                TunnelKind::Local => "-L",
                TunnelKind::Remote => "-R",
                TunnelKind::Dynamic => "-D",
            };
            let summary = match rule.kind {
                TunnelKind::Dynamic => format!("{}:{} (SOCKS5)", rule.bind_address, rule.bind_port),
                _ => format!(
                    "{}:{} → {}:{}",
                    rule.bind_address, rule.bind_port, rule.target_host, rule.target_port
                ),
            };
            div()
                .relative()
                .w_full()
                .h(px(56.))
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
                .flex()
                .items_center()
                .gap(px(8.))
                .hover(move |s| {
                    if is_selected {
                        s.bg(colors.accent_subtle())
                    } else {
                        s.bg(colors.hover_bg())
                    }
                })
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
                    Button::new(("tunnel", id))
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .bg(transparent_black())
                        .justify_start()
                        .gap(px(8.))
                        .cursor_pointer()
                        .child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded_md()
                                .bg(rgb(colors.surface))
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(match running_state {
                                    Some(TunnelState::Active) => colors.success,
                                    Some(TunnelState::Starting) => colors.accent,
                                    None => colors.secondary,
                                }))
                                .child(badge),
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
                                        .child(rule.name.clone()),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .truncate()
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.muted))
                                        .child(summary),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.edit_rule(Some(id), window, cx)
                        })),
                )
                .child(
                    Button::new(("tunnel-toggle", id))
                        .xsmall()
                        .h(px(24.))
                        .px(px(8.))
                        .rounded_md()
                        .bg(if running_state.is_some() {
                            colors.danger_soft()
                        } else {
                            colors.accent_soft()
                        })
                        .text_color(rgb(if running_state.is_some() {
                            colors.danger
                        } else {
                            colors.accent
                        }))
                        .cursor_pointer()
                        .child(match running_state {
                            Some(TunnelState::Active) => language.text("Stop", "停止"),
                            Some(TunnelState::Starting) => language.text("…", "…"),
                            None => language.text("Start", "启动"),
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_rule(id, window, cx)
                        })),
                )
        }));

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(280.))
            .bg(rgb(colors.panel))
            .border_r_1()
            .border_color(colors.hairline())
            .child(
                div()
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
                                    .child(language.text("Port Forwarding", "端口转发")),
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
                        primary_button(
                            "add-tunnel",
                            Some(Icon::Plus),
                            language.text("New Tunnel", "新建隧道"),
                            colors,
                        )
                        .w_full()
                        .h(px(30.))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.edit_rule(None, window, cx)),
                        ),
                    ),
            )
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
                    .child(Icon::Network.render(px(24.), colors.accent)),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.foreground))
                    .child(language.text("SSH Port Forwarding", "SSH 端口转发")),
            )
            .child(
                div()
                    .max_w(px(360.))
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(rgb(colors.muted))
                    .child(language.text(
                        "Create Local (-L), Remote (-R), and Dynamic SOCKS5 (-D) tunnels and toggle them with one click.",
                        "创建本地转发（-L）、远程转发（-R）与动态 SOCKS5 代理（-D）隧道，一键启停。",
                    )),
            )
            .child(
                primary_button(
                    "empty-add-tunnel",
                    Some(Icon::Plus),
                    language.text("New Tunnel", "新建隧道"),
                    colors,
                )
                .on_click(cx.listener(|this, _, window, cx| this.edit_rule(None, window, cx))),
            )
    }

    fn form(
        &self,
        editor: &TunnelEditor,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let saved = editor.id.and_then(|id| self.saved_rule(id, cx));
        let title = if saved.is_some() {
            language.text("Edit Tunnel", "编辑端口转发")
        } else {
            language.text("New Tunnel", "新建端口转发")
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
            .gap(px(8.))
            .child(
                primary_button(
                    "save-tunnel",
                    Some(Icon::Check),
                    language.text("Save Rule", "保存规则"),
                    colors,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.save_rule(window, cx);
                })),
            )
            .child(
                secondary_button(
                    "cancel-tunnel",
                    None,
                    language.text("Cancel", "取消"),
                    colors,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.editor = None;
                    cx.notify();
                })),
            );
        if let Some(id) = saved.as_ref().map(|r| r.id) {
            let is_running = self.running.contains_key(&id);
            left = left.child(
                Button::new("run-tunnel-btn")
                    .ghost()
                    .small()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded_lg()
                    .bg(if is_running {
                        colors.danger_soft()
                    } else {
                        colors.success_soft()
                    })
                    .border_1()
                    .border_color(if is_running {
                        colors.danger_border()
                    } else {
                        colors.success_border()
                    })
                    .text_color(rgb(if is_running {
                        colors.danger
                    } else {
                        colors.success
                    }))
                    .icon(Icon::Play)
                    .label(if is_running {
                        language.text("Stop Tunnel", "停止隧道")
                    } else {
                        language.text("Start Tunnel", "启动隧道")
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_rule(id, window, cx)
                    })),
            );
        }
        actions = actions.child(left);
        if saved.is_some() {
            let page = cx.entity().downgrade();
            actions = actions.child(
                danger_button(
                    "remove-tunnel",
                    Some(Icon::Trash),
                    language.text("Remove", "删除"),
                    false,
                    colors,
                )
                .on_click(move |_, window, cx| {
                    let page = page.clone();
                    crate::notify::confirm(
                        language.text("Remove Tunnel", "删除端口转发"),
                        language.text("Remove this tunnel rule?", "确定删除此端口转发规则？"),
                        language.text("Remove", "确认删除"),
                        move |window, cx| {
                            let _ = page.update(cx, |this, cx| this.remove_rule(window, cx));
                        },
                        window,
                        cx,
                    );
                }),
            );
        }

        let mut card = div()
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
                            .child(language.text("Name", "名称")),
                    )
                    .child(input_field(&editor.name, None, colors)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.secondary))
                            .child(language.text("Forwarding Type", "转发类型")),
                    )
                    .child(
                        Select::new(&editor.kind_select),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.secondary))
                            .child(language.text("SSH Host", "SSH 跳板/目标主机")),
                    )
                    .child(Select::new(&editor.host_select)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text("Bind Address", "监听地址")),
                            )
                            .child(input_field(&editor.bind_address, None, colors)),
                    )
                    .child(
                        div()
                            .w(px(120.))
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text("Bind Port", "监听端口")),
                            )
                            .child(input_field(&editor.bind_port, None, colors)),
                    ),
            );

        if editor.kind != TunnelKind::Dynamic {
            card = card.child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text("Target Host", "目标主机")),
                            )
                            .child(input_field(&editor.target_host, None, colors)),
                    )
                    .child(
                        div()
                            .w(px(120.))
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text("Target Port", "目标端口")),
                            )
                            .child(input_field(&editor.target_port, None, colors)),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("tunnel-fields")
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
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .child(title),
                            )
                            .child(card),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(12.))
                    .bg(rgb(colors.panel))
                    .border_t_1()
                    .border_color(colors.hairline())
                    .child(actions),
            )
    }

    pub fn render_host_drawer_body(
        &mut self,
        host: &HostProfile,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div()
            .id("tunnels-drawer-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.));

        if let Some(editor) = &self.editor
            && editor.host_id == host.id
        {
            let is_existing = editor.id.is_some();
            let is_dynamic = editor.kind == TunnelKind::Dynamic;
            let labeled_input = |label: &'static str, field: &Entity<InputState>| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.secondary))
                            .child(label),
                    )
                    .child(input_field(field, None, colors))
            };
            let mut form = div()
                .p(px(10.))
                .rounded_lg()
                .bg(rgb(colors.surface))
                .border_1()
                .border_color(rgb(colors.line))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(labeled_input(
                    language.text("Name", "名称"),
                    &editor.name,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(colors.secondary))
                                .child(language.text("Type", "转发类型")),
                        )
                        .child(Select::new(&editor.kind_select).small()),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_1()
                                .child(labeled_input(
                                    language.text("Bind Addr", "监听地址"),
                                    &editor.bind_address,
                                )),
                        )
                        .child(
                            div()
                                .w(px(76.))
                                .child(labeled_input(
                                    language.text("Port", "端口"),
                                    &editor.bind_port,
                                )),
                        ),
                );

            if !is_dynamic {
                form = form.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_1()
                                .child(labeled_input(
                                    language.text("Target Host", "目标主机"),
                                    &editor.target_host,
                                )),
                        )
                        .child(
                            div()
                                .w(px(76.))
                                .child(labeled_input(
                                    language.text("Port", "端口"),
                                    &editor.target_port,
                                )),
                        ),
                );
            }

            let mut actions = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pt(px(2.))
                .child(
                    primary_button(
                        "drawer-tunnel-save",
                        Some(Icon::Check),
                        language.text("Save", "保存"),
                        colors,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.save_rule(window, cx) {
                            this.editor = None;
                        }
                        cx.notify();
                    })),
                )
                .child(
                    secondary_button(
                        "drawer-tunnel-cancel",
                        None,
                        language.text("Cancel", "取消"),
                        colors,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.editor = None;
                        cx.notify();
                    })),
                );

            if is_existing {
                actions = actions.child(
                    danger_button(
                        "drawer-tunnel-delete",
                        Some(Icon::Trash),
                        "",
                        false,
                        colors,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.remove_rule(window, cx);
                    })),
                );
            }

            body = body.child(form.child(actions));
        }

        let host_id = host.id;
        let rules: Vec<PortForwardRule> = self
            .store
            .read(cx)
            .current
            .tunnels
            .iter()
            .filter(|r| r.host_id == host_id)
            .cloned()
            .collect();

        if rules.is_empty() && self.editor.is_none() {
            body = body.child(
                div()
                    .p(px(14.))
                    .text_center()
                    .text_size(px(12.))
                    .text_color(rgb(colors.muted))
                    .child(language.text(
                        "No forwarding rules for this host. Click New to add one.",
                        "此主机暂无端口转发规则，点击「新建」添加。",
                    )),
            );
        } else {
            body = body.children(rules.into_iter().map(|rule| {
                let id = rule.id;
                let running_state = self.running.get(&id).map(|(s, _)| *s);
                let badge = match rule.kind {
                    TunnelKind::Local => "-L",
                    TunnelKind::Remote => "-R",
                    TunnelKind::Dynamic => "-D",
                };
                let summary = match rule.kind {
                    TunnelKind::Dynamic => {
                        format!("{}:{} (SOCKS5)", rule.bind_address, rule.bind_port)
                    }
                    _ => format!(
                        "{}:{} → {}:{}",
                        rule.bind_address, rule.bind_port, rule.target_host, rule.target_port
                    ),
                };
                div()
                    .w_full()
                    .p(px(8.))
                    .rounded_lg()
                    .bg(rgb(colors.surface))
                    .border_1()
                    .border_color(if running_state == Some(TunnelState::Active) {
                        colors.accent_border()
                    } else {
                        rgb(colors.line_subtle).into()
                    })
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .px(px(5.))
                                    .py(px(1.))
                                    .rounded_md()
                                    .bg(rgb(colors.panel))
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(match running_state {
                                        Some(TunnelState::Active) => colors.success,
                                        Some(TunnelState::Starting) => colors.accent,
                                        None => colors.secondary,
                                    }))
                                    .child(badge),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(rule.name.clone()),
                            )
                            .child(
                                Button::new(("drawer-tunnel-edit", id))
                                    .ghost()
                                    .size(px(22.))
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::Edit.render(px(11.), colors.muted))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.edit_rule_for_host(Some(id), host_id, window, cx)
                                    })),
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
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(summary),
                            )
                            .child(
                                Button::new(("drawer-tunnel-toggle", id))
                                    .xsmall()
                                    .h(px(24.))
                                    .px(px(8.))
                                    .rounded_md()
                                    .bg(if running_state.is_some() {
                                        colors.danger_soft()
                                    } else {
                                        colors.accent_soft()
                                    })
                                    .text_color(rgb(if running_state.is_some() {
                                        colors.danger
                                    } else {
                                        colors.accent
                                    }))
                                    .cursor_pointer()
                                    .child(match running_state {
                                        Some(TunnelState::Active) => {
                                            language.text("Stop", "停止")
                                        }
                                        Some(TunnelState::Starting) => "…",
                                        None => language.text("Start", "启动"),
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.toggle_rule(id, window, cx)
                                    })),
                            ),
                    )
            }));
        }

        body.into_any_element()
    }
}

impl Render for TunnelsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
        let language = saved.language;
        let mut detail = div().flex().flex_col().flex_1().min_w_0().min_h_0();

        if let Some(editor) = &self.editor {
            detail = detail.child(self.form(editor, colors, language, cx));
        } else {
            detail = detail.child(self.empty_state(colors, language, cx));
        }
        div()
            .flex()
            .size_full()
            .min_h_0()
            .bg(rgb(colors.background))
            .text_color(rgb(colors.foreground))
            .child(self.list(colors, language, cx))
            .child(detail)
    }
}
