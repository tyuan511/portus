use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window,
    component::{
        Placement, Sizable as _,
        button::{Button, ButtonVariants as _},
    },
    div, px, rgb, transparent_black,
};

use crate::file_drawer::{FileDrawer, FileDrawerEvent};
use crate::icons::Icon;
use crate::platform::{apply_window_style, drag_window};
use crate::preferences::{
    HostsEvent, HostsPage, Preferences, SnippetsEvent, SnippetsPage, TunnelsPage,
};
use crate::quick_connect::{QuickConnectEvent, QuickConnectPalette};
use crate::settings::{
    DEFAULT_SIDEBAR_WIDTH, HostProfile, Language, SettingsStore, Snippet, clamp_sidebar_width,
    group_hosts,
};
use crate::sftp_view::SftpView;
use crate::terminal::TerminalSession;
use crate::theme::{Colors, Theme};
use crate::{
    CloseTab, CloseWindow, FindInTerminal, MinimizeWindow, NewTab, NextTab, PreviousTab,
    SelectTab, ShowHosts, ShowSettings, ShowSnippets, ShowTunnels, ToggleFilesDrawer,
    ToggleFullscreen, ToggleQuickConnect, ToggleSnippetsDrawer,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightDrawer {
    Files,
    Snippets,
    Tunnels,
}

enum TabKind {
    Terminal {
        terminal: Entity<TerminalSession>,
        _updates: Subscription,
    },
    Sftp {
        view: Entity<SftpView>,
        host_name: String,
        _updates: Subscription,
    },
    Hosts,
    Snippets,
    Tunnels,
    Settings,
}

struct Tab {
    id: u64,
    kind: TabKind,
}

pub struct Workspace {
    focus: FocusHandle,
    tabs: Vec<Tab>,
    active: usize,
    next_tab: u64,
    tab_scroll: ScrollHandle,
    settings: Entity<SettingsStore>,
    hosts: Entity<HostsPage>,
    snippets: Entity<SnippetsPage>,
    tunnels: Entity<TunnelsPage>,
    preferences: Entity<Preferences>,
    quick_connect: Entity<QuickConnectPalette>,
    file_drawer: Entity<FileDrawer>,
    quick_connect_open: bool,
    right_drawer: Option<RightDrawer>,
    sidebar_width: f32,
    resizing_sidebar: Option<(f32, f32)>,
    theme: Theme,
    font_family: String,
    font_size: u8,
    language: Language,
    opacity: u8,
    _settings_updates: Subscription,
    _hosts_events: Subscription,
    _snippets_events: Subscription,
    _quick_connect_events: Subscription,
    _file_drawer_events: Subscription,
    _window_bounds: Subscription,
}

impl Workspace {
    pub fn new(
        settings: Entity<SettingsStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = &settings.read(cx).current;
        let (theme, font_family, font_size, language, opacity, sidebar_width) = (
            saved.theme,
            saved.font_family.clone(),
            saved.font_size,
            saved.language,
            saved.opacity,
            clamp_sidebar_width(saved.sidebar_width),
        );
        window.defer(cx, move |window, _| apply_window_style(window, opacity));
        let window_bounds = cx.observe_window_bounds(window, |this, window, _| {
            apply_window_style(window, this.opacity);
        });
        let settings_updates = cx.observe_in(&settings, window, |this, settings, window, cx| {
            let saved = &settings.read(cx).current;
            let (theme, font_family, font_size, language, opacity, sidebar_width) = (
                saved.theme,
                saved.font_family.clone(),
                saved.font_size,
                saved.language,
                saved.opacity,
                clamp_sidebar_width(saved.sidebar_width),
            );
            if this.theme != theme
                || this.language != language
                || this.font_family != font_family
                || this.font_size != font_size
            {
                this.theme = theme;
                this.font_family = font_family.clone();
                this.font_size = font_size;
                this.language = language;
                let palette = theme.palette();
                let colors = theme.colors();
                let terminals: Vec<_> = this
                    .tabs
                    .iter()
                    .filter_map(|tab| match &tab.kind {
                        TabKind::Terminal { terminal, .. } => Some(terminal.clone()),
                        _ => None,
                    })
                    .collect();
                for terminal in terminals {
                    let ff = font_family.clone();
                    terminal.update(cx, |terminal, cx| {
                        terminal.set_appearance(colors, palette, ff, font_size, language, cx);
                    });
                }
            }
            if this.opacity != opacity {
                this.opacity = opacity;
                apply_window_style(window, opacity);
            }
            if this.resizing_sidebar.is_none() {
                this.sidebar_width = sidebar_width;
            }
            cx.notify();
        });
        let hosts = cx.new(|cx| HostsPage::new(settings.clone(), window, cx));
        let hosts_events = cx.subscribe_in(&hosts, window, |this, _, event, window, cx| {
            match event {
                HostsEvent::Connect(host) => this.connect_host(host.clone(), window, cx),
            }
        });
        let snippets = cx.new(|cx| SnippetsPage::new(settings.clone(), window, cx));
        let snippets_events = cx.subscribe_in(&snippets, window, |this, _, event, window, cx| {
            match event {
                SnippetsEvent::Run(snippet) => this.run_snippet(snippet, window, cx),
            }
        });
        let tunnels = cx.new(|cx| TunnelsPage::new(settings.clone(), window, cx));
        let preferences = cx.new(|cx| Preferences::new(settings.clone(), window, cx));
        let quick_connect = cx.new(|cx| QuickConnectPalette::new(settings.clone(), window, cx));
        let file_drawer = cx.new(|cx| FileDrawer::new(settings.clone(), window, cx));
        let file_drawer_events =
            cx.subscribe_in(&file_drawer, window, |this, _, event, window, cx| {
                match event {
                    FileDrawerEvent::InsertPath(path) => {
                        if let Some(Tab {
                            kind: TabKind::Terminal { terminal, .. },
                            ..
                        }) = this.tabs.get(this.active)
                        {
                            let escaped = shell_words::quote(&path).into_owned();
                            let bytes = format!("{escaped} ").into_bytes();
                            terminal.update(cx, |session, cx| {
                                session.send_bytes(&bytes, cx);
                                session.focus_handle().focus(window, cx);
                            });
                        }
                    }
                    FileDrawerEvent::OpenFullSftp(host) => {
                        this.open_sftp(host.clone(), window, cx);
                    }
                }
            });
        let quick_connect_events =
            cx.subscribe_in(&quick_connect, window, |this, _, event, window, cx| {
                this.quick_connect_open = false;
                match event {
                    QuickConnectEvent::ConnectHost(host) => {
                        this.connect_host(host.clone(), window, cx);
                    }
                    QuickConnectEvent::RunSnippet(snippet) => {
                        this.run_snippet(snippet, window, cx);
                    }
                    QuickConnectEvent::NewLocalTab => this.new_tab(window, cx),
                    QuickConnectEvent::OpenHosts => this.open_hosts(None, window, cx),
                    QuickConnectEvent::OpenSettings => this.open_settings(window, cx),
                    QuickConnectEvent::Close => this.restore_active_focus(window, cx),
                }
                cx.notify();
            });
        let mut this = Self {
            focus: cx.focus_handle(),
            tabs: Vec::new(),
            active: 0,
            next_tab: 1,
            tab_scroll: ScrollHandle::new(),
            settings,
            hosts,
            snippets,
            tunnels,
            preferences,
            quick_connect,
            file_drawer,
            quick_connect_open: false,
            right_drawer: None,
            sidebar_width,
            resizing_sidebar: None,
            theme,
            font_family,
            font_size,
            language,
            opacity,
            _settings_updates: settings_updates,
            _hosts_events: hosts_events,
            _snippets_events: snippets_events,
            _quick_connect_events: quick_connect_events,
            _file_drawer_events: file_drawer_events,
            _window_bounds: window_bounds,
        };
        this.new_tab(window, cx);
        this
    }

    fn alloc_tab_id(&mut self) -> u64 {
        let id = self.next_tab;
        self.next_tab += 1;
        id
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let colors = self.theme.colors();
        let palette = self.theme.palette();
        let shell = self.settings.read(cx).current.shell.clone();
        let terminal = cx.new(|cx| {
            TerminalSession::local(
                colors,
                palette,
                self.font_family.clone(),
                self.font_size,
                self.language,
                &shell,
                cx,
            )
        });
        let updates = cx.observe(&terminal, |_, _, cx| cx.notify());
        let id = self.alloc_tab_id();
        self.tabs.push(Tab {
            id,
            kind: TabKind::Terminal {
                terminal,
                _updates: updates,
            },
        });
        self.select_tab(self.tabs.len() - 1, window, cx);
    }

    pub fn connect_host(&mut self, host: HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        let host_id = host.id;
        if host_id != 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .ok();
            if let Some(ts) = now {
                self.settings.update(cx, |store, cx| {
                    let _ = store.update(|settings| {
                        if let Some(saved) = settings.hosts.iter_mut().find(|h| h.id == host_id) {
                            saved.last_connected = Some(ts);
                        }
                    });
                    cx.notify();
                });
            }
        }
        let colors = self.theme.colors();
        let palette = self.theme.palette();
        let all_hosts = self.settings.read(cx).current.hosts.clone();
        let terminal = cx.new(|cx| {
            TerminalSession::ssh(
                colors,
                palette,
                self.font_family.clone(),
                self.font_size,
                self.language,
                host,
                all_hosts,
                window,
                cx,
            )
        });
        let updates = cx.observe(&terminal, |_, _, cx| cx.notify());
        let id = self.alloc_tab_id();
        self.tabs.push(Tab {
            id,
            kind: TabKind::Terminal {
                terminal,
                _updates: updates,
            },
        });
        self.select_tab(self.tabs.len() - 1, window, cx);
    }

    fn toggle_quick_connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quick_connect_open = !self.quick_connect_open;
        if self.quick_connect_open {
            self.quick_connect
                .update(cx, |palette, cx| palette.focus_input(window, cx));
        } else {
            self.restore_active_focus(window, cx);
        }
        cx.notify();
    }

    fn restore_active_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            match &tab.kind {
                TabKind::Terminal { terminal, .. } => {
                    terminal.read(cx).focus_handle().focus(window, cx);
                }
                TabKind::Sftp { .. }
                | TabKind::Hosts
                | TabKind::Snippets
                | TabKind::Tunnels
                | TabKind::Settings => {
                    self.focus.focus(window, cx);
                }
            }
        }
    }

    fn toggle_sidebar_group(&mut self, group: String, cx: &mut Context<Self>) {
        self.settings.update(cx, |store, cx| {
            let _ = store.update(|settings| {
                if let Some(pos) = settings
                    .collapsed_groups
                    .iter()
                    .position(|existing| existing == &group)
                {
                    settings.collapsed_groups.remove(pos);
                } else {
                    settings.collapsed_groups.push(group);
                }
            });
            cx.notify();
        });
    }

    fn start_sidebar_resize(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if event.click_count >= 2 {
            self.resizing_sidebar = None;
            self.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
            self.save_sidebar_width(cx);
            cx.notify();
            return;
        }
        self.resizing_sidebar = Some((f32::from(event.position.x), self.sidebar_width));
        cx.notify();
    }

    fn update_sidebar_resize(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start_x, start_width)) = self.resizing_sidebar else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.finish_sidebar_resize(cx);
            return;
        }
        let next = clamp_sidebar_width(start_width + (f32::from(event.position.x) - start_x));
        if (next - self.sidebar_width).abs() >= f32::EPSILON {
            self.sidebar_width = next;
            cx.notify();
        }
    }

    fn finish_sidebar_resize(&mut self, cx: &mut Context<Self>) {
        if self.resizing_sidebar.take().is_some() {
            self.save_sidebar_width(cx);
            cx.notify();
        }
    }

    fn save_sidebar_width(&mut self, cx: &mut Context<Self>) {
        let width = clamp_sidebar_width(self.sidebar_width);
        self.sidebar_width = width;
        self.settings.update(cx, |store, cx| {
            if (store.current.sidebar_width - width).abs() >= f32::EPSILON {
                let _ = store.update(|settings| settings.sidebar_width = width);
                cx.notify();
            }
        });
    }

    pub fn open_sftp(&mut self, host: HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        let host_name = host.name.clone();
        let store = self.settings.clone();
        let view = cx.new(|cx| SftpView::new(store, host, window, cx));
        let updates = cx.observe(&view, |_, _, cx| cx.notify());
        let id = self.alloc_tab_id();
        self.tabs.push(Tab {
            id,
            kind: TabKind::Sftp {
                view,
                host_name,
                _updates: updates,
            },
        });
        self.select_tab(self.tabs.len() - 1, window, cx);
    }

    pub fn open_hosts(&mut self, host: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let index = if let Some(existing) = self
            .tabs
            .iter()
            .position(|tab| matches!(tab.kind, TabKind::Hosts))
        {
            existing
        } else {
            let id = self.alloc_tab_id();
            self.tabs.push(Tab {
                id,
                kind: TabKind::Hosts,
            });
            self.tabs.len() - 1
        };
        self.select_tab(index, window, cx);
        if let Some(id) = host {
            self.hosts
                .update(cx, |hosts, cx| hosts.edit_host(Some(id), window, cx));
        }
    }

    pub fn open_snippets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = if let Some(existing) = self
            .tabs
            .iter()
            .position(|tab| matches!(tab.kind, TabKind::Snippets))
        {
            existing
        } else {
            let id = self.alloc_tab_id();
            self.tabs.push(Tab {
                id,
                kind: TabKind::Snippets,
            });
            self.tabs.len() - 1
        };
        self.select_tab(index, window, cx);
    }

    pub fn run_snippet(&mut self, snippet: &Snippet, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_or_new_terminal(window, cx);
        if let Some(Tab {
            kind: TabKind::Terminal { terminal, .. },
            ..
        }) = self.tabs.get(self.active)
        {
            let payload = snippet.payload();
            terminal.update(cx, |session, cx| {
                session.send_bytes(&payload, cx);
                session.focus_handle().focus(window, cx);
            });
        }
    }

    pub fn open_tunnels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = if let Some(existing) = self
            .tabs
            .iter()
            .position(|tab| matches!(tab.kind, TabKind::Tunnels))
        {
            existing
        } else {
            let id = self.alloc_tab_id();
            self.tabs.push(Tab {
                id,
                kind: TabKind::Tunnels,
            });
            self.tabs.len() - 1
        };
        self.select_tab(index, window, cx);
    }

    pub fn export_active_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Tab {
            kind: TabKind::Terminal { terminal, .. },
            ..
        }) = self.tabs.get(self.active)
        else {
            return;
        };
        let bytes = terminal.read(cx).export_log();
        let language = self.language;
        let default_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&default_dir, Some("bulwark-session.log"));
        let window_handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let res = std::fs::write(&path, bytes);
                let _ = window_handle.update(cx, |_, window, cx| match res {
                    Ok(()) => crate::notify::success(
                        language.text("Session log exported.", "会话日志已导出。"),
                        window,
                        cx,
                    ),
                    Err(err) => crate::notify::error(err.to_string(), window, cx),
                });
            }
        })
        .detach();
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = if let Some(existing) = self
            .tabs
            .iter()
            .position(|tab| matches!(tab.kind, TabKind::Settings))
        {
            existing
        } else {
            let id = self.alloc_tab_id();
            self.tabs.push(Tab {
                id,
                kind: TabKind::Settings,
            });
            self.tabs.len() - 1
        };
        self.select_tab(index, window, cx);
    }

    fn sync_file_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.right_drawer != Some(RightDrawer::Files) {
            return;
        }
        let host = if let Some(Tab {
            kind: TabKind::Terminal { terminal, .. },
            ..
        }) = self.tabs.get(self.active)
        {
            terminal.read(cx).host_profile().cloned()
        } else {
            None
        };
        self.file_drawer
            .update(cx, |drawer, cx| drawer.sync_target(host.as_ref(), window, cx));
    }

    fn toggle_right_drawer(
        &mut self,
        drawer: RightDrawer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.right_drawer == Some(drawer) {
            self.right_drawer = None;
        } else {
            self.right_drawer = Some(drawer);
            if drawer == RightDrawer::Files {
                self.sync_file_drawer(window, cx);
            }
        }
        cx.notify();
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        if self.active != index
            && let Some(Tab {
                kind: TabKind::Terminal { terminal, .. },
                ..
            }) = self.tabs.get(self.active)
        {
            terminal.update(cx, TerminalSession::clear_composition);
        }
        self.active = index;
        match &self.tabs[index].kind {
            TabKind::Terminal { terminal, .. } => {
                if self.right_drawer == Some(RightDrawer::Tunnels)
                    && !terminal.read(cx).is_ssh()
                {
                    self.right_drawer = None;
                }
                terminal.read(cx).focus_handle().focus(window, cx);
            }
            TabKind::Sftp { .. }
            | TabKind::Hosts
            | TabKind::Snippets
            | TabKind::Tunnels
            | TabKind::Settings => {
                self.focus.focus(window, cx);
            }
        }
        self.tab_scroll.scroll_to_item(index);
        self.sync_file_drawer(window, cx);
        cx.notify();
    }

    fn focus_or_new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.tabs.get(self.active).map(|t| &t.kind),
            Some(TabKind::Terminal { .. })
        ) {
            self.select_tab(self.active, window, cx);
            return;
        }
        if let Some(index) = self
            .tabs
            .iter()
            .rposition(|tab| matches!(tab.kind, TabKind::Terminal { .. }))
        {
            self.select_tab(index, window, cx);
        } else {
            self.new_tab(window, cx);
        }
    }

    fn step_tab(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len();
        if count != 0 {
            let offset = if backwards { count - 1 } else { 1 };
            self.select_tab((self.active + offset) % count, window, cx);
        }
    }

    fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            let id = tab.id;
            self.close_tab(id, window, cx);
        } else {
            self.tunnels.update(cx, |tunnels, _| tunnels.shutdown_all());
            window.remove_window();
        }
    }

    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            match &tab.kind {
                TabKind::Terminal { terminal, .. } => {
                    terminal.update(cx, |terminal, cx| terminal.shutdown(cx));
                }
                TabKind::Sftp { view, .. } => {
                    view.update(cx, |sftp, _| sftp.shutdown());
                }
                _ => {}
            }
        }
        self.tunnels.update(cx, |tunnels, _| tunnels.shutdown_all());
        self.file_drawer.update(cx, |drawer, _| drawer.shutdown_sftp());
        window.remove_window();
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = self.tabs.remove(index);
        let was_terminal = matches!(tab.kind, TabKind::Terminal { .. });
        match tab.kind {
            TabKind::Terminal { terminal, .. } => {
                terminal.update(cx, |terminal, cx| terminal.shutdown(cx));
            }
            TabKind::Sftp { view, .. } => {
                view.update(cx, |sftp, _| sftp.shutdown());
            }
            _ => {}
        }
        if self.tabs.is_empty() {
            self.active = 0;
            if was_terminal {
                self.open_hosts(None, window, cx);
            } else {
                self.tunnels.update(cx, |tunnels, _| tunnels.shutdown_all());
        self.file_drawer.update(cx, |drawer, _| drawer.shutdown_sftp());
        window.remove_window();
            }
            return;
        }
        let next_active = active_after_close(self.active, index, self.tabs.len());
        self.select_tab(next_active, window, cx);
    }

    fn sidebar(&self, colors: Colors, cx: &Context<Self>) -> impl IntoElement {
        let language = self.language;
        let active_kind = self.tabs.get(self.active).map(|tab| &tab.kind);
        let terminal_active = matches!(active_kind, Some(TabKind::Terminal { .. }));
        let hosts_active = matches!(active_kind, Some(TabKind::Hosts));
        let snippets_active = matches!(active_kind, Some(TabKind::Snippets));
        let tunnels_active = matches!(active_kind, Some(TabKind::Tunnels));
        let settings_active = matches!(active_kind, Some(TabKind::Settings));

        let terminal_count = self
            .tabs
            .iter()
            .filter(|tab| matches!(tab.kind, TabKind::Terminal { .. }))
            .count();

        let connected_host_ids: Vec<u64> = self
            .tabs
            .iter()
            .filter_map(|tab| match &tab.kind {
                TabKind::Terminal { terminal, .. } => {
                    let s = terminal.read(cx);
                    if s.is_connected() {
                        s.host_profile().map(|h| h.id)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        let saved = &self.settings.read(cx).current;
        let host_count = saved.hosts.len();
        let snippet_count = saved.snippets.len();
        let tunnel_count = saved.tunnels.len();
        let collapsed_groups = &saved.collapsed_groups;
        let grouped_hosts = group_hosts(saved.hosts.iter());

        let brand = div()
            .flex()
            .h(px(44.))
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .px(px(12.))
            .border_b_1()
            .border_color(colors.hairline())
            .on_mouse_down(MouseButton::Left, drag_window)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(colors.accent_soft())
                            .border_1()
                            .border_color(colors.accent_border())
                            .child(Icon::Shield.render(px(13.), colors.accent)),
                    )
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child("Bulwark"),
                    ),
            )
            .child(
                Button::new("sidebar-new-tab")
                    .ghost()
                    .xsmall()
                    .size(px(24.))
                    .rounded_md()
                    .tooltip(language.text("New Local Tab (⌘T)", "新建本地标签 (⌘T)"))
                    .icon(Icon::Plus)
                    .on_click(cx.listener(|this, _, window, cx| this.new_tab(window, cx))),
            );

        let quick_trigger = div().px(px(10.)).pt(px(10.)).pb(px(4.)).child(
            Button::new("sidebar-quick-connect")
                .ghost()
                .w_full()
                .h(px(30.))
                .px(px(9.))
                .rounded_lg()
                .bg(rgb(colors.surface))
                .border_1()
                .border_color(colors.hairline())
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .min_w_0()
                        .child(Icon::Search.render(px(12.5), colors.muted))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(colors.muted))
                                .truncate()
                                .child(language.text("Quick connect…", "快速连接…")),
                        ),
                )
                .child(
                    div()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .bg(rgb(colors.panel))
                        .border_1()
                        .border_color(colors.hairline())
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors.muted))
                        .child("⌘K"),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_quick_connect(window, cx);
                })),
        );

        let mut sidebar = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(self.sidebar_width))
            .bg(rgb(colors.chrome))
            .border_r_1()
            .border_color(colors.hairline())
            .child(brand)
            .child(quick_trigger)
            .child(
                div()
                    .px(px(10.))
                    .pt(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        nav_button(
                            "nav-terminal",
                            Icon::Terminal,
                            language.text("Terminal", "终端"),
                            (terminal_count > 0).then(|| terminal_count.to_string()),
                            terminal_active,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.focus_or_new_terminal(window, cx);
                        })),
                    )
                    .child(
                        nav_button(
                            "nav-hosts",
                            Icon::Server,
                            language.text("SSH Hosts", "SSH 主机"),
                            (host_count > 0).then(|| host_count.to_string()),
                            hosts_active,
                            colors,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_hosts(None, window, cx)),
                        ),
                    )
                    .child(
                        nav_button(
                            "nav-snippets",
                            Icon::Code,
                            language.text("Snippets", "命令片段"),
                            (snippet_count > 0).then(|| snippet_count.to_string()),
                            snippets_active,
                            colors,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_snippets(window, cx)),
                        ),
                    )
                    .child(
                        nav_button(
                            "nav-tunnels",
                            Icon::Network,
                            language.text("Port Forwarding", "端口转发"),
                            (tunnel_count > 0).then(|| tunnel_count.to_string()),
                            tunnels_active,
                            colors,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_tunnels(window, cx)),
                        ),
                    ),
            );

        if host_count > 0 {
            sidebar = sidebar.child(
                div()
                    .mt(px(14.))
                    .mb(px(4.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.muted))
                            .child(language.text("HOSTS", "常用主机")),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(colors.muted))
                            .child(host_count.to_string()),
                    ),
            );
        }

        let mut quick_list = div()
            .id("quick-hosts")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .gap(px(1.5));

        for (group_ix, (group_name, items)) in grouped_hosts.into_iter().enumerate() {
            let is_collapsed =
                !group_name.is_empty() && collapsed_groups.iter().any(|g| g == &group_name);
            if !group_name.is_empty() {
                let toggle_name = group_name.clone();
                let chevron = if is_collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                };
                quick_list = quick_list.child(
                    Button::new(("sidebar-group", group_ix))
                        .ghost()
                        .w_full()
                        .h(px(24.))
                        .px(px(6.))
                        .mt(px(4.))
                        .rounded_md()
                        .justify_start()
                        .gap(px(5.))
                        .child(chevron.render(px(11.), colors.muted))
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(colors.muted))
                                .truncate()
                                .child(group_name),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_sidebar_group(toggle_name.clone(), cx);
                        })),
                );
            }
            if is_collapsed {
                continue;
            }
            quick_list = quick_list.children(items.into_iter().cloned().map(|host| {
                let id = host.id;
                let is_live = connected_host_ids.contains(&id);
                let name = host.name.clone();
                let endpoint = host.endpoint();
                let group = format!("quick-host-group-{id}");
                let connect_host = host.clone();
                div()
                    .group(group.clone())
                    .relative()
                    .w_full()
                    .child(
                        Button::new(("quick-host", id))
                            .ghost()
                            .w_full()
                            .h(px(36.))
                            .rounded_lg()
                            .justify_start()
                            .pl(px(8.))
                            .pr(px(28.))
                            .gap(px(8.))
                            .text_color(rgb(colors.secondary))
                            .child(if is_live {
                                div()
                                    .size(px(18.))
                                    .rounded_md()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(colors.success_soft())
                                    .child(
                                        div()
                                            .size(px(6.))
                                            .rounded_full()
                                            .bg(rgb(colors.success)),
                                    )
                            } else {
                                div()
                                    .size(px(18.))
                                    .rounded_md()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(rgb(colors.surface))
                                    .child(host.icon.icon().render(px(11.), colors.muted))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(0.5))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(if is_live {
                                                FontWeight::MEDIUM
                                            } else {
                                                FontWeight::NORMAL
                                            })
                                            .text_color(rgb(if is_live {
                                                colors.foreground
                                            } else {
                                                colors.secondary
                                            }))
                                            .truncate()
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(rgb(colors.muted))
                                            .truncate()
                                            .child(endpoint),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.connect_host(connect_host.clone(), window, cx)
                            })),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(px(5.))
                            .flex()
                            .items_center()
                            .child(
                                Button::new(("quick-host-edit", id))
                                    .ghost()
                                    .xsmall()
                                    .opacity(0.0)
                                    .group_hover(group, |style| style.opacity(1.0))
                                    .icon(Icon::Edit)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_hosts(Some(id), window, cx)
                                    })),
                            ),
                    )
            }));
        }

        sidebar
            .child(quick_list)
            .child(
                div()
                    .p(px(8.))
                    .border_t_1()
                    .border_color(colors.hairline())
                    .child(
                        nav_button(
                            "nav-settings",
                            Icon::Settings,
                            language.text("Settings", "设置"),
                            None,
                            settings_active,
                            colors,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                        ),
                    ),
            )
    }

    fn sidebar_resize_handle(&self, colors: Colors, cx: &Context<Self>) -> impl IntoElement {
        let resizing = self.resizing_sidebar.is_some();
        div()
            .id("sidebar-resize-handle")
            .group("sidebar-resize")
            .w(px(6.))
            .ml(px(-3.))
            .mr(px(-3.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .justify_center()
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.start_sidebar_resize(event, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .w(px(2.))
                    .h_full()
                    .bg(if resizing {
                        rgb(colors.accent).into()
                    } else {
                        transparent_black()
                    })
                    .group_hover("sidebar-resize", move |style| style.bg(rgb(colors.accent))),
            )
    }

    fn snippets_drawer(&self, colors: Colors, cx: &Context<Self>) -> impl IntoElement {
        let language = self.language;
        let snippets = &self.settings.read(cx).current.snippets;
        let mut sorted: Vec<_> = snippets.iter().cloned().collect();
        sorted.sort_by_cached_key(|s| (s.group.to_lowercase(), s.name.to_lowercase()));

        div()
            .w(px(264.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(colors.panel))
            .border_l_1()
            .border_color(colors.hairline())
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(44.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(Icon::Code.render(px(13.5), colors.accent))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .child(language.text("Snippets", "命令片段")),
                            ),
                    )
                    .child(
                        Button::new("drawer-manage-snippets")
                            .outline()
                            .xsmall()
                            .label(language.text("Manage", "管理"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_snippets(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("snippets-drawer-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .children(if sorted.is_empty() {
                        vec![
                            div()
                                .p(px(20.))
                                .text_center()
                                .text_size(px(12.))
                                .text_color(rgb(colors.muted))
                                .child(language.text(
                                    "No snippets yet. Click Manage to create one.",
                                    "暂无片段，点击上方「管理」创建。",
                                ))
                                .into_any_element(),
                        ]
                    } else {
                        sorted
                            .into_iter()
                            .map(|snippet| {
                                let preview = snippet
                                    .command
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_owned();
                                let run_item = snippet.clone();
                                Button::new(("drawer-snippet", snippet.id))
                                    .ghost()
                                    .w_full()
                                    .px(px(10.))
                                    .py(px(8.))
                                    .rounded_lg()
                                    .bg(rgb(colors.background))
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(snippet.name),
                                            )
                                            .child(
                                                div()
                                                    .size(px(20.))
                                                    .rounded_md()
                                                    .bg(colors.accent_soft())
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(Icon::Play.render(px(10.), colors.accent)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .px(px(6.))
                                            .py(px(3.))
                                            .rounded(px(4.))
                                            .bg(rgb(colors.surface))
                                            .truncate()
                                            .text_size(px(11.))
                                            .text_color(rgb(colors.secondary))
                                            .child(preview),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_snippet(&run_item, window, cx);
                                    }))
                                    .into_any_element()
                            })
                            .collect()
                    }),
            )
    }

    fn tunnels_drawer(
        &self,
        host: &HostProfile,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = self.language;
        let host_id = host.id;
        let host_name = host.name.clone();
        let body = self
            .tunnels
            .update(cx, |tunnels, cx| tunnels.render_host_drawer_body(host, colors, language, cx));

        div()
            .w(px(280.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(colors.panel))
            .border_l_1()
            .border_color(colors.hairline())
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(44.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .truncate()
                                    .child(language.text("Port Forwarding", "端口转发")),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(colors.muted))
                                    .truncate()
                                    .child(host_name),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                Button::new("drawer-new-tunnel")
                                    .xsmall()
                                    .h(px(24.))
                                    .px(px(8.))
                                    .rounded_md()
                                    .bg(colors.accent_soft())
                                    .border_1()
                                    .border_color(colors.accent_border())
                                    .text_color(rgb(colors.accent))
                                    .cursor_pointer()
                                    .child(language.text("New", "新建"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.tunnels.update(cx, |tunnels, cx| {
                                            tunnels.edit_rule_for_host(None, host_id, window, cx)
                                        });
                                    })),
                            )
                            .child(
                                Button::new("drawer-manage-tunnels")
                                    .outline()
                                    .xsmall()
                                    .label(language.text("Manage", "管理"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_tunnels(window, cx)
                                    })),
                            ),
                    ),
            )
            .child(body)
    }

    fn right_rail(
        &self,
        host: Option<&HostProfile>,
        colors: Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let language = self.language;
        let is_ssh = host.is_some();
        let files_active = self.right_drawer == Some(RightDrawer::Files);
        let snippets_active = self.right_drawer == Some(RightDrawer::Snippets);
        let tunnels_active = self.right_drawer == Some(RightDrawer::Tunnels);
        let active_tunnels = host
            .map(|h| self.tunnels.read(cx).active_count_for_host(h.id, cx))
            .unwrap_or(0);

        let make_rail_item = |active: bool, btn: Button| {
            div()
                .relative()
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .children(active.then(|| {
                    div()
                        .absolute()
                        .right_0()
                        .top(px(7.))
                        .bottom(px(7.))
                        .w(px(2.5))
                        .rounded_l_full()
                        .bg(rgb(colors.accent))
                }))
                .child(btn)
        };

        let build_btn = |id: &'static str,
                         icon: Icon,
                         tooltip: &'static str,
                         active: bool,
                         badge: Option<usize>| {
            let fg = if active {
                colors.accent
            } else {
                colors.muted
            };
            Button::new(id)
                .ghost()
                .relative()
                .size(px(32.))
                .rounded_lg()
                .flex()
                .items_center()
                .justify_center()
                .bg(if active {
                    colors.accent_subtle()
                } else {
                    transparent_black().into()
                })
                .text_color(rgb(fg))
                .tooltip(tooltip)
                .tooltip_placement(Placement::Left)
                .child(icon.render(px(15.), fg))
                .children(badge.filter(|&n| n > 0).map(|n| {
                    div()
                        .absolute()
                        .top(px(2.))
                        .right(px(2.))
                        .px(px(4.))
                        .rounded_full()
                        .bg(rgb(colors.success))
                        .text_size(px(9.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.background))
                        .child(n.to_string())
                }))
        };

        let mut rail = div()
            .w(px(42.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(colors.chrome))
            .border_l_1()
            .border_color(colors.hairline())
            .py(px(8.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .child(make_rail_item(
                files_active,
                build_btn(
                    "rail-files-drawer",
                    Icon::Folder,
                    if is_ssh {
                        language.text("SFTP File Browser (⌘⇧E)", "SFTP 文件传输 (⌘⇧E)")
                    } else {
                        language.text("Files Drawer (⌘⇧E)", "文件目录抽屉 (⌘⇧E)")
                    },
                    files_active,
                    None,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_right_drawer(RightDrawer::Files, window, cx);
                })),
            ))
            .child(make_rail_item(
                snippets_active,
                build_btn(
                    "rail-snippets-drawer",
                    Icon::Code,
                    language.text("Snippets Drawer (⌘⇧S)", "命令片段抽屉 (⌘⇧S)"),
                    snippets_active,
                    None,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_right_drawer(RightDrawer::Snippets, window, cx);
                })),
            ));

        if is_ssh {
            rail = rail.child(make_rail_item(
                tunnels_active,
                build_btn(
                    "rail-tunnels-drawer",
                    Icon::Network,
                    language.text("Port Forwarding", "端口转发"),
                    tunnels_active,
                    Some(active_tunnels),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_right_drawer(RightDrawer::Tunnels, window, cx);
                })),
            ));
        }

        rail.child(
            div()
                .w(px(20.))
                .h(px(1.))
                .my(px(4.))
                .bg(colors.hairline()),
        )
        .child(make_rail_item(
            false,
            build_btn(
                "rail-export-log",
                Icon::Import,
                language.text("Download Session Log", "导出会话日志"),
                false,
                None,
            )
            .on_click(cx.listener(|this, _, window, cx| this.export_active_log(window, cx))),
        ))
    }

    fn tab_bar(&self, colors: Colors, cx: &Context<Self>) -> impl IntoElement {
        let language = self.language;
        let is_terminal = matches!(
            self.tabs.get(self.active).map(|t| &t.kind),
            Some(TabKind::Terminal { .. })
        );
        let drag = div()
            .flex_1()
            .min_w(px(24.))
            .h_full()
            .on_mouse_down(MouseButton::Left, drag_window);

        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .h(px(44.))
            .flex_shrink_0()
            .bg(rgb(colors.chrome))
            .border_b_1()
            .border_color(colors.hairline())
            .child(
                div()
                    .id("workspace-tabs")
                    .flex()
                    .items_center()
                    .max_w_full()
                    .min_w_0()
                    .h(px(30.))
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    .gap(px(4.))
                    .children(
                        self.tabs
                            .iter()
                            .enumerate()
                            .map(|(index, tab)| self.tab_item(index, tab, colors, cx)),
                    ),
            )
            .child(
                Button::new("new-tab")
                    .ghost()
                    .xsmall()
                    .size(px(26.))
                    .rounded_md()
                    .flex_shrink_0()
                    .tooltip(language.text("New Local Tab (⌘T)", "新建本地标签 (⌘T)"))
                    .icon(Icon::Plus)
                    .on_click(cx.listener(|this, _, window, cx| this.new_tab(window, cx))),
            )
            .child(drag)
            .children(is_terminal.then(|| {
                Button::new("tabbar-find")
                    .ghost()
                    .xsmall()
                    .size(px(26.))
                    .rounded_md()
                    .flex_shrink_0()
                    .tooltip(language.text("Find in Terminal (⌘F)", "终端内搜索 (⌘F)"))
                    .icon(Icon::Search)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(Tab {
                            kind: TabKind::Terminal { terminal, .. },
                            ..
                        }) = this.tabs.get(this.active)
                        {
                            terminal.update(cx, |terminal, cx| terminal.toggle_search(window, cx));
                        }
                    }))
            }))
    }

    fn tab_item(
        &self,
        index: usize,
        tab: &Tab,
        colors: Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let id = tab.id;
        let active = index == self.active;
        let (icon, icon_color, status_dot, title): (Icon, u32, Option<u32>, String) = match &tab.kind {
            TabKind::Terminal { terminal, .. } => {
                let session = terminal.read(cx);
                let raw = session.title();
                let title = if raw.is_empty() {
                    self.language.text("Local", "本地")
                } else {
                    raw
                };
                if session.is_ssh() {
                    let dot = if session.is_connected() {
                        Some(colors.success)
                    } else {
                        Some(colors.accent)
                    };
                    let icon = session
                        .host_profile()
                        .map(|h| h.icon.icon())
                        .unwrap_or(Icon::Server);
                    let color = if active {
                        colors.foreground
                    } else {
                        colors.muted
                    };
                    (icon, color, dot, title.to_owned())
                } else {
                    (
                        Icon::Terminal,
                        if active { colors.accent } else { colors.muted },
                        None,
                        title.to_owned(),
                    )
                }
            }
            TabKind::Sftp { host_name, .. } => (
                Icon::Folder,
                if active { colors.accent } else { colors.muted },
                None,
                format!("SFTP · {host_name}"),
            ),
            TabKind::Hosts => (
                Icon::Server,
                if active { colors.accent } else { colors.muted },
                None,
                self.language.text("SSH Hosts", "SSH 主机").to_owned(),
            ),
            TabKind::Snippets => (
                Icon::Code,
                if active { colors.accent } else { colors.muted },
                None,
                self.language.text("Snippets", "命令片段").to_owned(),
            ),
            TabKind::Tunnels => (
                Icon::Network,
                if active { colors.accent } else { colors.muted },
                None,
                self.language.text("Port Forwarding", "端口转发").to_owned(),
            ),
            TabKind::Settings => (
                Icon::Settings,
                if active { colors.accent } else { colors.muted },
                None,
                self.language.text("Settings", "设置").to_owned(),
            ),
        };
        let fg_color = if active {
            colors.foreground
        } else {
            colors.muted
        };
        let group = format!("workspace-tab-{id}");
        let tab_width = match &tab.kind {
            TabKind::Terminal { .. } | TabKind::Sftp { .. } => px(168.),
            TabKind::Hosts | TabKind::Snippets | TabKind::Tunnels | TabKind::Settings => px(136.),
        };

        div()
            .group(group.clone())
            .relative()
            .flex_shrink_0()
            .w(tab_width)
            .h_full()
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| this.close_tab(id, window, cx)),
            )
            .child(
                Button::new(("tab", id))
                    .ghost()
                    .size_full()
                    .rounded_lg()
                    .border_1()
                    .border_color(if active {
                        colors.hairline_strong()
                    } else {
                        transparent_black().into()
                    })
                    .bg(if active {
                        rgb(colors.background).into()
                    } else {
                        transparent_black()
                    })
                    .text_color(rgb(fg_color))
                    .font_weight(if active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .pl(px(9.))
                            .pr(px(26.))
                            .children(status_dot.map(|dot| {
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(rgb(dot))
                            }))
                            .child(icon.render(px(12.5), icon_color))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(12.))
                                    .child(title.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(index) = this.tabs.iter().position(|tab| tab.id == id) {
                            this.select_tab(index, window, cx);
                        }
                    })),
            )
            .children(active.then(|| {
                div()
                    .absolute()
                    .top_0()
                    .left(px(14.))
                    .right(px(14.))
                    .h(px(2.))
                    .rounded_b_full()
                    .bg(rgb(colors.accent))
            }))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(4.))
                    .flex()
                    .items_center()
                    .child(
                        Button::new(("close-tab", id))
                            .ghost()
                            .xsmall()
                            .opacity(if active { 0.7 } else { 0.0 })
                            .group_hover(group, |style| style.opacity(1.0))
                            .icon(Icon::Close)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab(id, window, cx)
                            })),
                    ),
            )
    }

    fn status_bar(&self, colors: Colors, cx: &Context<Self>) -> impl IntoElement {
        let language = self.language;
        let (dot_color, primary_label, secondary_label, host_id) =
            match self.tabs.get(self.active).map(|t| &t.kind) {
                Some(TabKind::Terminal { terminal, .. }) => {
                    let s = terminal.read(cx);
                    if let Some(host) = s.host_profile() {
                        let dot = if s.is_connected() {
                            colors.success
                        } else {
                            colors.accent
                        };
                        let status_word = if s.is_connected() {
                            language.text("Connected", "已连接")
                        } else {
                            language.text("Connecting", "连接中")
                        };
                        (
                            dot,
                            format!("{} · {}", status_word, host.endpoint()),
                            s.title().to_owned(),
                            Some(host.id),
                        )
                    } else {
                        (
                            colors.accent,
                            language.text("Local Shell", "本地终端").to_owned(),
                            s.title().to_owned(),
                            None,
                        )
                    }
                }
                Some(TabKind::Sftp { host_name, .. }) => (
                    colors.accent,
                    format!("SFTP · {host_name}"),
                    String::new(),
                    None,
                ),
                Some(TabKind::Hosts) => (
                    colors.muted,
                    language.text("SSH Hosts", "SSH 主机管理").to_owned(),
                    String::new(),
                    None,
                ),
                Some(TabKind::Snippets) => (
                    colors.muted,
                    language.text("Snippets", "命令片段管理").to_owned(),
                    String::new(),
                    None,
                ),
                Some(TabKind::Tunnels) => (
                    colors.muted,
                    language.text("Port Forwarding", "端口转发管理").to_owned(),
                    String::new(),
                    None,
                ),
                Some(TabKind::Settings) => (
                    colors.muted,
                    language.text("Settings", "偏好设置").to_owned(),
                    String::new(),
                    None,
                ),
                None => (colors.muted, "Bulwark".to_owned(), String::new(), None),
            };

        let active_tunnels = host_id
            .map(|id| self.tunnels.read(cx).active_count_for_host(id, cx))
            .unwrap_or(0);
        let font_display = if self.font_family.trim().is_empty() {
            format!("Mono · {}px", self.font_size)
        } else {
            format!("{} · {}px", self.font_family, self.font_size)
        };
        let theme_name = self.theme.label(language);

        div()
            .h(px(26.))
            .flex_shrink_0()
            .px(px(12.))
            .bg(rgb(colors.chrome))
            .border_t_1()
            .border_color(colors.hairline())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .text_size(px(11.))
            .text_color(rgb(colors.muted))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(rgb(dot_color)),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.secondary))
                                    .child(primary_label),
                            ),
                    )
                    .children((!secondary_label.is_empty()).then(|| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .min_w_0()
                            .child(div().w(px(1.)).h(px(10.)).bg(colors.hairline_strong()))
                            .child(div().truncate().child(secondary_label))
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .flex_shrink_0()
                    .children((active_tunnels > 0).then(|| {
                        Button::new("status-tunnels")
                            .ghost()
                            .xsmall()
                            .h(px(18.))
                            .px(px(6.))
                            .rounded_full()
                            .bg(colors.success_soft())
                            .text_color(rgb(colors.success))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!(
                                        "{active_tunnels} {}",
                                        language.text("tunnels", "条转发")
                                    )),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_right_drawer(RightDrawer::Tunnels, window, cx);
                            }))
                    }))
                    .child(div().text_size(px(10.5)).child(font_display))
                    .child(div().w(px(1.)).h(px(10.)).bg(colors.hairline_strong()))
                    .child(
                        Button::new("status-theme")
                            .ghost()
                            .xsmall()
                            .h(px(18.))
                            .px(px(5.))
                            .rounded(px(4.))
                            .text_color(rgb(colors.muted))
                            .child(div().text_size(px(10.5)).child(theme_name))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_settings(window, cx);
                            })),
                    ),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors();
        let mut workspace = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.background))
            .child(self.tab_bar(colors, cx));

        if let Some(tab) = self.tabs.get(self.active) {
            match &tab.kind {
                TabKind::Terminal { terminal, .. } => {
                    let host_profile = terminal.read(cx).host_profile().cloned();
                    let mut row = div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .child(terminal.clone()),
                        );
                    match self.right_drawer {
                        Some(RightDrawer::Snippets) => {
                            row = row.child(self.snippets_drawer(colors, cx));
                        }
                        Some(RightDrawer::Files) => {
                            row = row.child(self.file_drawer.clone());
                        }
                        Some(RightDrawer::Tunnels) => {
                            if let Some(host) = host_profile.as_ref() {
                                row = row.child(self.tunnels_drawer(host, colors, cx));
                            }
                        }
                        None => {}
                    }
                    row = row.child(self.right_rail(host_profile.as_ref(), colors, cx));
                    workspace = workspace.child(row);
                }
                TabKind::Sftp { view, .. } => {
                    workspace = workspace.child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(view.clone()),
                    );
                }
                TabKind::Hosts => {
                    workspace = workspace.child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.hosts.clone()),
                    );
                }
                TabKind::Snippets => {
                    workspace = workspace.child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.snippets.clone()),
                    );
                }
                TabKind::Tunnels => {
                    workspace = workspace.child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.tunnels.clone()),
                    );
                }
                TabKind::Settings => {
                    workspace = workspace.child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.preferences.clone()),
                    );
                }
            }
        }
        workspace = workspace.child(self.status_bar(colors, cx));

        let mut root = div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.new_tab(window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_active(window, cx)))
            .on_action(
                cx.listener(|this, _: &NextTab, window, cx| this.step_tab(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PreviousTab, window, cx| this.step_tab(true, window, cx)),
            )
            .on_action(cx.listener(|this, action: &SelectTab, window, cx| {
                this.select_tab(action.index, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowHosts, window, cx| {
                this.open_hosts(None, window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ShowSnippets, window, cx| {
                this.open_snippets(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ShowTunnels, window, cx| {
                this.open_tunnels(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ToggleSnippetsDrawer, window, cx| {
                this.toggle_right_drawer(RightDrawer::Snippets, window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ToggleFilesDrawer, window, cx| {
                this.toggle_right_drawer(RightDrawer::Files, window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                this.open_settings(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ToggleQuickConnect, window, cx| {
                this.toggle_quick_connect(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &FindInTerminal, window, cx| {
                if let Some(Tab {
                    kind: TabKind::Terminal { terminal, .. },
                    ..
                }) = this.tabs.get(this.active)
                {
                    terminal.update(cx, |terminal, cx| terminal.toggle_search(window, cx));
                    cx.stop_propagation();
                }
            }))
            .on_action(
                cx.listener(|this, _: &CloseWindow, window, cx| this.close_window(window, cx)),
            )
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &ToggleFullscreen, window, _| window.toggle_fullscreen())
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.update_sidebar_resize(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_sidebar_resize(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_sidebar_resize(cx);
                }),
            )
            .size_full()
            .flex()
            .text_size(px(13.))
            .bg(rgb(colors.background))
            .text_color(rgb(colors.foreground))
            .child(self.sidebar(colors, cx))
            .child(self.sidebar_resize_handle(colors, cx))
            .child(workspace);
        if self.resizing_sidebar.is_some() {
            root = root.cursor_col_resize();
        }
        if self.quick_connect_open {
            root = root.child(self.quick_connect.clone());
        }
        if !cfg!(target_os = "macos") {
            root = root.opacity(f32::from(self.opacity) / 100.);
        }
        root
    }
}

fn nav_button(
    id: impl Into<gpui_kit::ElementId>,
    icon: Icon,
    name: impl Into<SharedString>,
    badge: Option<String>,
    selected: bool,
    colors: Colors,
) -> Button {
    let name = name.into();
    let icon_color = if selected {
        colors.accent
    } else {
        colors.muted
    };
    let text_color = if selected {
        colors.foreground
    } else {
        colors.secondary
    };
    let mut btn = Button::new(id)
        .relative()
        .w_full()
        .h(px(32.))
        .rounded_lg()
        .justify_start()
        .px(px(9.))
        .ghost()
        .gap(px(8.))
        .bg(if selected {
            colors.accent_subtle()
        } else {
            transparent_black().into()
        })
        .border_1()
        .border_color(if selected {
            colors.hairline()
        } else {
            transparent_black().into()
        })
        .text_color(rgb(text_color))
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .children(selected.then(|| {
            div()
                .absolute()
                .left_0()
                .top(px(6.))
                .bottom(px(6.))
                .w(px(2.5))
                .rounded_r_full()
                .bg(rgb(colors.accent))
        }))
        .child(icon.render(px(14.5), icon_color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.5))
                .child(name),
        );

    if let Some(badge) = badge {
        btn = btn.child(
            div()
                .px(px(6.))
                .py(px(1.))
                .rounded_full()
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .bg(if selected {
                    colors.accent_soft()
                } else {
                    rgb(colors.surface)
                })
                .text_color(rgb(if selected {
                    colors.accent
                } else {
                    colors.muted
                }))
                .child(badge),
        );
    }
    btn
}

fn active_after_close(active: usize, removed: usize, remaining: usize) -> usize {
    if removed < active {
        active - 1
    } else {
        active.min(remaining.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::active_after_close;

    #[test]
    fn closing_tabs_preserves_the_active_session_or_selects_a_neighbor() {
        assert_eq!(active_after_close(2, 0, 3), 1);
        assert_eq!(active_after_close(1, 3, 3), 1);
        assert_eq!(active_after_close(1, 1, 3), 1);
        assert_eq!(active_after_close(3, 3, 3), 2);
    }
}
