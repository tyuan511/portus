use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
    IntoElement, KeyDownEvent, Keystroke, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
        kbd::Kbd,
        tag::Tag,
    },
    div, px, rgb, transparent_black,
};

use crate::icons::Icon;
use crate::settings::{HostIcon, HostProfile, HostProtocol, Language, SettingsStore, Snippet};
use crate::theme::Colors;

pub enum QuickConnectEvent {
    ConnectHost(HostProfile),
    RunSnippet(Snippet),
    NewLocalTab,
    OpenHosts,
    OpenSettings,
    Close,
}

#[derive(Clone)]
enum PaletteItem {
    DirectSsh(HostProfile),
    SavedHost(HostProfile),
    Snippet(Snippet),
    NewLocalTab,
    OpenHosts,
    OpenSettings,
}

pub struct QuickConnectPalette {
    store: Entity<SettingsStore>,
    query: Entity<InputState>,
    selected: usize,
    _sub: Subscription,
}

impl EventEmitter<QuickConnectEvent> for QuickConnectPalette {}

impl QuickConnectPalette {
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = store.read(cx).current.language;
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(
                "Search hosts, user@host:port, or commands…",
                "搜索主机、输入 user@host:port 直连，或执行命令…",
            ))
        });
        let sub = cx.subscribe(&query, |this, _, event, cx| match event {
            InputEvent::Change => {
                this.selected = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                this.activate_selected(cx);
            }
            _ => {}
        });
        Self {
            store,
            query,
            selected: 0,
            _sub: sub,
        }
    }

    pub fn focus_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = 0;
        self.query.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn items(&self, cx: &App) -> Vec<PaletteItem> {
        let raw = self.query.read(cx).value().trim().to_owned();
        let needle = raw.to_lowercase();
        let hosts = &self.store.read(cx).current.hosts;
        let mut items = Vec::new();

        let mut matching_hosts: Vec<_> = hosts
            .iter()
            .filter(|host| {
                if needle.is_empty() {
                    return true;
                }
                let name = host.name.to_lowercase();
                let endpoint = host.endpoint().to_lowercase();
                let group = host.group.to_lowercase();
                needle.split_whitespace().all(|term| {
                    name.contains(term)
                        || endpoint.contains(term)
                        || group.contains(term)
                        || host.tags.iter().any(|t| t.to_lowercase().contains(term))
                })
            })
            .cloned()
            .collect();
        matching_hosts.sort_by(|a, b| {
            b.last_connected
                .cmp(&a.last_connected)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        for host in matching_hosts.into_iter().take(8) {
            items.push(PaletteItem::SavedHost(host));
        }

        let snippets = &self.store.read(cx).current.snippets;
        let matching_snippets: Vec<_> = snippets
            .iter()
            .filter(|s| {
                needle.is_empty()
                    || s.name.to_lowercase().contains(&needle)
                    || s.command.to_lowercase().contains(&needle)
                    || s.group.to_lowercase().contains(&needle)
            })
            .take(5)
            .cloned()
            .collect();
        for snippet in matching_snippets {
            items.push(PaletteItem::Snippet(snippet));
        }

        if let Some(direct) = parse_quick_ssh(&raw) {
            items.push(PaletteItem::DirectSsh(direct));
        }

        if needle.is_empty() || "local terminal shell new tab 本地 终端".contains(&needle) {
            items.push(PaletteItem::NewLocalTab);
        }
        if needle.is_empty() || "ssh hosts manage 主机 管理".contains(&needle) {
            items.push(PaletteItem::OpenHosts);
        }
        if needle.is_empty() || "settings preferences 设置 偏好".contains(&needle) {
            items.push(PaletteItem::OpenSettings);
        }

        items
    }

    fn activate_selected(&mut self, cx: &mut Context<Self>) {
        let items = self.items(cx);
        if let Some(item) = items.get(self.selected).cloned() {
            self.activate_item(item, cx);
        }
    }

    fn activate_item(&mut self, item: PaletteItem, cx: &mut Context<Self>) {
        match item {
            PaletteItem::DirectSsh(host) | PaletteItem::SavedHost(host) => {
                cx.emit(QuickConnectEvent::ConnectHost(host));
            }
            PaletteItem::Snippet(snippet) => cx.emit(QuickConnectEvent::RunSnippet(snippet)),
            PaletteItem::NewLocalTab => cx.emit(QuickConnectEvent::NewLocalTab),
            PaletteItem::OpenHosts => cx.emit(QuickConnectEvent::OpenHosts),
            PaletteItem::OpenSettings => cx.emit(QuickConnectEvent::OpenSettings),
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.emit(QuickConnectEvent::Close);
                cx.stop_propagation();
            }
            "up" => {
                let count = self.items(cx).len();
                if count > 0 {
                    self.selected = if self.selected == 0 {
                        count - 1
                    } else {
                        self.selected - 1
                    };
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "down" => {
                let count = self.items(cx).len();
                if count > 0 {
                    self.selected = (self.selected + 1) % count;
                    cx.notify();
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for QuickConnectPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors: Colors = saved.theme.colors();
        let language: Language = saved.language;
        let items = self.items(cx);
        let selected = self.selected.min(items.len().saturating_sub(1));

        let mut list = div()
            .id("quick-connect-list")
            .max_h(px(340.))
            .overflow_y_scroll()
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(2.));

        let mut last_section: Option<&'static str> = None;
        for (ix, item) in items.into_iter().enumerate() {
            let section = match &item {
                PaletteItem::DirectSsh(_) | PaletteItem::SavedHost(_) => {
                    language.text("HOSTS", "主机")
                }
                PaletteItem::Snippet(_) => language.text("SNIPPETS", "命令片段"),
                PaletteItem::NewLocalTab | PaletteItem::OpenHosts | PaletteItem::OpenSettings => {
                    language.text("ACTIONS", "快捷操作")
                }
            };
            if last_section != Some(section) {
                if last_section.is_some() {
                    list = list.child(div().h(px(6.)));
                }
                list = list.child(
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.muted))
                        .child(section),
                );
                last_section = Some(section);
            }

            let is_selected = ix == selected;
            let (icon, title, subtitle, badge) = match &item {
                PaletteItem::DirectSsh(host) => (
                    Icon::Play,
                    format!(
                        "{} {}",
                        language.text("Quick Connect:", "快速连接："),
                        host.endpoint()
                    ),
                    language
                        .text("Open a temporary SSH session", "建立临时 SSH 会话")
                        .to_owned(),
                    Some("SSH".to_owned()),
                ),
                PaletteItem::SavedHost(host) => (
                    host.icon.icon(),
                    host.name.clone(),
                    host.endpoint(),
                    (!host.group.is_empty()).then(|| host.group.clone()),
                ),
                PaletteItem::Snippet(snippet) => (
                    Icon::Code,
                    snippet.name.clone(),
                    snippet.command.lines().next().unwrap_or("").trim().to_owned(),
                    Some(language.text("Snippet", "片段").to_owned()),
                ),
                PaletteItem::NewLocalTab => (
                    Icon::Terminal,
                    language.text("New Local Terminal", "新建本地终端").to_owned(),
                    language
                        .text("Open a local shell tab", "打开本地 Shell 标签页")
                        .to_owned(),
                    Some("⌘T".to_owned()),
                ),
                PaletteItem::OpenHosts => (
                    Icon::Server,
                    language.text("Manage SSH Hosts", "管理 SSH 主机").to_owned(),
                    language
                        .text("Edit hosts, groups, and OpenSSH imports", "编辑主机、分组与导入配置")
                        .to_owned(),
                    None,
                ),
                PaletteItem::OpenSettings => (
                    Icon::Settings,
                    language.text("Open Settings", "打开设置").to_owned(),
                    language
                        .text("Appearance, themes, shell, and language", "外观主题、Shell 与界面语言")
                        .to_owned(),
                    Some("⌘,".to_owned()),
                ),
            };

            list = list.child(
                Button::new(("palette-item", ix))
                    .ghost()
                    .relative()
                    .w_full()
                    .h(px(46.))
                    .px(px(10.))
                    .rounded_lg()
                    .bg(if is_selected {
                        colors.accent_subtle()
                    } else {
                        transparent_black().into()
                    })
                    .border_1()
                    .border_color(if is_selected {
                        colors.accent_border()
                    } else {
                        transparent_black().into()
                    })
                    .justify_start()
                    .gap(px(10.))
                    .child(icon.badge(px(26.)))
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
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.muted))
                                    .child(subtitle),
                            ),
                    )
                    .children(badge.map(|badge| Tag::secondary().small().child(badge)))
                    .children(is_selected.then(|| {
                        div()
                            .px(px(5.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(colors.accent_soft())
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.accent))
                            .child("↵")
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.activate_item(item.clone(), cx);
                    })),
            );
        }

        let footer_hint = |key: &'static str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(
                    div()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .bg(rgb(colors.surface))
                        .border_1()
                        .border_color(colors.hairline())
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors.secondary))
                        .child(key),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(colors.muted))
                        .child(label),
                )
        };

        div()
            .id("quick-connect-backdrop")
            .absolute()
            .inset_0()
            .bg(colors.overlay_backdrop())
            .flex()
            .items_start()
            .justify_center()
            .pt(px(72.))
            .pb(px(32.))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(QuickConnectEvent::Close)))
            .child(
                div()
                    .id("quick-connect-card")
                    .w(px(580.))
                    .max_w(px(580.))
                    .max_h(px(480.))
                    .rounded_2xl()
                    .bg(colors.popover_bg())
                    .border_1()
                    .border_color(colors.hairline_strong())
                    .shadow_2xl()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&self.query)
                                        .appearance(false)
                                        .text_xs()
                                        .prefix(Icon::Search.render(px(16.), colors.accent))
                                        .cleanable(true),
                                ),
                            )
                            .children(Keystroke::parse("escape").ok().map(Kbd::new)),
                    )
                    .child(list)
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(8.))
                            .rounded_b(px(15.))
                            .bg(rgb(colors.chrome))
                            .border_t_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(footer_hint("↑↓", language.text("Navigate", "选择")))
                                    .child(footer_hint("↵", language.text("Open", "打开")))
                                    .child(footer_hint("Esc", language.text("Close", "关闭"))),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(colors.muted))
                                    .child(language.text(
                                        "Type user@host:port to connect",
                                        "支持输入 user@host:port 直连",
                                    )),
                            ),
                    ),
            )
    }
}

pub fn parse_quick_ssh(raw: &str) -> Option<HostProfile> {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_prefix("ssh ").map(str::trim).unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix("ssh://").unwrap_or(trimmed);
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return None;
    }

    let (username, endpoint) = match trimmed.split_once('@') {
        Some((user, rest)) if !user.is_empty() && !rest.is_empty() => (user.to_owned(), rest),
        Some(_) => return None,
        None => {
            // Require explicit '@' or a host with a dot/colon so casual words don't trigger Quick Connect.
            if !trimmed.contains('.') && !trimmed.contains(':') {
                return None;
            }
            let default_user = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".into());
            (default_user, trimmed)
        }
    };

    let (address, port) = if let Some(rest) = endpoint.strip_prefix('[') {
        let (addr, after) = rest.split_once(']')?;
        let port = if let Some(port_str) = after.strip_prefix(':') {
            port_str.parse::<u16>().ok().filter(|p| *p != 0)?
        } else if after.is_empty() {
            22
        } else {
            return None;
        };
        (addr.to_owned(), port)
    } else if endpoint.matches(':').count() == 1 {
        let (addr, port_str) = endpoint.rsplit_once(':')?;
        let port = port_str.parse::<u16>().ok().filter(|p| *p != 0)?;
        (addr.to_owned(), port)
    } else {
        (endpoint.to_owned(), 22)
    };

    if address.is_empty() || address.chars().any(char::is_control) {
        return None;
    }

    Some(HostProfile {
        id: 0,
        name: format!("{username}@{address}"),
        protocol: HostProtocol::Ssh,
        icon: HostIcon::Server,
        address,
        username,
        port,
        baud_rate: 115200,
        keychain_id: None,
        identity_file: String::new(),
        password: String::new(),
        jump_host: String::new(),
        jump_host_id: None,
        jump_relay_command: String::new(),
        jump_relay_keychain_id: None,
        jump_relay_password: String::new(),
        group: String::new(),
        tags: Vec::new(),
        last_connected: None,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_quick_ssh;

    #[test]
    fn parses_quick_ssh_targets() {
        let a = parse_quick_ssh("deploy@prod.example.com:2222").unwrap();
        assert_eq!(a.username, "deploy");
        assert_eq!(a.address, "prod.example.com");
        assert_eq!(a.port, 2222);

        let b = parse_quick_ssh("ssh ops@[2001:db8::1]:2200").unwrap();
        assert_eq!(b.username, "ops");
        assert_eq!(b.address, "2001:db8::1");
        assert_eq!(b.port, 2200);

        assert!(parse_quick_ssh("settings").is_none());
        assert!(parse_quick_ssh("root@host:0").is_none());
    }
}
