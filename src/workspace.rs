use std::collections::BTreeMap;
use std::sync::Arc;

use gpui_kit::{
    AppContext as _, ClipboardItem, Context, ElementId, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Pixels,
    Point, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
    },
    div, px, rgb, rgba, transparent_black,
};

use crate::file_drawer::{FileDrawer, FileDrawerEvent};
use crate::icons::Icon;
use crate::mcp::{McpRuntime, McpUiEvent};
use crate::platform::{apply_window_style, drag_window, hide_traffic_lights};
use crate::preferences::{
    HostsEvent, HostsPage, KeychainPage, Preferences, SnippetsEvent, SnippetsPage, TunnelsPage,
};
use crate::quick_connect::{QuickConnectEvent, QuickConnectPalette};
use crate::settings::{HostProfile, HostProtocol, Language, SettingsStore, Snippet};
use crate::sftp_view::{SftpView, SftpViewEvent};
use crate::ssh::{ServerTelemetry, supports_sftp};
use crate::terminal::TerminalSession;
use crate::theme::{Colors, Theme};
use crate::{
    CloseTab, CloseWindow, FindInTerminal, MinimizeWindow, NewTab, NextTab, PreviousTab,
    SelectTab, ShowHosts, ShowSettings, ShowSnippets, ShowTunnels, ShowVault, SplitPane,
    ToggleFilesDrawer, ToggleFullscreen, ToggleQuickConnect, ToggleSnippetsDrawer,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum VaultSection {
    Hosts,
    Keychain,
    Tunnels,
    Snippets,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightDrawer {
    Files,
    Snippets,
    Tunnels,
    Docker,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum DockerFilter {
    #[default]
    All,
    Running,
    Stopped,
}

struct DockerLogsState {
    session: Entity<TerminalSession>,
    host_name: String,
    container_name: String,
    container_running: bool,
    tail: usize,
    timestamps: bool,
    loading: bool,
    error: Option<String>,
    logs: String,
    search: Entity<InputState>,
    _search_sub: Subscription,
    request_seq: u64,
    focus: FocusHandle,
}

struct TerminalPane {
    terminal: Entity<TerminalSession>,
    _updates: Subscription,
}

enum TabKind {
    Terminal {
        panes: Vec<TerminalPane>,
        active_pane: usize,
    },
    Sftp {
        view: Entity<SftpView>,
        host_name: String,
        _updates: Subscription,
        _events: Subscription,
    },
}

struct Tab {
    id: u64,
    kind: TabKind,
}

pub struct Workspace {
    focus: FocusHandle,
    in_vault: bool,
    vault_section: VaultSection,
    selected_group: Option<String>,
    vault_search: Entity<InputState>,
    tabs: Vec<Tab>,
    active: usize,
    next_tab: u64,
    tab_scroll: ScrollHandle,
    settings: Entity<SettingsStore>,
    hosts: Entity<HostsPage>,
    keychain: Entity<KeychainPage>,
    snippets: Entity<SnippetsPage>,
    tunnels: Entity<TunnelsPage>,
    mcp_runtime: Arc<McpRuntime>,
    preferences: Entity<Preferences>,
    quick_connect: Entity<QuickConnectPalette>,
    file_drawer: Entity<FileDrawer>,
    quick_connect_open: bool,
    new_tab_menu_open: bool,
    new_tab_menu_pos: Option<Point<Pixels>>,
    right_drawer: Option<RightDrawer>,
    docker_filter: DockerFilter,
    docker_logs: Option<DockerLogsState>,
    telemetry_open: bool,
    theme: Theme,
    font_family: String,
    font_size: u8,
    language: Language,
    opacity: u8,
    vibrancy: bool,
    _settings_updates: Subscription,
    _hosts_events: Subscription,
    _snippets_events: Subscription,
    _quick_connect_events: Subscription,
    _file_drawer_events: Subscription,
    _search_events: Subscription,
    _window_bounds: Subscription,
}

impl Workspace {
    pub fn new(
        settings: Entity<SettingsStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = &settings.read(cx).current;
        let (theme, font_family, font_size, language, opacity, vibrancy) = (
            saved.theme,
            saved.font_family.clone(),
            saved.font_size,
            saved.language,
            saved.opacity,
            saved.vibrancy,
        );
        window.defer(cx, move |window, _| {
            apply_window_style(window, opacity, vibrancy)
        });
        let window_bounds = cx.observe_window_bounds(window, |_this, window, _| {
            hide_traffic_lights(window);
        });
        let settings_updates = cx.observe_in(&settings, window, |this, settings, window, cx| {
            let saved = settings.read(cx).current.clone();
            this.mcp_runtime.sync_settings(saved.clone());
            let (theme, font_family, font_size, language, opacity, vibrancy) = (
                saved.theme,
                saved.font_family.clone(),
                saved.font_size,
                saved.language,
                saved.opacity,
                saved.vibrancy,
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
                    .flat_map(|tab| match &tab.kind {
                        TabKind::Terminal { panes, .. } => {
                            panes.iter().map(|p| p.terminal.clone()).collect::<Vec<_>>()
                        }
                        TabKind::Sftp { .. } => Vec::new(),
                    })
                    .collect();
                for terminal in terminals {
                    let ff = font_family.clone();
                    terminal.update(cx, |terminal, cx| {
                        terminal.set_appearance(colors, palette, ff, font_size, language, cx);
                    });
                }
            }
            if this.opacity != opacity || this.vibrancy != vibrancy {
                this.opacity = opacity;
                this.vibrancy = vibrancy;
                apply_window_style(window, opacity, vibrancy);
            }
            this.sync_file_drawer(window, cx);
            cx.notify();
        });

        let hosts = cx.new(|cx| HostsPage::new(settings.clone(), window, cx));
        let hosts_events = cx.subscribe_in(&hosts, window, |this, _, event, window, cx| {
            match event {
                HostsEvent::Connect(host) => this.connect_host(host.clone(), window, cx),
                HostsEvent::OpenSftp(host) => this.open_sftp(host.clone(), window, cx),
            }
        });
        let keychain = cx.new(|cx| KeychainPage::new(settings.clone(), window, cx));
        let snippets = cx.new(|cx| SnippetsPage::new(settings.clone(), window, cx));
        let snippets_events = cx.subscribe_in(&snippets, window, |this, _, event, window, cx| {
            match event {
                SnippetsEvent::Run(snippet) => this.run_snippet(snippet, window, cx),
            }
        });
        let tunnels = cx.new(|cx| TunnelsPage::new(settings.clone(), window, cx));
        let (mcp_tx, mcp_rx) = async_channel::unbounded::<McpUiEvent>();
        let mcp_runtime = McpRuntime::new(settings.read(cx).current.clone(), Some(mcp_tx));
        cx.spawn_in(window, async move |ws, cx| {
            while let Ok(ev) = mcp_rx.recv().await {
                let _ = cx.update(|window, cx| {
                    let _ = ws.update(cx, |this, cx| {
                        match ev {
                            McpUiEvent::Changed => {}
                            McpUiEvent::ApprovalRequested { client, host, cmd } => {
                                let lang = this.language;
                                crate::notify::warning(
                                    format!(
                                        "{} {client} → {host}: {cmd}",
                                        lang.text("MCP approval needed:", "MCP 待审批：")
                                    ),
                                    window,
                                    cx,
                                );
                            }
                            McpUiEvent::ServerError(err) => {
                                crate::notify::error(err, window, cx);
                            }
                        }
                        this.preferences.update(cx, |p, cx| p.notify_mcp(cx));
                        cx.notify();
                    });
                });
            }
        })
        .detach();
        let preferences = cx.new(|cx| {
            Preferences::new(settings.clone(), Arc::clone(&mcp_runtime), window, cx)
        });
        let quick_connect = cx.new(|cx| QuickConnectPalette::new(settings.clone(), window, cx));
        let file_drawer = cx.new(|cx| FileDrawer::new(settings.clone(), window, cx));
        let file_drawer_events =
            cx.subscribe_in(&file_drawer, window, |this, _, event, window, cx| {
                match event {
                    FileDrawerEvent::InsertPath(path) => {
                        if let Some(terminal) = this.active_terminal() {
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
                    FileDrawerEvent::Close => {
                        this.right_drawer = None;
                        cx.notify();
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

        let vault_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(
                "Search hosts, IP, tags…",
                "搜索主机、IP、标签…",
            ))
        });
        let search_events = cx.subscribe(&vault_search, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let q = input.read(cx).value().to_string();
                this.hosts.update(cx, |h, cx| h.set_search_query(q, cx));
            }
        });

        let this = Self {
            focus: cx.focus_handle(),
            in_vault: true,
            vault_section: VaultSection::Hosts,
            selected_group: None,
            vault_search,
            tabs: Vec::new(),
            active: 0,
            next_tab: 1,
            tab_scroll: ScrollHandle::new(),
            settings,
            hosts,
            keychain,
            snippets,
            tunnels,
            mcp_runtime,
            preferences,
            quick_connect,
            file_drawer,
            quick_connect_open: false,
            new_tab_menu_open: false,
            new_tab_menu_pos: None,
            right_drawer: None,
            docker_filter: DockerFilter::All,
            docker_logs: None,
            telemetry_open: false,
            theme,
            font_family,
            font_size,
            language,
            opacity,
            vibrancy,
            _settings_updates: settings_updates,
            _hosts_events: hosts_events,
            _snippets_events: snippets_events,
            _quick_connect_events: quick_connect_events,
            _file_drawer_events: file_drawer_events,
            _search_events: search_events,
            _window_bounds: window_bounds,
        };
        this
    }

    fn alloc_tab_id(&mut self) -> u64 {
        let id = self.next_tab;
        self.next_tab += 1;
        id
    }

    fn active_terminal(&self) -> Option<Entity<TerminalSession>> {
        match &self.tabs.get(self.active)?.kind {
            TabKind::Terminal { panes, active_pane } => {
                panes.get(*active_pane).or_else(|| panes.first()).map(|p| p.terminal.clone())
            }
            TabKind::Sftp { .. } => None,
        }
    }

    fn spawn_local_tab(&mut self, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let colors = self.theme.colors();
        let palette = self.theme.palette();
        let font_family = self.font_family.clone();
        let font_size = self.font_size;
        let language = self.language;
        let shell = self.settings.read(cx).current.shell.clone();
        let terminal = cx.new(|cx| {
            TerminalSession::local(colors, palette, font_family, font_size, language, &shell, cx)
        });
        let updates = cx.observe(&terminal, |_, _, cx| cx.notify());
        let id = self.alloc_tab_id();
        self.tabs.push(Tab {
            id,
            kind: TabKind::Terminal {
                panes: vec![TerminalPane {
                    terminal: terminal.clone(),
                    _updates: updates,
                }],
                active_pane: 0,
            },
        });
        if activate {
            self.active = self.tabs.len() - 1;
            self.in_vault = false;
            self.telemetry_open = false;
            terminal.read(cx).focus_handle().focus(window, cx);
            self.sync_file_drawer(window, cx);
        }
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.spawn_local_tab(true, window, cx);
    }

    fn connect_host(&mut self, host: HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        let resolved = self.settings.read(cx).current.resolve_host_auth(&host);
        let all_hosts: Vec<HostProfile> = self
            .settings
            .read(cx)
            .current
            .hosts
            .iter()
            .map(|h| self.settings.read(cx).current.resolve_host_auth(h))
            .collect();
        if host.id > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let target_id = host.id;
            let _ = self.settings.update(cx, |store, cx| {
                let _ = store.update(|s| {
                    if let Some(h) = s.hosts.iter_mut().find(|h| h.id == target_id) {
                        h.last_connected = Some(now);
                    }
                });
                cx.notify();
            });
        }
        let colors = self.theme.colors();
        let palette = self.theme.palette();
        let font_family = self.font_family.clone();
        let font_size = self.font_size;
        let language = self.language;
        let terminal = cx.new(|cx| {
            TerminalSession::ssh(
                colors,
                palette,
                font_family,
                font_size,
                language,
                resolved,
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
                panes: vec![TerminalPane {
                    terminal: terminal.clone(),
                    _updates: updates,
                }],
                active_pane: 0,
            },
        });
        self.active = self.tabs.len() - 1;
        self.in_vault = false;
        self.telemetry_open = false;
        terminal.read(cx).focus_handle().focus(window, cx);
        self.sync_file_drawer(window, cx);
        cx.notify();
    }

    fn toggle_split_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.in_vault {
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let TabKind::Terminal { panes, active_pane } = &mut tab.kind else {
            return;
        };
        if panes.len() >= 2 {
            let removed = panes.pop();
            if let Some(pane) = removed {
                pane.terminal.update(cx, |t, cx| t.shutdown(cx));
            }
            *active_pane = 0;
            if let Some(first) = panes.first() {
                first.terminal.read(cx).focus_handle().focus(window, cx);
            }
            cx.notify();
            return;
        }

        let host_opt = panes
            .first()
            .and_then(|p| p.terminal.read(cx).host_profile().cloned());
        let colors = self.theme.colors();
        let palette = self.theme.palette();
        let font_family = self.font_family.clone();
        let font_size = self.font_size;
        let language = self.language;

        let new_term = if let Some(host) = host_opt {
            let resolved = self.settings.read(cx).current.resolve_host_auth(&host);
            let all_hosts: Vec<HostProfile> = self
                .settings
                .read(cx)
                .current
                .hosts
                .iter()
                .map(|h| self.settings.read(cx).current.resolve_host_auth(h))
                .collect();
            cx.new(|cx| {
                TerminalSession::ssh(
                    colors,
                    palette,
                    font_family,
                    font_size,
                    language,
                    resolved,
                    all_hosts,
                    window,
                    cx,
                )
            })
        } else {
            let shell = self.settings.read(cx).current.shell.clone();
            cx.new(|cx| {
                TerminalSession::local(
                    colors,
                    palette,
                    font_family,
                    font_size,
                    language,
                    &shell,
                    cx,
                )
            })
        };
        let updates = cx.observe(&new_term, |_, _, cx| cx.notify());
        if let Some(Tab {
            kind: TabKind::Terminal { panes, active_pane },
            ..
        }) = self.tabs.get_mut(self.active)
        {
            panes.push(TerminalPane {
                terminal: new_term.clone(),
                _updates: updates,
            });
            *active_pane = panes.len() - 1;
        }
        new_term.read(cx).focus_handle().focus(window, cx);
        cx.notify();
    }

    fn open_sftp(&mut self, host: HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        if !supports_sftp(&host, &self.settings.read(cx).current.hosts) {
            let language = self.settings.read(cx).current.language;
            crate::notify::info(
                language.text(
                    "SFTP is unavailable for this host.",
                    "当前主机不支持 SFTP。",
                ),
                window,
                cx,
            );
            return;
        }
        let resolved = self.settings.read(cx).current.resolve_host_auth(&host);
        let host_name = resolved.name.clone();
        let view = cx.new(|cx| SftpView::new(self.settings.clone(), resolved, window, cx));
        let updates = cx.observe(&view, |_, _, cx| cx.notify());
        let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            SftpViewEvent::ReturnToTerminal(host) => {
                this.return_to_terminal(host.clone(), window, cx);
            }
        });
        let id = self.alloc_tab_id();
        self.tabs.push(Tab {
            id,
            kind: TabKind::Sftp {
                view,
                host_name,
                _updates: updates,
                _events: events,
            },
        });
        self.active = self.tabs.len() - 1;
        self.in_vault = false;
        self.telemetry_open = false;
        cx.notify();
    }

    fn return_to_terminal(
        &mut self,
        host: HostProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(idx) = self.tabs.iter().position(|tab| match &tab.kind {
            TabKind::Terminal { panes, .. } => panes.iter().any(|p| {
                p.terminal
                    .read(cx)
                    .host_profile()
                    .is_some_and(|h| h.id == host.id)
            }),
            TabKind::Sftp { .. } => false,
        }) {
            self.active = idx;
            self.in_vault = false;
            self.restore_active_focus(window, cx);
            self.sync_file_drawer(window, cx);
            cx.notify();
        } else {
            self.connect_host(host, window, cx);
        }
    }

    fn run_snippet(&mut self, snippet: &Snippet, window: &mut Window, cx: &mut Context<Self>) {
        if self.in_vault || self.active_terminal().is_none() {
            if let Some(idx) = self
                .tabs
                .iter()
                .position(|t| matches!(t.kind, TabKind::Terminal { .. }))
            {
                self.active = idx;
                self.in_vault = false;
            } else {
                self.new_tab(window, cx);
            }
        }
        if let Some(terminal) = self.active_terminal() {
            let payload = snippet.payload();
            terminal.update(cx, |session, cx| {
                session.send_bytes(&payload, cx);
                session.focus_handle().focus(window, cx);
            });
        }
        cx.notify();
    }

    fn open_vault(&mut self, section: VaultSection, cx: &mut Context<Self>) {
        self.in_vault = true;
        self.vault_section = section;
        self.telemetry_open = false;
        cx.notify();
    }

    pub fn open_hosts(&mut self, _edit_id: Option<u64>, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_vault(VaultSection::Hosts, cx);
    }

    pub fn open_snippets(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_vault(VaultSection::Snippets, cx);
    }

    pub fn open_tunnels(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_vault(VaultSection::Tunnels, cx);
    }

    pub fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_vault(VaultSection::Settings, cx);
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active = index;
            self.in_vault = false;
            self.telemetry_open = false;
            self.restore_active_focus(window, cx);
            self.sync_file_drawer(window, cx);
            cx.notify();
        }
    }

    fn step_tab(&mut self, backward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let next = if backward {
            (self.active + self.tabs.len() - 1) % self.tabs.len()
        } else {
            (self.active + 1) % self.tabs.len()
        };
        self.select_tab(next, window, cx);
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let removed = self.tabs.remove(index);
        match removed.kind {
            TabKind::Terminal { panes, .. } => {
                for pane in panes {
                    pane.terminal.update(cx, |t, cx| t.shutdown(cx));
                }
            }
            TabKind::Sftp { .. } => {}
        }
        if self.tabs.is_empty() {
            self.active = 0;
            self.in_vault = true;
        } else {
            self.active = active_after_close(self.active, index, self.tabs.len());
            if !self.in_vault {
                self.restore_active_focus(window, cx);
                self.sync_file_drawer(window, cx);
            }
        }
        cx.notify();
    }

    fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.in_vault && !self.tabs.is_empty() {
            self.close_tab(self.active, window, cx);
        }
    }

    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tunnels.update(cx, |t, _| t.shutdown_all());
        for tab in &self.tabs {
            if let TabKind::Terminal { panes, .. } = &tab.kind {
                for pane in panes {
                    pane.terminal.update(cx, |t, cx| t.shutdown(cx));
                }
            }
        }
        window.remove_window();
    }

    fn toggle_quick_connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quick_connect_open = !self.quick_connect_open;
        if self.quick_connect_open {
            self.quick_connect
                .update(cx, |qc, cx| qc.focus_input(window, cx));
        } else {
            self.restore_active_focus(window, cx);
        }
        cx.notify();
    }

    fn toggle_right_drawer(
        &mut self,
        drawer: RightDrawer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.right_drawer == Some(drawer) {
            self.right_drawer = None;
        } else if drawer != RightDrawer::Files || self.file_drawer_terminal(cx).is_some() {
            self.right_drawer = Some(drawer);
            if drawer == RightDrawer::Files {
                self.sync_file_drawer(window, cx);
            }
        }
        cx.notify();
    }

    fn toggle_tunnels_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self
            .active_terminal()
            .and_then(|terminal| terminal.read(cx).host_profile().cloned());
        let Some(host) = host else {
            return;
        };
        if host.protocol != HostProtocol::Ssh {
            let language = self.language;
            crate::notify::info(
                language.text(
                    "Port forwarding is only supported for SSH hosts.",
                    "当前主机协议不支持端口转发。",
                ),
                window,
                cx,
            );
            return;
        }
        self.toggle_right_drawer(RightDrawer::Tunnels, window, cx);
    }

    fn file_drawer_terminal(&self, cx: &Context<Self>) -> Option<Entity<TerminalSession>> {
        if self.in_vault {
            return None;
        }
        let all_hosts = &self.settings.read(cx).current.hosts;
        self.active_terminal().filter(|terminal| {
            terminal
                .read(cx)
                .host_profile()
                .is_none_or(|host| supports_sftp(host, all_hosts))
        })
    }

    fn sync_file_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.right_drawer != Some(RightDrawer::Files) {
            return;
        }
        let terminal = self.file_drawer_terminal(cx);
        if terminal.is_none() {
            self.right_drawer = None;
        }
        let host = terminal.and_then(|t| t.read(cx).host_profile().cloned());
        self.file_drawer.update(cx, |drawer, cx| {
            drawer.sync_target(host.as_ref(), window, cx);
        });
    }

    fn restore_active_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.in_vault
            && let Some(term) = self.active_terminal()
        {
            term.read(cx).focus_handle().focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
    }

    fn titlebar(&self, colors: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let vibrancy = self.vibrancy;

        let traffic_lights = cfg!(target_os = "macos").then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .mr(px(8.))
                .child(
                    div()
                        .id("tl-close")
                        .size(px(12.))
                        .rounded_full()
                        .bg(rgb(0xff5f57))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_window(window, cx);
                        })),
                )
                .child(
                    div()
                        .id("tl-min")
                        .size(px(12.))
                        .rounded_full()
                        .bg(rgb(0xfebc2e))
                        .cursor_pointer()
                        .on_click(|_, window, _| window.minimize_window()),
                )
                .child(
                    div()
                        .id("tl-full")
                        .size(px(12.))
                        .rounded_full()
                        .bg(rgb(0x28c840))
                        .cursor_pointer()
                        .on_click(|_, window, _| window.toggle_fullscreen()),
                )
        });

        let vault_btn = div()
            .id("tab-vault-home")
            .h(px(26.))
            .px(px(10.))
            .rounded(px(6.))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(6.))
            .border_1()
            .border_color(if self.in_vault {
                colors.hairline_strong()
            } else {
                transparent_black().into()
            })
            .bg(if self.in_vault {
                colors.surface_active_glass()
            } else {
                transparent_black().into()
            })
            .hover(|s| s.bg(colors.surface_hover_glass()))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(if self.in_vault {
                colors.foreground
            } else {
                colors.secondary
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.in_vault = true;
                this.telemetry_open = false;
                cx.notify();
            }))
            .child(crate::icons::app_logo(px(18.)))
            .child(language.text("Workspace", "工作台"));

        let tabs_row = div()
            .id("session-tabs-strip")
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .flex()
            .items_center()
            .gap(px(4.))
            .children(self.tabs.iter().enumerate().map(|(idx, tab)| {
                let is_active = !self.in_vault && idx == self.active;
                let (dot_color, label) = match &tab.kind {
                    TabKind::Terminal { panes, active_pane } => {
                        let term = panes
                            .get(*active_pane)
                            .or_else(|| panes.first())
                            .map(|p| p.terminal.read(cx));
                        let (is_ssh, connected, title) = match term {
                            Some(t) => (t.is_ssh(), t.is_connected(), t.title().to_owned()),
                            None => (false, false, "Terminal".into()),
                        };
                        let dot = if is_ssh && connected {
                            colors.success
                        } else if is_ssh {
                            colors.warning()
                        } else {
                            colors.muted
                        };
                        let split_suffix = if panes.len() > 1 { " · 2" } else { "" };
                        (dot, format!("{title}{split_suffix}"))
                    }
                    TabKind::Sftp { host_name, .. } => (colors.accent, format!("SFTP · {host_name}")),
                };

                div()
                    .id(("session-tab", tab.id))
                    .h(px(26.))
                    .max_w(px(210.))
                    .pl(px(10.))
                    .pr(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .border_1()
                    .border_color(if is_active {
                        colors.hairline()
                    } else {
                        transparent_black().into()
                    })
                    .bg(if is_active {
                        colors.surface_active_glass()
                    } else {
                        transparent_black().into()
                    })
                    .hover(|s| s.bg(colors.surface_hover_glass()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_tab(idx, window, cx);
                    }))
                    .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(rgb(dot_color)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(if is_active {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(if is_active {
                                colors.foreground
                            } else {
                                colors.secondary
                            }))
                            .child(label),
                    )
                    .child(
                        div()
                            .id(("tab-close", tab.id))
                            .size(px(16.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(colors.surface_hover_glass()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(idx, window, cx);
                            }))
                            .child(Icon::Close.render(px(10.), colors.muted)),
                    )
            }))
            .child(
                div()
                    .id("new-tab-dropdown")
                    .size(px(26.))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(if self.new_tab_menu_open {
                        colors.hairline()
                    } else {
                        transparent_black().into()
                    })
                    .bg(if self.new_tab_menu_open {
                        colors.surface_active_glass()
                    } else {
                        transparent_black().into()
                    })
                    .hover(|s| s.bg(colors.surface_hover_glass()))
                    .on_click(cx.listener(|this, event: &gpui_kit::ClickEvent, _, cx| {
                        this.new_tab_menu_open = !this.new_tab_menu_open;
                        this.new_tab_menu_pos = Some(event.position());
                        cx.notify();
                    }))
                    .child(Icon::Plus.render(
                        px(13.),
                        if self.new_tab_menu_open {
                            colors.foreground
                        } else {
                            colors.secondary
                        },
                    )),
            );

        let pill_bg = if self.quick_connect_open {
            colors.surface_active_glass()
        } else if colors.dark {
            rgb(0xffffff).alpha(0.055)
        } else {
            rgb(0xffffff).alpha(0.62)
        };
        let pill_hover_bg = if colors.dark {
            rgb(0xffffff).alpha(0.09)
        } else {
            rgb(0xffffff).alpha(0.84)
        };
        let kbd_bg = if colors.dark {
            rgb(0xffffff).alpha(0.07)
        } else {
            rgb(0x000000).alpha(0.045)
        };

        let cmdk_pill = div()
            .id("titlebar-cmdk")
            .h(px(26.))
            .w(px(196.))
            .flex_shrink_0()
            .px(px(9.))
            .rounded(px(7.))
            .cursor_pointer()
            .bg(pill_bg)
            .border_1()
            .border_color(if self.quick_connect_open {
                colors.hairline_strong()
            } else {
                colors.hairline()
            })
            .shadow_xs()
            .hover(move |s| s.bg(pill_hover_bg).border_color(colors.hairline_strong()))
            .active(|s| s.bg(colors.surface_active_glass()))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_quick_connect(window, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(Icon::Search.render(px(12.), colors.secondary))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.secondary))
                            .child(language.text("Quick Connect", "快速连接")),
                    ),
            )
            .child(
                div()
                    .h(px(16.))
                    .px(px(5.))
                    .rounded(px(4.))
                    .bg(kbd_bg)
                    .border_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(9.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(colors.secondary))
                    .child("⌘K"),
            );

        div()
            .id("unified-titlebar")
            .h(px(38.))
            .w_full()
            .px(px(12.))
            .bg(colors.titlebar_bg(vibrancy, self.opacity))
            .border_b_1()
            .border_color(colors.hairline())
            .flex()
            .items_center()
            .gap(px(8.))
            .on_mouse_down(MouseButton::Left, drag_window)
            .children(traffic_lights)
            .child(vault_btn)
            .child(div().w(px(1.)).h(px(16.)).bg(colors.hairline()))
            .child(tabs_row)
            .child(cmdk_pill)
    }

    fn render_new_tab_menu(&self, colors: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let hosts = self.settings.read(cx).current.hosts.clone();
        let left_x = self
            .new_tab_menu_pos
            .map(|p| (f32::from(p.x) - 18.0).max(12.0))
            .unwrap_or(240.0);

        let menu = div()
            .id("new-tab-host-menu")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .top(px(38.))
            .left(px(left_x))
            .w(px(248.))
            .max_h(px(360.))
            .rounded(px(9.))
            .bg(colors.popover_bg())
            .border_1()
            .border_color(colors.hairline_strong())
            .shadow_lg()
            .p(px(5.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .id("new-tab-menu-local")
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(colors.accent_soft()))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.new_tab_menu_open = false;
                        this.new_tab(window, cx);
                    }))
                    .child(Icon::Terminal.render(px(14.), colors.accent))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("Local Terminal", "本地终端")),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors.muted))
                            .child("⌘T"),
                    ),
            )
            .child(div().h(px(1.)).my(px(3.)).bg(colors.hairline()))
            .child(
                div()
                    .id("new-tab-menu-hosts-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(if hosts.is_empty() {
                        vec![
                            div()
                                .px(px(8.))
                                .py(px(8.))
                                .text_size(px(11.5))
                                .text_color(rgb(colors.muted))
                                .child(language.text("No saved hosts", "暂无已保存主机"))
                                .into_any_element(),
                        ]
                    } else {
                        hosts
                            .into_iter()
                            .enumerate()
                            .map(|(ix, host)| {
                                let label = host.name.clone();
                                let endpoint = host.endpoint();
                                let icon = host.icon.icon();
                                div()
                                    .id(("new-tab-menu-host", ix))
                                    .px(px(8.))
                                    .py(px(6.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(colors.accent_soft()))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.new_tab_menu_open = false;
                                        this.connect_host(host.clone(), window, cx);
                                    }))
                                    .child(icon.render(px(14.), colors.secondary))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(label),
                                            )
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(10.5))
                                                    .text_color(rgb(colors.muted))
                                                    .child(endpoint),
                                            ),
                                    )
                                    .into_any_element()
                            })
                            .collect()
                    }),
            );

        div()
            .id("new-tab-menu-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.new_tab_menu_open = false;
                    cx.notify();
                }),
            )
            .child(menu)
    }

    fn vault_sidebar(&self, colors: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.settings.read(cx).current;
        let language = saved.language;
        let vibrancy = self.vibrancy;
        let hosts_count = saved.hosts.len();
        let kc_count = saved.keychain.len();
        let tunnels_count = saved.tunnels.len();
        let snippets_count = saved.snippets.len();

        let mut group_counts: BTreeMap<String, usize> = BTreeMap::new();
        for h in &saved.hosts {
            let g = h.group.trim();
            if !g.is_empty() {
                *group_counts.entry(g.to_owned()).or_insert(0) += 1;
            }
        }

        let dot_palette = [colors.success, colors.accent, colors.warning(), 0xbf5af2];

        div()
            .w(px(214.))
            .flex_shrink_0()
            .h_full()
            .bg(colors.sidebar_bg(vibrancy, self.opacity))
            .border_r_1()
            .border_color(colors.hairline())
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                Input::new(&self.vault_search)
                    .h(px(30.))
                    .py(px(1.))
                    .text_size(px(12.5))
                    .line_height(px(22.))
                    .prefix(Icon::Search.render(px(12.5), colors.muted)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .px(px(8.))
                            .pb(px(4.))
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.muted))
                            .child(language.text("RESOURCES", "资源")),
                    )
                    .child(
                        vault_nav_item(
                            "vnav-hosts",
                            Icon::Server,
                            language.text("Hosts", "主机"),
                            Some(hosts_count.to_string()),
                            self.vault_section == VaultSection::Hosts && self.selected_group.is_none(),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.vault_section = VaultSection::Hosts;
                            this.selected_group = None;
                            this.hosts.update(cx, |h, cx| h.set_group_filter(None, cx));
                            cx.notify();
                        })),
                    )
                    .child(
                        vault_nav_item(
                            "vnav-keychain",
                            Icon::Key,
                            language.text("Keychain", "密钥库"),
                            Some(kc_count.to_string()),
                            self.vault_section == VaultSection::Keychain,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.vault_section = VaultSection::Keychain;
                            cx.notify();
                        })),
                    )
                    .child(
                        vault_nav_item(
                            "vnav-tunnels",
                            Icon::Network,
                            language.text("Port Forwarding", "端口转发"),
                            Some(tunnels_count.to_string()),
                            self.vault_section == VaultSection::Tunnels,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.vault_section = VaultSection::Tunnels;
                            cx.notify();
                        })),
                    )
                    .child(
                        vault_nav_item(
                            "vnav-snippets",
                            Icon::Code,
                            language.text("Snippets", "命令片段"),
                            Some(snippets_count.to_string()),
                            self.vault_section == VaultSection::Snippets,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.vault_section = VaultSection::Snippets;
                            cx.notify();
                        })),
                    ),
            )
            .children((!group_counts.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .px(px(8.))
                            .pb(px(4.))
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.muted))
                            .child(language.text("GROUPS", "分组")),
                    )
                    .children(group_counts.into_iter().enumerate().map(|(i, (name, count))| {
                        let dot = dot_palette[i % dot_palette.len()];
                        let active = self.vault_section == VaultSection::Hosts
                            && self.selected_group.as_deref() == Some(name.as_str());
                        let group_click = name.clone();
                        div()
                            .id(("vgroup", i))
                            .h(px(28.))
                            .px(px(8.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .bg(if active {
                                colors.accent_soft()
                            } else {
                                transparent_black().into()
                            })
                            .hover(|s| s.bg(colors.surface_hover_glass()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.vault_section = VaultSection::Hosts;
                                let next = if this.selected_group.as_deref() == Some(&group_click) {
                                    None
                                } else {
                                    Some(group_click.clone())
                                };
                                this.selected_group = next.clone();
                                this.hosts.update(cx, |h, cx| h.set_group_filter(next, cx));
                                cx.notify();
                            }))
                            .child(div().size(px(7.)).rounded_full().bg(rgb(dot)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .text_color(rgb(if active {
                                        colors.accent
                                    } else {
                                        colors.secondary
                                    }))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(count.to_string()),
                            )
                    }))
            }))
            .child(
                div()
                    .mt_auto()
                    .pt(px(8.))
                    .border_t_1()
                    .border_color(colors.hairline())
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        vault_nav_item(
                            "vnav-settings",
                            Icon::Settings,
                            language.text("Settings", "设置"),
                            None,
                            self.vault_section == VaultSection::Settings,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.vault_section = VaultSection::Settings;
                            cx.notify();
                        })),
                    ),
            )
    }

    fn snippets_drawer(&self, colors: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let snippets = self.settings.read(cx).current.snippets.clone();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(38.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("Snippets", "命令片段")),
                    )
                    .child(
                        Button::new("close-snip-drawer")
                            .ghost()
                            .xsmall()
                            .icon(Icon::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.right_drawer = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("snip-drawer-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(snippets.into_iter().map(|s| {
                        let id = s.id;
                        let snippet_run = s.clone();
                        div()
                            .id(("sd-item", id))
                            .p(px(9.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .bg(colors.surface_glass())
                            .border_1()
                            .border_color(colors.hairline())
                            .hover(|st| {
                                st.bg(colors.surface_hover_glass())
                                    .border_color(colors.accent_border())
                            })
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.run_snippet(&snippet_run, window, cx);
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .truncate()
                                            .child(s.name),
                                    )
                                    .children((!s.group.is_empty()).then(|| {
                                        div()
                                            .px(px(5.))
                                            .py(px(1.))
                                            .rounded(px(4.))
                                            .bg(colors.surface_active_glass())
                                            .text_size(px(9.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(s.group)
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .truncate()
                                    .child(s.command),
                            )
                    })),
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
        let body = self.tunnels.update(cx, |tunnels, cx| {
            tunnels.render_host_drawer_body(host, colors, language, cx)
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(38.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("Port Forwarding", "端口转发")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .children((host.protocol == HostProtocol::Ssh).then(|| {
                                Button::new("add-tunnel-drawer-btn")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::Plus)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.tunnels.update(cx, |t, cx| {
                                            t.edit_rule_for_host(None, host_id, window, cx);
                                        });
                                        cx.notify();
                                    }))
                            }))
                            .child(
                                Button::new("close-tun-drawer")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::Close)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.right_drawer = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(body)
    }

    fn docker_drawer(
        &self,
        host_name: &str,
        telemetry: Option<&ServerTelemetry>,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let containers = telemetry.map(|t| t.containers.as_slice()).unwrap_or(&[]);
        let running_count = containers.iter().filter(|c| c.running).count();
        let stopped_count = containers.len().saturating_sub(running_count);
        let filtered: Vec<_> = containers
            .iter()
            .filter(|c| match self.docker_filter {
                DockerFilter::All => true,
                DockerFilter::Running => c.running,
                DockerFilter::Stopped => !c.running,
            })
            .collect();

        let filter_btn = |id: &'static str, kind: DockerFilter, label: String| {
            let active = self.docker_filter == kind;
            div()
                .id(id)
                .flex_1()
                .py(px(4.))
                .rounded(px(5.))
                .cursor_pointer()
                .text_center()
                .text_size(px(11.))
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.docker_filter = kind;
                    cx.notify();
                }))
                .child(label)
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(38.))
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
                            .child(crate::file_icons::FileIconKind::Docker.render(px(14.)))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .child(format!("Docker · {host_name}")),
                            ),
                    )
                    .child(
                        Button::new("close-docker-drawer")
                            .ghost()
                            .xsmall()
                            .icon(Icon::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.right_drawer = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("docker-drawer-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(7.))
                            .bg(colors.surface_glass())
                            .border_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.))
                            .text_color(rgb(colors.secondary))
                            .child(format!(
                                "{} {} · {} {} · {} {}",
                                language.text("Total", "共"),
                                containers.len(),
                                running_count,
                                language.text("running", "运行中"),
                                stopped_count,
                                language.text("stopped", "已停止")
                            ))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(rgb(colors.muted))
                                    .child(language.text("Read-only", "只读监控")),
                            ),
                    )
                    .child(
                        div()
                            .p(px(2.))
                            .rounded(px(7.))
                            .bg(colors.surface_glass())
                            .border_1()
                            .border_color(colors.hairline())
                            .flex()
                            .gap(px(2.))
                            .child(filter_btn(
                                "dk-all",
                                DockerFilter::All,
                                format!("{} ({})", language.text("All", "全部"), containers.len()),
                            ))
                            .child(filter_btn(
                                "dk-run",
                                DockerFilter::Running,
                                format!("{} ({running_count})", language.text("Running", "运行中")),
                            ))
                            .child(filter_btn(
                                "dk-stop",
                                DockerFilter::Stopped,
                                format!("{} ({stopped_count})", language.text("Stopped", "已停止")),
                            )),
                    )
                    .children(filtered.is_empty().then(|| {
                        div()
                            .py(px(28.))
                            .text_center()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(if telemetry.is_none() {
                                language.text("Collecting container telemetry…", "正在采集容器状态…")
                            } else {
                                language.text("No Docker containers found", "未检测到 Docker 容器")
                            })
                    }))
                    .children(filtered.into_iter().enumerate().map(|(idx, c)| {
                        let dot_color = if c.running { colors.success } else { colors.muted };
                        let meta_line = if c.ports.is_empty() {
                            c.image.clone()
                        } else {
                            format!("{} · {}", c.image, c.ports)
                        };
                        let cpu_text = if c.running {
                            format!("{:.1}%", c.cpu_pct)
                        } else {
                            "—".into()
                        };
                        let mem_text = if c.running {
                            c.mem_used.clone()
                        } else {
                            "—".into()
                        };
                        let cpu_bar_w = if c.running {
                            (c.cpu_pct * 2.0).clamp(2.0, 100.0)
                        } else {
                            0.0
                        };
                        let mem_bar_w = if c.running {
                            c.mem_pct.clamp(2.0, 100.0)
                        } else {
                            0.0
                        };
                        let log_host = host_name.to_owned();
                        let log_name = c.name.clone();
                        let log_running = c.running;

                        div()
                            .p(px(9.))
                            .rounded(px(8.))
                            .bg(colors.surface_glass())
                            .border_1()
                            .border_color(colors.hairline())
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .size(px(6.5))
                                                    .rounded_full()
                                                    .flex_shrink_0()
                                                    .bg(rgb(dot_color)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(colors.foreground))
                                                    .truncate()
                                                    .child(c.name.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .flex_shrink_0()
                                            .child(
                                                div()
                                                    .text_size(px(10.5))
                                                    .text_color(rgb(colors.muted))
                                                    .child(c.status.clone()),
                                            )
                                            .child(
                                                Button::new(("dk-logs", idx))
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(Icon::List)
                                                    .label(language.text("Logs", "日志"))
                                                    .on_click(cx.listener(move |this, _, window, cx| {
                                                        this.open_docker_logs(
                                                            log_host.clone(),
                                                            log_name.clone(),
                                                            log_running,
                                                            window,
                                                            cx,
                                                        );
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(colors.secondary))
                                    .truncate()
                                    .child(meta_line),
                            )
                            .child(
                                div()
                                    .pt(px(5.))
                                    .border_t_1()
                                    .border_color(colors.hairline())
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .justify_between()
                                                    .text_size(px(10.5))
                                                    .child(div().text_color(rgb(colors.secondary)).child("CPU"))
                                                    .child(
                                                        div()
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(colors.foreground))
                                                            .child(cpu_text),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .h(px(4.))
                                                    .w_full()
                                                    .rounded_full()
                                                    .bg(colors.surface_active_glass())
                                                    .overflow_hidden()
                                                    .child(
                                                        div()
                                                            .h_full()
                                                            .rounded_full()
                                                            .bg(rgb(colors.accent))
                                                            .w(gpui_kit::relative(cpu_bar_w / 100.0)),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .justify_between()
                                                    .text_size(px(10.5))
                                                    .child(div().text_color(rgb(colors.secondary)).child("MEM"))
                                                    .child(
                                                        div()
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(colors.foreground))
                                                            .child(mem_text),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .h(px(4.))
                                                    .w_full()
                                                    .rounded_full()
                                                    .bg(colors.surface_active_glass())
                                                    .overflow_hidden()
                                                    .child(
                                                        div()
                                                            .h_full()
                                                            .rounded_full()
                                                            .bg(rgb(colors.success))
                                                            .w(gpui_kit::relative(mem_bar_w / 100.0)),
                                                    ),
                                            ),
                                    ),
                            )
                    })),
            )
    }

    fn open_docker_logs(
        &mut self,
        host_name: String,
        container_name: String,
        container_running: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.active_terminal() else {
            return;
        };
        let placeholder = self.language.text("Filter logs…", "过滤日志…");
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let _search_sub = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);

        self.docker_logs = Some(DockerLogsState {
            session,
            host_name,
            container_name,
            container_running,
            tail: 500,
            timestamps: false,
            loading: true,
            error: None,
            logs: String::new(),
            search,
            _search_sub,
            request_seq: 0,
            focus,
        });
        self.refresh_docker_logs(window, cx);
    }

    fn refresh_docker_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.docker_logs.as_mut() else {
            return;
        };
        state.request_seq = state.request_seq.wrapping_add(1);
        let seq = state.request_seq;
        state.loading = true;
        state.error = None;
        let container = state.container_name.clone();
        let tail = state.tail;
        let timestamps = state.timestamps;
        let rx = state
            .session
            .read(cx)
            .fetch_docker_logs(container, tail, timestamps);

        let Some(rx) = rx else {
            state.loading = false;
            state.error = Some(
                self.language
                    .text(
                        "Docker logs are only available on active SSH sessions.",
                        "仅在活跃的 SSH 会话中支持查看容器日志。",
                    )
                    .into(),
            );
            cx.notify();
            return;
        };

        cx.spawn_in(window, async move |ws, cx| {
            let timeout = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(20));
            let result = tokio::select! {
                biased;
                reply = rx => reply.unwrap_or_else(|_| {
                    Err("SSH session closed while fetching logs.".to_owned())
                }),
                _ = timeout => Err("Timed out fetching Docker logs.".to_owned()),
            };
            let _ = ws.update(cx, |this, cx| {
                let Some(state) = this.docker_logs.as_mut() else {
                    return;
                };
                if state.request_seq != seq {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(logs) => {
                        state.logs = logs;
                        state.error = None;
                    }
                    Err(err) => {
                        state.error = Some(err);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn close_docker_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.docker_logs = None;
        if let Some(term) = self.active_terminal() {
            let handle = term.read(cx).focus_handle();
            handle.focus(window, cx);
        }
        cx.notify();
    }

    fn render_docker_logs_modal(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let state = self.docker_logs.as_ref()?;
        let language = self.language;
        let dot_color = if state.container_running {
            colors.success
        } else {
            colors.muted
        };
        let query = state.search.read(cx).value().trim().to_lowercase();
        let all_lines: Vec<&str> = if state.logs.is_empty() {
            Vec::new()
        } else {
            state.logs.lines().collect()
        };
        let total_lines = all_lines.len();
        let filtered_lines: Vec<(usize, String)> = all_lines
            .into_iter()
            .enumerate()
            .filter(|(_, line)| query.is_empty() || line.to_lowercase().contains(&query))
            .map(|(idx, line)| (idx + 1, line.to_owned()))
            .collect();
        let shown_lines = filtered_lines.len();
        let copy_text = state.logs.clone();

        let tail_btn = |tail: usize| {
            let active = state.tail == tail;
            div()
                .id(("dk-log-tail", tail))
                .px(px(7.))
                .py(px(3.))
                .rounded(px(5.))
                .cursor_pointer()
                .text_size(px(11.))
                .font_weight(if active {
                    FontWeight::SEMIBOLD
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
                .hover(|s| s.bg(colors.surface_hover_glass()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(st) = this.docker_logs.as_mut()
                        && st.tail != tail
                    {
                        st.tail = tail;
                        this.refresh_docker_logs(window, cx);
                    }
                }))
                .child(format!("{tail}"))
        };

        let ts_active = state.timestamps;
        let modal = div()
            .id("docker-logs-modal")
            .track_focus(&state.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_docker_logs(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(860.))
            .max_w(gpui_kit::relative(0.88))
            .h(px(580.))
            .max_h(gpui_kit::relative(0.84))
            .rounded(px(12.))
            .bg(colors.popover_bg())
            .border_1()
            .border_color(colors.hairline_strong())
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(42.))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(crate::file_icons::FileIconKind::Docker.render(px(15.)))
                            .child(
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(rgb(dot_color)),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .truncate()
                                    .child(state.container_name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.muted))
                                    .truncate()
                                    .child(format!("· {}", state.host_name)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .child(
                                div()
                                    .p(px(2.))
                                    .rounded(px(6.))
                                    .bg(colors.surface_glass())
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .child(tail_btn(100))
                                    .child(tail_btn(500))
                                    .child(tail_btn(2000)),
                            )
                            .child(
                                div()
                                    .id("dk-log-timestamps")
                                    .px(px(8.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .border_1()
                                    .border_color(if ts_active {
                                        colors.accent_soft()
                                    } else {
                                        colors.hairline()
                                    })
                                    .bg(if ts_active {
                                        colors.accent_soft()
                                    } else {
                                        colors.surface_glass()
                                    })
                                    .text_size(px(11.))
                                    .text_color(rgb(if ts_active {
                                        colors.accent
                                    } else {
                                        colors.secondary
                                    }))
                                    .hover(|s| s.bg(colors.surface_hover_glass()))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(st) = this.docker_logs.as_mut() {
                                            st.timestamps = !st.timestamps;
                                            this.refresh_docker_logs(window, cx);
                                        }
                                    }))
                                    .child(language.text("Timestamps", "时间戳")),
                            )
                            .child(
                                Button::new("dk-log-refresh")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::Refresh)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.refresh_docker_logs(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("dk-log-copy")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::Copy)
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        if !copy_text.is_empty() {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy_text.clone(),
                                            ));
                                            crate::notify::success(
                                                language.text(
                                                    "Copied container logs",
                                                    "已复制容器日志",
                                                ),
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .child(
                                Button::new("dk-log-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::Close)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_docker_logs(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(14.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&state.search).xsmall()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors.muted))
                            .flex_shrink_0()
                            .child(if state.loading {
                                language.text("Loading…", "加载中…").to_owned()
                            } else if query.is_empty() {
                                format!("{total_lines} {}", language.text("lines", "行"))
                            } else {
                                format!(
                                    "{shown_lines} / {total_lines} {}",
                                    language.text("lines", "行")
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .id("docker-logs-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(12.))
                    .font_family(self.font_family.clone())
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .children(state.loading.then(|| {
                        div()
                            .py(px(40.))
                            .text_center()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(language.text("Fetching container logs…", "正在获取容器日志…"))
                    }))
                    .children((!state.loading && state.error.is_some()).then(|| {
                        let err = state.error.clone().unwrap_or_default();
                        div()
                            .py(px(36.))
                            .px(px(20.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors.danger))
                                    .child(err),
                            )
                            .child(
                                Button::new("dk-log-retry")
                                    .secondary()
                                    .xsmall()
                                    .h(px(24.))
                                    .px(px(8.))
                                    .icon(Icon::Refresh)
                                    .label(language.text("Retry", "重试"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.refresh_docker_logs(window, cx);
                                    })),
                            )
                    }))
                    .children(
                        (!state.loading && state.error.is_none() && filtered_lines.is_empty()).then(
                            || {
                                div()
                                    .py(px(40.))
                                    .text_center()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors.muted))
                                    .child(if total_lines == 0 {
                                        language.text("No log output", "暂无日志输出")
                                    } else {
                                        language.text("No matching log lines", "没有匹配的日志行")
                                    })
                            },
                        ),
                    )
                    .children(
                        (!state.loading && state.error.is_none()).then(|| {
                            div().flex().flex_col().children(filtered_lines.into_iter().map(
                                |(line_no, line)| {
                                    let lower = line.to_lowercase();
                                    let line_color = if lower.contains("error")
                                        || lower.contains("fatal")
                                        || lower.contains("panic")
                                    {
                                        colors.danger
                                    } else if lower.contains("warn") {
                                        colors.warning()
                                    } else {
                                        colors.foreground
                                    };
                                    div()
                                        .flex()
                                        .items_start()
                                        .gap(px(10.))
                                        .px(px(4.))
                                        .py(px(1.))
                                        .rounded(px(3.))
                                        .hover(|s| s.bg(colors.surface_hover_glass()))
                                        .child(
                                            div()
                                                .w(px(38.))
                                                .flex_shrink_0()
                                                .text_right()
                                                .text_size(px(10.5))
                                                .text_color(rgb(colors.muted))
                                                .child(format!("{line_no}")),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_color(rgb(line_color))
                                                .child(line),
                                        )
                                },
                            ))
                        }),
                    ),
            );

        Some(
            div()
                .id("docker-logs-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x00000078))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.close_docker_logs(window, cx);
                }))
                .child(modal),
        )
    }

    fn telemetry_popover(
        &self,
        host_name: &str,
        tele: &ServerTelemetry,
        colors: Colors,
    ) -> impl IntoElement {
        let mem_pct = if tele.mem_total_mb > 0 {
            ((tele.mem_used_mb * 100) / tele.mem_total_mb).min(100) as f32
        } else {
            0.0
        };
        let mem_label = format!(
            "{:.1} / {:.1} GB",
            tele.mem_used_mb as f64 / 1024.0,
            tele.mem_total_mb as f64 / 1024.0
        );
        let sub_text = {
            let uptime = compact_uptime(&tele.uptime);
            if uptime.is_empty() {
                format!("{}ms", tele.latency_ms)
            } else {
                format!("up {uptime} · {}ms", tele.latency_ms)
            }
        };

        div()
            .id("telemetry-popover")
            .absolute()
            .bottom(px(34.))
            .left(px(120.))
            .w(px(300.))
            .p(px(14.))
            .rounded(px(11.))
            .bg(colors.popover_bg())
            .border_1()
            .border_color(colors.hairline_strong())
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(format!("{host_name} 实时概览")),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(165.))
                            .truncate()
                            .text_size(px(10.5))
                            .text_color(rgb(colors.muted))
                            .child(sub_text),
                    ),
            )
            .child(tele_bar("CPU", &format!("{}%", tele.cpu_pct), f32::from(tele.cpu_pct), colors.accent, colors))
            .child(tele_bar("Memory", &mem_label, mem_pct, colors.success, colors))
            .child(tele_bar(
                "Disk /",
                &format!("{}% ({})", tele.disk_pct, tele.disk_used),
                f32::from(tele.disk_pct),
                colors.warning(),
                colors,
            ))
            .child(
                div()
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.))
                    .child(div().text_color(rgb(colors.muted)).child("Load Average"))
                    .child(
                        div()
                            .text_color(rgb(colors.foreground))
                            .child(if tele.load_avg.is_empty() {
                                "—".to_owned()
                            } else {
                                tele.load_avg.clone()
                            }),
                    ),
            )
    }

    fn session_status_bar(&self, colors: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let vibrancy = self.vibrancy;
        let active_term = self.active_terminal();
        let can_sftp = self.file_drawer_terminal(cx).is_some();
        let (is_ssh, connected, host_label, telemetry, is_split, tunnel_count, has_host) =
            match active_term {
            Some(ref term_entity) => {
                let t = term_entity.read(cx);
                let is_ssh = t.is_ssh();
                let connected = t.is_connected();
                let host_opt = t.host_profile();
                let label = match host_opt {
                    Some(h) => format!("{} · {}", h.protocol.label(), h.name),
                    None => language.text("Local · Shell", "本地终端").to_owned(),
                };
                let t_count = host_opt
                    .map(|h| self.tunnels.read(cx).active_count_for_host(h.id, cx))
                    .unwrap_or(0);
                let split = match self.tabs.get(self.active) {
                    Some(Tab {
                        kind: TabKind::Terminal { panes, .. },
                        ..
                    }) => panes.len() > 1,
                    _ => false,
                };
                (
                    is_ssh,
                    connected,
                    label,
                    t.telemetry().cloned(),
                    split,
                    t_count,
                    host_opt.is_some(),
                )
            }
            None => (false, false, "SFTP".into(), None, false, 0, false),
        };

        let dot_color = if is_ssh && connected {
            colors.success
        } else if is_ssh {
            colors.warning()
        } else {
            colors.muted
        };

        let tele_pill = if let Some(tele) = telemetry.as_ref() {
            let mem_str = if tele.mem_total_mb > 0 {
                format!(
                    "{:.1}/{:.0}G",
                    tele.mem_used_mb as f64 / 1024.0,
                    tele.mem_total_mb as f64 / 1024.0
                )
            } else {
                "—".into()
            };
            Some(
                div()
                    .id("status-tele-pill")
                    .px(px(7.))
                    .py(px(1.5))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .bg(if self.telemetry_open {
                        colors.accent_soft()
                    } else {
                        transparent_black().into()
                    })
                    .hover(|s| s.bg(colors.surface_hover_glass()))
                    .text_size(px(11.))
                    .text_color(rgb(if self.telemetry_open {
                        colors.accent
                    } else {
                        colors.secondary
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.telemetry_open = !this.telemetry_open;
                        cx.notify();
                    }))
                    .child(Icon::Cpu.render(px(11.5), colors.accent))
                    .child(format!(
                        "{}ms · CPU {}% · MEM {} · DISK {}%",
                        tele.latency_ms, tele.cpu_pct, mem_str, tele.disk_pct
                    )),
            )
        } else if is_ssh && connected {
            Some(
                div()
                    .id("status-tele-pill")
                    .px(px(7.))
                    .py(px(1.5))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(11.))
                    .text_color(rgb(colors.muted))
                    .child(Icon::Cpu.render(px(11.5), colors.muted))
                    .child(language.text("Monitoring…", "采集监控中…")),
            )
        } else {
            None
        };

        let docker_count = telemetry
            .as_ref()
            .map(|t| t.containers.iter().filter(|c| c.running).count())
            .unwrap_or(0);
        let docker_label = if docker_count > 0 {
            format!("Docker ({docker_count})")
        } else {
            "Docker".into()
        };

        let tunnels_title = language.text("Tunnels", "端口转发");
        let tunnels_label = if tunnel_count > 0 {
            format!("{tunnels_title} ({tunnel_count})")
        } else {
            tunnels_title.to_owned()
        };

        div()
            .h(px(26.))
            .w_full()
            .px(px(12.))
            .bg(colors.titlebar_bg(vibrancy, self.opacity))
            .border_t_1()
            .border_color(colors.hairline())
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(11.))
            .text_color(rgb(colors.secondary))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().size(px(6.)).rounded_full().bg(rgb(dot_color)))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(host_label),
                            ),
                    )
                    .children(tele_pill),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        status_pill_btn(
                            "sb-split",
                            Some(Icon::Split),
                            None,
                            language.text("Split", "分屏"),
                            is_split,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_split_pane(window, cx);
                        })),
                    )
                    .children(is_ssh.then(|| {
                        let active = self.right_drawer == Some(RightDrawer::Docker);
                        status_pill_btn_with(
                            "sb-docker",
                            Some(
                                crate::file_icons::FileIconKind::Docker
                                    .render(px(12.))
                                    .into_any_element(),
                            ),
                            docker_label,
                            active,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_right_drawer(RightDrawer::Docker, window, cx);
                        }))
                    }))
                    .children(can_sftp.then(|| {
                        status_pill_btn(
                            "sb-sftp",
                            Some(Icon::Folder),
                            Some(0x32ade6),
                            language.text("SFTP", "文件传输"),
                            self.right_drawer == Some(RightDrawer::Files),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_right_drawer(RightDrawer::Files, window, cx);
                        }))
                    }))
                    .child(
                        status_pill_btn(
                            "sb-snippets",
                            Some(Icon::Code),
                            Some(0xaf52de),
                            language.text("Snippets", "命令片段"),
                            self.right_drawer == Some(RightDrawer::Snippets),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_right_drawer(RightDrawer::Snippets, window, cx);
                        })),
                    )
                    .children(has_host.then(|| {
                        status_pill_btn(
                            "sb-tunnels",
                            Some(Icon::Network),
                            Some(0x34c759),
                            tunnels_label,
                            self.right_drawer == Some(RightDrawer::Tunnels),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_tunnels_drawer(window, cx);
                        }))
                    })),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors();
        let vibrancy = self.vibrancy;

        let body = if self.in_vault {
            let main_view = match self.vault_section {
                VaultSection::Hosts => self.hosts.clone().into_any_element(),
                VaultSection::Keychain => self.keychain.clone().into_any_element(),
                VaultSection::Tunnels => self.tunnels.clone().into_any_element(),
                VaultSection::Snippets => self.snippets.clone().into_any_element(),
                VaultSection::Settings => self.preferences.clone().into_any_element(),
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .overflow_hidden()
                .child(self.vault_sidebar(colors, cx))
                .child(div().flex_1().min_w_0().min_h_0().overflow_hidden().child(main_view))
                .into_any_element()
        } else if let Some(tab) = self.tabs.get(self.active) {
            match &tab.kind {
                TabKind::Terminal { panes, active_pane } => {
                    let active_term = panes
                        .get(*active_pane)
                        .or_else(|| panes.first())
                        .map(|p| p.terminal.clone());
                    let host_profile = active_term
                        .as_ref()
                        .and_then(|t| t.read(cx).host_profile().cloned());
                    let telemetry = active_term
                        .as_ref()
                        .and_then(|t| t.read(cx).telemetry().cloned());
                    let is_split = panes.len() > 1;
                    let active_idx = *active_pane;

                    let panes_row = div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .bg(colors.term_bg_for(
                            self.theme.palette().background,
                            vibrancy,
                            self.opacity,
                        ))
                        .flex()
                        .overflow_hidden()
                        .children(panes.iter().enumerate().map(|(pidx, pane)| {
                            let is_focused = pidx == active_idx;
                            let term = pane.terminal.clone();
                            let mut pane_div = div()
                                .id(("term-pane", pidx))
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, window, cx| {
                                        if let Some(Tab {
                                            kind: TabKind::Terminal { panes, active_pane },
                                            ..
                                        }) = this.tabs.get_mut(this.active)
                                        {
                                            *active_pane = pidx;
                                            if let Some(p) = panes.get(pidx) {
                                                p.terminal
                                                    .read(cx)
                                                    .focus_handle()
                                                    .focus(window, cx);
                                            }
                                            cx.notify();
                                        }
                                    }),
                                )
                                .child(term);
                            if pidx > 0 {
                                pane_div = pane_div.border_l_1().border_color(colors.hairline());
                            }
                            if is_split && !is_focused {
                                pane_div = pane_div.opacity(0.82);
                            }
                            pane_div
                        }));

                    let mut main_row = div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .overflow_hidden()
                        .child(panes_row);

                    let drawer_content = match self.right_drawer {
                        Some(RightDrawer::Files) => Some(self.file_drawer.clone().into_any_element()),
                        Some(RightDrawer::Snippets) => {
                            Some(self.snippets_drawer(colors, cx).into_any_element())
                        }
                        Some(RightDrawer::Tunnels) => host_profile
                            .as_ref()
                            .map(|host| self.tunnels_drawer(host, colors, cx).into_any_element()),
                        Some(RightDrawer::Docker) => {
                            let host_name = host_profile
                                .as_ref()
                                .map(|h| h.name.as_str())
                                .unwrap_or("Host");
                            Some(
                                self.docker_drawer(
                                    host_name,
                                    telemetry.as_ref(),
                                    colors,
                                    self.language,
                                    cx,
                                )
                                .into_any_element(),
                            )
                        }
                        None => None,
                    };

                    if let Some(drawer_el) = drawer_content {
                        main_row = main_row.child(
                            div()
                                .id("right-drawer-overlay")
                                .occlude()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right_0()
                                .w(px(306.))
                                .bg(colors.popover_bg())
                                .border_l_1()
                                .border_color(colors.hairline_strong())
                                .shadow_lg()
                                .flex()
                                .flex_col()
                                .overflow_hidden()
                                .child(drawer_el),
                        );
                    }

                    let tele_pop = self
                        .telemetry_open
                        .then(|| {
                            telemetry.as_ref().map(|tele| {
                                let name = host_profile
                                    .as_ref()
                                    .map(|h| h.name.as_str())
                                    .unwrap_or("Host");
                                self.telemetry_popover(name, tele, colors)
                            })
                        })
                        .flatten();

                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(main_row)
                        .children(tele_pop)
                        .children(
                            host_profile
                                .is_some()
                                .then(|| self.session_status_bar(colors, cx)),
                        )
                        .into_any_element()
                }
                TabKind::Sftp { view, .. } => div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(view.clone())
                    .into_any_element(),
            }
        } else {
            div().flex_1().into_any_element()
        };

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
            .on_action(cx.listener(|this, _: &ShowVault, _, cx| {
                this.open_vault(VaultSection::Hosts, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &SplitPane, window, cx| {
                this.toggle_split_pane(window, cx);
                cx.stop_propagation();
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
                if let Some(terminal) = this.active_terminal() {
                    terminal.update(cx, |terminal, cx| terminal.toggle_search(window, cx));
                    cx.stop_propagation();
                }
            }))
            .on_action(
                cx.listener(|this, _: &CloseWindow, window, cx| this.close_window(window, cx)),
            )
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &ToggleFullscreen, window, _| window.toggle_fullscreen())
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_size(px(13.))
            .bg(colors.window_bg(vibrancy, self.opacity))
            .text_color(rgb(colors.foreground))
            .child(self.titlebar(colors, cx))
            .child(body);

        if self.new_tab_menu_open {
            root = root.child(self.render_new_tab_menu(colors, cx));
        }
        if self.quick_connect_open {
            root = root.child(self.quick_connect.clone());
        }
        if let Some(logs_modal) = self.render_docker_logs_modal(colors, cx) {
            root = root.child(logs_modal);
        }
        root
    }
}

fn vault_nav_item(
    id: impl Into<ElementId>,
    icon: Icon,
    label: impl Into<SharedString>,
    badge: Option<String>,
    active: bool,
    colors: Colors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let label = label.into();
    let mut row = div()
        .id(id)
        .h(px(28.))
        .px(px(8.))
        .rounded(px(6.))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(8.))
        .bg(if active {
            colors.accent_soft()
        } else {
            transparent_black().into()
        })
        .hover(|s| s.bg(colors.surface_hover_glass()))
        .child(icon.badge(px(20.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.5))
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
                .child(label),
        );

    if let Some(count) = badge {
        row = row.child(
            div()
                .text_size(px(11.))
                .text_color(rgb(if active { colors.accent } else { colors.muted }))
                .child(count),
        );
    }
    row
}

fn status_pill_btn(
    id: impl Into<ElementId>,
    icon: Option<Icon>,
    icon_color: Option<u32>,
    label: impl Into<SharedString>,
    active: bool,
    colors: Colors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let resolved_icon_color = if active {
        colors.accent
    } else {
        icon_color.unwrap_or(colors.secondary)
    };
    let icon_el = icon.map(|ic| ic.render(px(11.5), resolved_icon_color).into_any_element());
    status_pill_btn_with(id, icon_el, label, active, colors)
}

fn status_pill_btn_with(
    id: impl Into<ElementId>,
    icon: Option<gpui_kit::AnyElement>,
    label: impl Into<SharedString>,
    active: bool,
    colors: Colors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let label = label.into();
    div()
        .id(id)
        .px(px(7.))
        .py(px(1.5))
        .rounded(px(5.))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(4.))
        .bg(if active {
            colors.accent_soft()
        } else {
            transparent_black().into()
        })
        .hover(|s| s.bg(colors.surface_hover_glass()))
        .text_size(px(11.))
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
        .children(icon)
        .child(label)
}

fn compact_uptime(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = trimmed
        .split(',')
        .filter_map(|seg| {
            let mut it = seg.split_whitespace();
            let num = it.next()?;
            let unit = it.next()?;
            let short = match unit.trim_end_matches('s') {
                "year" => "y",
                "week" => "w",
                "day" => "d",
                "hour" => "h",
                "minute" => "m",
                "second" => "s",
                _ => return None,
            };
            Some(format!("{num}{short}"))
        })
        .take(3)
        .collect();
    if parts.is_empty() {
        trimmed.to_owned()
    } else {
        parts.join(" ")
    }
}

fn tele_bar(
    label: &str,
    value: &str,
    pct: f32,
    bar_color: u32,
    colors: Colors,
) -> impl IntoElement {
    let fill_w = (pct.clamp(0.0, 100.0) / 100.0) * 270.0;
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(11.))
                .child(div().text_color(rgb(colors.secondary)).child(label.to_owned()))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors.foreground))
                        .child(value.to_owned()),
                ),
        )
        .child(
            div()
                .w(px(270.))
                .h(px(5.))
                .rounded_full()
                .bg(colors.surface_active_glass())
                .overflow_hidden()
                .child(div().h_full().w(px(fill_w)).rounded_full().bg(rgb(bar_color))),
        )
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
