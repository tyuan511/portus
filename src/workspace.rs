use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use gpui_kit::{
    AppContext as _, ClipboardItem, Context, DragMoveEvent, ElementId, Entity, FocusHandle,
    FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    ParentElement as _, Pixels, Point, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
    },
    div, px, rgb, rgba, transparent_black,
};
use gpui_terminal::TerminalProgress;

use crate::agent_bus::{AgentBus, AgentControlCommand, AgentSessionSnapshot};
use crate::file_drawer::{FileDrawer, FileDrawerEvent};
use crate::icons::Icon;
use crate::mcp::{McpRuntime, McpUiEvent};
use crate::platform::{apply_window_style, drag_window, hide_traffic_lights};
use crate::preferences::{
    HostsEvent, HostsPage, KeychainPage, Preferences, PreferencesEvent, SnippetsEvent, SnippetsPage,
    TunnelsPage,
};
use crate::quick_connect::{QuickConnectEvent, QuickConnectPalette};
use crate::settings::{
    HostProfile, HostProtocol, Language, SavedHostRef, SavedPane, SavedSession, SavedTab,
    SettingsStore, Snippet,
};
use crate::sftp_view::{SftpView, SftpViewEvent};
use crate::ssh::{ServerTelemetry, supports_sftp};
use crate::sync::{SyncManager, SyncUiEvent, apply_remote_settings, compute_snapshot_hash};
use crate::terminal::{AgentActivity, TerminalConfig, TerminalSession, TerminalSessionEvent};
use crate::theme::{Colors, Theme};
use crate::updater::{CURRENT_VERSION, UpdateManager, UpdateStatus, UpdateUiEvent};
use crate::{
    CheckForUpdates, CloseTab, CloseWindow, FindInTerminal, JumpNextAttention, JumpNextPrompt,
    JumpPrevPrompt, MinimizeWindow, NewGitWorktree, NewTab, NextTab, PreviousTab, SelectTab,
    ShowHosts, ShowSettings, ShowSnippets, ShowTunnels, ShowVault, SplitPane, ToggleFilesDrawer,
    ToggleFullscreen, ToggleQuickConnect, ToggleSnippetsDrawer,
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
    _events: Subscription,
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

#[derive(Clone)]
struct DraggedTab {
    tab_id: u64,
    label: SharedString,
    dot_color: u32,
    colors: Colors,
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(26.))
            .min_w(px(120.))
            .max_w(px(210.))
            .pl(px(10.))
            .pr(px(10.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(7.))
            .bg(self.colors.surface_active_glass())
            .border_1()
            .border_color(self.colors.hairline_strong())
            .shadow_md()
            .opacity(0.92)
            .child(
                div()
                    .size(px(6.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(rgb(self.dot_color)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(self.colors.foreground))
                    .child(self.label.clone()),
            )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TabDropTarget {
    target_id: u64,
    insert_after: bool,
}

pub struct Workspace {
    focus: FocusHandle,
    in_vault: bool,
    vault_section: VaultSection,
    selected_group: Option<String>,
    tabs: Vec<Tab>,
    active: usize,
    next_tab: u64,
    tab_scroll: ScrollHandle,
    dragging_tab_id: Option<u64>,
    tab_drop_target: Option<TabDropTarget>,
    settings: Entity<SettingsStore>,
    hosts: Option<Entity<HostsPage>>,
    keychain: Option<Entity<KeychainPage>>,
    snippets: Option<Entity<SnippetsPage>>,
    tunnels: Option<Entity<TunnelsPage>>,
    mcp_runtime: Arc<McpRuntime>,
    agent_bus: Arc<AgentBus>,
    updater: Arc<UpdateManager>,
    sync_manager: Arc<SyncManager>,
    auto_sync_gen: u64,
    preferences: Option<Entity<Preferences>>,
    quick_connect: Option<Entity<QuickConnectPalette>>,
    file_drawer: Option<Entity<FileDrawer>>,
    quick_connect_open: bool,
    worktree_modal_open: bool,
    worktree_branch_input: Option<Entity<InputState>>,
    worktree_repo_root: Option<PathBuf>,
    _worktree_sub: Option<Subscription>,
    new_tab_menu_open: bool,
    new_tab_menu_pos: Option<Point<Pixels>>,
    right_drawer: Option<RightDrawer>,
    docker_filter: DockerFilter,
    docker_logs: Option<DockerLogsState>,
    telemetry_open: bool,
    theme: Theme,
    font_family: String,
    language: Language,
    opacity: u8,
    terminal_config: TerminalConfig,
    _settings_updates: Subscription,
    _hosts_events: Option<Subscription>,
    _snippets_events: Option<Subscription>,
    _preferences_events: Option<Subscription>,
    _quick_connect_events: Option<Subscription>,
    _file_drawer_events: Option<Subscription>,
    _window_bounds: Subscription,
}

impl Workspace {
    pub fn new(
        settings: Entity<SettingsStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = &settings.read(cx).current;
        let terminal_config = TerminalConfig::from_settings(saved);
        let (theme, font_family, language, opacity) = (
            saved.theme,
            saved.font_family.clone(),
            saved.language,
            saved.opacity,
        );
        let dark = theme.colors().dark;
        window.defer(cx, move |window, cx| {
            apply_window_style(window, opacity, dark, cx)
        });
        let window_bounds = cx.observe_window_bounds(window, |_this, window, _| {
            hide_traffic_lights(window);
        });
        let settings_updates = cx.observe_in(&settings, window, |this, settings, window, cx| {
            let saved = settings.read(cx).current.clone();
            this.mcp_runtime.sync_settings(saved.clone());
            let next_terminal_config = TerminalConfig::from_settings(&saved);
            let (theme, font_family, language, opacity) = (
                saved.theme,
                saved.font_family.clone(),
                saved.language,
                saved.opacity,
            );
            let prev_dark = this.theme.colors().dark;
            let next_dark = theme.colors().dark;
            this.theme = theme;
            this.font_family = font_family;
            this.language = language;
            if this.terminal_config != next_terminal_config {
                this.terminal_config = next_terminal_config.clone();
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
                    let cfg = next_terminal_config.clone();
                    terminal.update(cx, |terminal, cx| {
                        terminal.set_config(cfg, cx);
                    });
                }
            }
            if this.opacity != opacity || prev_dark != next_dark {
                this.opacity = opacity;
                apply_window_style(window, opacity, next_dark, cx);
            }
            this.sync_file_drawer(window, cx);
            if saved.sync.auto_sync && saved.sync.is_configured() {
                let current_hash = compute_snapshot_hash(&saved);
                if current_hash != saved.sync.last_sync_hash {
                    this.auto_sync_gen = this.auto_sync_gen.wrapping_add(1);
                    let expected_gen = this.auto_sync_gen;
                    let delay = cx
                        .background_executor()
                        .timer(std::time::Duration::from_secs(5));
                    cx.spawn_in(window, async move |ws, cx| {
                        delay.await;
                        let _ = cx.update(|_, cx| {
                            let _ = ws.update(cx, |this, cx| {
                                if this.auto_sync_gen != expected_gen {
                                    return;
                                }
                                let latest = this.settings.read(cx).current.clone();
                                if latest.sync.auto_sync
                                    && latest.sync.is_configured()
                                    && compute_snapshot_hash(&latest) != latest.sync.last_sync_hash
                                {
                                    this.sync_manager.push(latest, true);
                                }
                            });
                        });
                    })
                    .detach();
                }
            }
            cx.notify();
        });

        let (agent_cmd_tx, agent_cmd_rx) = async_channel::unbounded::<AgentControlCommand>();
        let agent_bus = AgentBus::start(agent_cmd_tx);
        let (mcp_tx, mcp_rx) = async_channel::unbounded::<McpUiEvent>();
        let mcp_runtime = McpRuntime::new(settings.read(cx).current.clone(), Some(mcp_tx));
        mcp_runtime.attach_agent_bus(Arc::clone(&agent_bus));

        cx.spawn_in(window, async move |ws, cx| {
            while let Ok(cmd) = agent_cmd_rx.recv().await {
                let _ = cx.update(|window, cx| {
                    let _ = ws.update(cx, |this, cx| {
                        this.handle_agent_command(cmd, window, cx);
                    });
                });
            }
        })
        .detach();
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
                        if let Some(preferences) = &this.preferences {
                            preferences.update(cx, |p, cx| p.notify_mcp(cx));
                        }
                        cx.notify();
                    });
                });
            }
        })
        .detach();
        let (update_tx, update_rx) = async_channel::unbounded::<UpdateUiEvent>();
        let updater = UpdateManager::new(Some(update_tx));
        let auto_check_updates = settings.read(cx).current.auto_check_updates;
        cx.spawn_in(window, async move |ws, cx| {
            while let Ok(ev) = update_rx.recv().await {
                let ws_for_action = ws.clone();
                let _ = cx.update(|window, cx| {
                    let _ = ws.update(cx, |this, cx| {
                        let lang = this.language;
                        match ev {
                            UpdateUiEvent::Changed => {}
                            UpdateUiEvent::UpdateFound { release, silent } => {
                                let viewing_settings =
                                    this.in_vault && this.vault_section == VaultSection::Settings;
                                if silent || !viewing_settings {
                                    let title = lang.text("Update Available", "发现新版本");
                                    let message = match lang {
                                        Language::English => format!(
                                            "Portus {} is available (current v{CURRENT_VERSION}).",
                                            release.tag_name
                                        ),
                                        Language::ChineseSimplified => format!(
                                            "Portus {} 已发布（当前 v{CURRENT_VERSION}）。",
                                            release.tag_name
                                        ),
                                    };
                                    let action_label = lang.text("View Update", "查看更新");
                                    let ws_action = ws_for_action.clone();
                                    crate::notify::update_action(
                                        title,
                                        message,
                                        action_label,
                                        move |window, cx| {
                                            let _ = ws_action.update(cx, |ws, cx| {
                                                ws.open_updates(window, cx);
                                            });
                                        },
                                        window,
                                        cx,
                                    );
                                }
                            }
                            UpdateUiEvent::UpToDate { silent } => {
                                if !silent {
                                    let msg = match lang {
                                        Language::English => {
                                            format!("Up to date: Portus v{CURRENT_VERSION}")
                                        }
                                        Language::ChineseSimplified => {
                                            format!("检查完成：当前已是最新版本 v{CURRENT_VERSION}")
                                        }
                                    };
                                    crate::notify::info(msg, window, cx);
                                }
                            }
                            UpdateUiEvent::ReadyToRestart { release } => {
                                let title = lang.text("Update Ready", "更新已就绪");
                                let message = match lang {
                                    Language::English => format!(
                                        "Portus {} has been downloaded and is ready to install.",
                                        release.tag_name
                                    ),
                                    Language::ChineseSimplified => format!(
                                        "Portus {} 已下载完成，重启应用即可完成安装。",
                                        release.tag_name
                                    ),
                                };
                                let action_label = lang.text("Restart to Update", "重启并安装");
                                let ws_action = ws_for_action.clone();
                                crate::notify::update_action(
                                    title,
                                    message,
                                    action_label,
                                    move |window, cx| {
                                        let _ = ws_action.update(cx, |ws, cx| {
                                            ws.install_update_and_quit(window, cx);
                                        });
                                    },
                                    window,
                                    cx,
                                );
                            }
                            UpdateUiEvent::InstallerDownloaded { release } => {
                                let msg = match lang {
                                    Language::English => format!(
                                        "Downloaded {} installer and opened it in Finder.",
                                        release.tag_name
                                    ),
                                    Language::ChineseSimplified => format!(
                                        "已下载 {} 安装包并在访达中打开。",
                                        release.tag_name
                                    ),
                                };
                                crate::notify::success(msg, window, cx);
                            }
                            UpdateUiEvent::CheckFailed { message, silent } => {
                                if !silent {
                                    crate::notify::error(message, window, cx);
                                }
                            }
                            UpdateUiEvent::InstallFailed { message } => {
                                crate::notify::error(message, window, cx);
                            }
                        }
                        if let Some(preferences) = &this.preferences {
                            preferences.update(cx, |p, cx| p.notify_updater(cx));
                        }
                        cx.notify();
                    });
                });
            }
        })
        .detach();

        if auto_check_updates {
            let updater_bg = Arc::clone(&updater);
            let delay = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(3));
            cx.background_spawn(async move {
                delay.await;
                updater_bg.check_for_updates(true);
            })
            .detach();
        }

        let (sync_tx, sync_rx) = async_channel::unbounded::<SyncUiEvent>();
        let sync_manager = SyncManager::new(Some(sync_tx));
        let initial_sync_settings = settings.read(cx).current.clone();
        cx.spawn_in(window, async move |ws, cx| {
            while let Ok(ev) = sync_rx.recv().await {
                let _ = cx.update(|window, cx| {
                    let _ = ws.update(cx, |this, cx| {
                        let lang = this.language;
                        match ev {
                            SyncUiEvent::Changed => {}
                            SyncUiEvent::TestSucceeded { message } => {
                                crate::notify::success(message, window, cx);
                            }
                            SyncUiEvent::TestFailed { message } => {
                                crate::notify::error(message, window, cx);
                            }
                            SyncUiEvent::Pushed {
                                timestamp,
                                content_hash,
                                device_id,
                                silent,
                            } => {
                                let _ = this.settings.update(cx, |store, cx| {
                                    let res = store.update(|s| {
                                        s.sync.last_sync_at = Some(timestamp);
                                        s.sync.last_sync_hash = content_hash;
                                        s.sync.device_id = device_id;
                                    });
                                    if res.is_ok() {
                                        cx.notify();
                                    }
                                });
                                if !silent {
                                    crate::notify::success(
                                        lang.text(
                                            "Configuration uploaded to cloud.",
                                            "配置已上传至云端。",
                                        ),
                                        window,
                                        cx,
                                    );
                                }
                            }
                            SyncUiEvent::Pulled {
                                settings: remote,
                                timestamp,
                                device_id,
                                strategy,
                                silent,
                            } => {
                                let mut apply_result = Ok(Default::default());
                                let save_res = this.settings.update(cx, |store, cx| {
                                    let res = store.update(|s| {
                                        apply_result = apply_remote_settings(s, *remote, strategy);
                                        if apply_result.is_ok() {
                                            s.sync.last_sync_at = Some(timestamp);
                                            s.sync.last_sync_hash = compute_snapshot_hash(s);
                                            s.sync.device_id = device_id;
                                        }
                                    });
                                    if res.is_ok() && apply_result.is_ok() {
                                        cx.notify();
                                    }
                                    res
                                });
                                match (save_res, apply_result) {
                                    (Ok(()), Ok(stats)) => {
                                        if let Some(preferences) = &this.preferences {
                                            preferences.update(cx, |p, cx| {
                                                p.sync_from_store(window, cx);
                                            });
                                        }
                                        if !silent {
                                            let msg = match lang {
                                                Language::English => format!(
                                                    "Synced from cloud ({} hosts, {} snippets, {} tunnels).",
                                                    stats.hosts, stats.snippets, stats.tunnels
                                                ),
                                                Language::ChineseSimplified => format!(
                                                    "已从云端拉取配置（{} 台主机、{} 个片段、{} 条隧道）。",
                                                    stats.hosts, stats.snippets, stats.tunnels
                                                ),
                                            };
                                            crate::notify::success(msg, window, cx);
                                        }
                                    }
                                    (Err(err), _) | (_, Err(err)) => {
                                        if !silent {
                                            crate::notify::error(err, window, cx);
                                        }
                                    }
                                }
                            }
                            SyncUiEvent::UpToDate { timestamp, silent } => {
                                let _ = this.settings.update(cx, |store, cx| {
                                    let res = store.update(|s| {
                                        s.sync.last_sync_at = Some(timestamp);
                                    });
                                    if res.is_ok() {
                                        cx.notify();
                                    }
                                });
                                if !silent {
                                    crate::notify::info(
                                        lang.text(
                                            "Configuration is already up to date.",
                                            "配置已是最新，无需同步。",
                                        ),
                                        window,
                                        cx,
                                    );
                                }
                            }
                            SyncUiEvent::Failed { message, silent } => {
                                if !silent {
                                    crate::notify::error(message, window, cx);
                                }
                            }
                        }
                        if let Some(preferences) = &this.preferences {
                            preferences.update(cx, |p, cx| p.notify_sync(cx));
                        }
                        cx.notify();
                    });
                });
            }
        })
        .detach();

        if initial_sync_settings.sync.auto_sync && initial_sync_settings.sync.is_configured() {
            let sync_bg = Arc::clone(&sync_manager);
            let delay = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(2));
            cx.background_spawn(async move {
                delay.await;
                sync_bg.sync_now(initial_sync_settings, true);
            })
            .detach();
        }

        let mut this = Self {
            focus: cx.focus_handle(),
            in_vault: true,
            vault_section: VaultSection::Hosts,
            selected_group: None,
            tabs: Vec::new(),
            active: 0,
            next_tab: 1,
            tab_scroll: ScrollHandle::new(),
            dragging_tab_id: None,
            tab_drop_target: None,
            settings,
            hosts: None,
            keychain: None,
            snippets: None,
            tunnels: None,
            mcp_runtime,
            agent_bus,
            updater,
            sync_manager,
            auto_sync_gen: 0,
            preferences: None,
            quick_connect: None,
            file_drawer: None,
            quick_connect_open: false,
            worktree_modal_open: false,
            worktree_branch_input: None,
            worktree_repo_root: None,
            _worktree_sub: None,
            new_tab_menu_open: false,
            new_tab_menu_pos: None,
            right_drawer: None,
            docker_filter: DockerFilter::All,
            docker_logs: None,
            telemetry_open: false,
            theme,
            font_family,
            language,
            opacity,
            terminal_config,
            _settings_updates: settings_updates,
            _hosts_events: None,
            _snippets_events: None,
            _preferences_events: None,
            _quick_connect_events: None,
            _file_drawer_events: None,
            _window_bounds: window_bounds,
        };
        this.restore_session(window, cx);
        this.sync_agent_bus(cx);
        this
    }

    fn snapshot_session(&self, cx: &Context<Self>) -> SavedSession {
        let tabs = self
            .tabs
            .iter()
            .filter_map(|tab| match &tab.kind {
                TabKind::Terminal { panes, active_pane } => {
                    let saved_panes: Vec<SavedPane> = panes
                        .iter()
                        .map(|pane| match pane.terminal.read(cx).host_profile() {
                            Some(host) => SavedPane::Host {
                                host: SavedHostRef::from_host(host),
                            },
                            None => SavedPane::Local,
                        })
                        .collect();
                    if saved_panes.is_empty() {
                        None
                    } else {
                        Some(SavedTab::Terminal {
                            panes: saved_panes,
                            active_pane: *active_pane,
                        })
                    }
                }
                TabKind::Sftp { view, .. } => Some(SavedTab::Sftp {
                    host: SavedHostRef::from_host(view.read(cx).host_profile()),
                }),
            })
            .collect();
        SavedSession {
            tabs,
            active: self.active,
            in_vault: self.in_vault,
        }
    }

    fn save_session(&self, cx: &Context<Self>) {
        let Some(dir) = self.settings.read(cx).config_dir().map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let session = self.snapshot_session(cx);
        let _ = session.save_to_dir(&dir);
    }

    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.settings.read(cx).config_dir().map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let Some(saved) = SavedSession::load_from_dir(&dir) else {
            return;
        };
        if saved.tabs.is_empty() {
            return;
        }

        let settings_snapshot = self.settings.read(cx).current.clone();
        let all_hosts: Vec<HostProfile> = settings_snapshot
            .hosts
            .iter()
            .map(|h| settings_snapshot.resolve_host_auth(h))
            .collect();
        let terminal_config = self.terminal_config.clone();

        for tab in saved.tabs {
            match tab {
                SavedTab::Terminal { panes, active_pane } => {
                    let mut restored_panes = Vec::new();
                    for pane in panes {
                        let terminal = match pane {
                            SavedPane::Local => {
                                let cfg = terminal_config.clone();
                                let shell = settings_snapshot.shell.clone();
                                cx.new(|cx| TerminalSession::local(cfg, &shell, cx))
                            }
                            SavedPane::Host { host } => {
                                let Some(host_profile) = host.resolve(&settings_snapshot.hosts)
                                else {
                                    continue;
                                };
                                let resolved = settings_snapshot.resolve_host_auth(&host_profile);
                                let cfg = terminal_config.clone();
                                let hosts_for_term = all_hosts.clone();
                                cx.new(|cx| {
                                    TerminalSession::ssh(cfg, resolved, hosts_for_term, window, cx)
                                })
                            }
                        };
                        let pane_entry = self.make_pane(terminal, window, cx);
                        restored_panes.push(pane_entry);
                    }
                    if !restored_panes.is_empty() {
                        let id = self.alloc_tab_id();
                        let active_pane = active_pane.min(restored_panes.len() - 1);
                        self.tabs.push(Tab {
                            id,
                            kind: TabKind::Terminal {
                                panes: restored_panes,
                                active_pane,
                            },
                        });
                    }
                }
                SavedTab::Sftp { host } => {
                    let Some(host_profile) = host.resolve(&settings_snapshot.hosts) else {
                        continue;
                    };
                    if !supports_sftp(&host_profile, &settings_snapshot.hosts) {
                        continue;
                    }
                    let resolved = settings_snapshot.resolve_host_auth(&host_profile);
                    let host_name = resolved.name.clone();
                    let view =
                        cx.new(|cx| SftpView::new(self.settings.clone(), resolved, window, cx));
                    let updates = cx.observe(&view, |_, _, cx| cx.notify());
                    let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| {
                        match event {
                            SftpViewEvent::ReturnToTerminal(host) => {
                                this.return_to_terminal(host.clone(), window, cx);
                            }
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
                }
            }
        }

        if !self.tabs.is_empty() {
            self.active = saved.active.min(self.tabs.len() - 1);
            self.in_vault = saved.in_vault;
            if !self.in_vault {
                self.restore_active_focus(window, cx);
                self.sync_file_drawer(window, cx);
            }
        }
        self.save_session(cx);
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

    fn make_pane(
        &self,
        terminal: Entity<TerminalSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TerminalPane {
        let updates = cx.observe(&terminal, |_, _, cx| cx.notify());
        let events = cx.subscribe_in(&terminal, window, |this, term, event, window, cx| {
            match event {
                TerminalSessionEvent::Notification { title, body } => {
                    this.on_terminal_notification(term, title.clone(), body.clone(), window, cx);
                }
                TerminalSessionEvent::CwdChanged => {
                    if this.active_terminal().is_some_and(|a| a == *term) {
                        this.sync_file_drawer(window, cx);
                    }
                    this.sync_agent_bus(cx);
                }
                TerminalSessionEvent::StateChanged => {
                    this.sync_agent_bus(cx);
                    cx.notify();
                }
                TerminalSessionEvent::OpenPath(path) => {
                    this.right_drawer = Some(RightDrawer::Files);
                    this.sync_file_drawer(window, cx);
                    let path_owned = path.clone();
                    this.ensure_file_drawer(window, cx)
                        .update(cx, |drawer, cx| drawer.focus_path(&path_owned, cx));
                    cx.notify();
                }
                TerminalSessionEvent::ToastInfo(msg) => {
                    crate::notify::info(msg.clone(), window, cx);
                }
                TerminalSessionEvent::ToastError(msg) => {
                    crate::notify::error(msg.clone(), window, cx);
                }
            }
        });
        TerminalPane {
            terminal,
            _updates: updates,
            _events: events,
        }
    }

    fn on_terminal_notification(
        &mut self,
        term: &Entity<TerminalSession>,
        title: Option<String>,
        body: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_focused = !self.in_vault
            && self.active_terminal().is_some_and(|active| active == *term)
            && crate::platform::is_app_active();
        let display_title = title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| term.read(cx).title().to_owned());
        term.update(cx, |t, cx| {
            t.apply_agent_notification(title, body.clone(), !is_focused, cx);
        });
        if !crate::platform::is_app_active() {
            crate::platform::send_os_notification(&display_title, &body);
        }
        self.sync_agent_bus(cx);
        cx.notify();
    }

    fn sync_agent_bus(&self, cx: &Context<Self>) {
        let mut snapshots: Vec<AgentSessionSnapshot> = Vec::new();
        for tab in &self.tabs {
            if let TabKind::Terminal { panes, .. } = &tab.kind {
                for pane in panes {
                    snapshots.push(pane.terminal.read(cx).snapshot());
                }
            }
        }
        self.agent_bus.update_sessions(snapshots);
    }

    fn find_terminal_by_session_id(
        &self,
        session_id: Option<&str>,
        cx: &Context<Self>,
    ) -> Option<Entity<TerminalSession>> {
        if let Some(id) = session_id {
            for tab in &self.tabs {
                if let TabKind::Terminal { panes, .. } = &tab.kind {
                    for pane in panes {
                        if pane.terminal.read(cx).session_id() == id {
                            return Some(pane.terminal.clone());
                        }
                    }
                }
            }
        }
        self.active_terminal()
    }

    fn handle_agent_command(
        &mut self,
        cmd: AgentControlCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match cmd {
            AgentControlCommand::Notify {
                session_id,
                title,
                body,
            } => {
                if let Some(term) = self.find_terminal_by_session_id(session_id.as_deref(), cx) {
                    self.on_terminal_notification(&term, title, body, window, cx);
                } else {
                    crate::platform::send_os_notification(
                        title.as_deref().unwrap_or("Portus"),
                        &body,
                    );
                }
            }
            AgentControlCommand::SetStatus {
                session_id,
                state,
                message,
                progress,
            } => {
                if let Some(term) = self.find_terminal_by_session_id(session_id.as_deref(), cx) {
                    term.update(cx, |t, cx| {
                        t.apply_agent_status(&state, message, progress, cx);
                    });
                    self.sync_agent_bus(cx);
                    cx.notify();
                }
            }
            AgentControlCommand::SplitPane { cwd } => {
                self.open_split_pane_with_cwd(cwd.map(PathBuf::from), window, cx);
            }
            AgentControlCommand::OpenFile { path } => {
                self.right_drawer = Some(RightDrawer::Files);
                self.sync_file_drawer(window, cx);
                self.ensure_file_drawer(window, cx)
                    .update(cx, |drawer, cx| drawer.focus_path(&path, cx));
                cx.notify();
            }
        }
    }

    fn unread_attention_count(&self, cx: &Context<Self>) -> usize {
        self.tabs
            .iter()
            .map(|tab| match &tab.kind {
                TabKind::Terminal { panes, .. } => panes
                    .iter()
                    .filter(|p| p.terminal.read(cx).unread_attention())
                    .count(),
                TabKind::Sftp { .. } => 0,
            })
            .sum()
    }

    fn jump_next_attention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let total = self.tabs.len();
        let start = if self.in_vault { 0 } else { self.active };
        for offset in 0..total {
            let idx = (start + offset) % total;
            if let TabKind::Terminal { panes, .. } = &self.tabs[idx].kind
                && let Some(pidx) = panes
                    .iter()
                    .position(|p| p.terminal.read(cx).unread_attention())
            {
                self.active = idx;
                self.in_vault = false;
                self.telemetry_open = false;
                let term = if let TabKind::Terminal { panes, active_pane } =
                    &mut self.tabs[idx].kind
                {
                    *active_pane = pidx;
                    panes.get(pidx).map(|p| p.terminal.clone())
                } else {
                    None
                };
                if let Some(term) = term {
                    term.update(cx, |t, cx| {
                        t.clear_attention(cx);
                        t.focus_handle().focus(window, cx);
                    });
                }
                self.sync_telemetry_active(cx);
                self.sync_file_drawer(window, cx);
                self.sync_agent_bus(cx);
                self.save_session(cx);
                cx.notify();
                return;
            }
        }
    }

    fn spawn_local_tab_in_cwd(
        &mut self,
        cwd: Option<PathBuf>,
        activate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cfg = self.terminal_config.clone();
        let shell = self.settings.read(cx).current.shell.clone();
        let terminal = cx.new(|cx| TerminalSession::local_with_cwd(cfg, &shell, cwd, cx));
        let pane = self.make_pane(terminal.clone(), window, cx);
        let id = self.alloc_tab_id();
        self.tabs.push(Tab {
            id,
            kind: TabKind::Terminal {
                panes: vec![pane],
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
        self.sync_agent_bus(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn spawn_local_tab(&mut self, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.spawn_local_tab_in_cwd(None, activate, window, cx);
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
        let cfg = self.terminal_config.clone();
        let terminal = cx.new(|cx| TerminalSession::ssh(cfg, resolved, all_hosts, window, cx));
        let pane = self.make_pane(terminal.clone(), window, cx);
        let id = self.alloc_tab_id();
        self.tabs.push(Tab {
            id,
            kind: TabKind::Terminal {
                panes: vec![pane],
                active_pane: 0,
            },
        });
        self.active = self.tabs.len() - 1;
        self.in_vault = false;
        self.telemetry_open = false;
        terminal.read(cx).focus_handle().focus(window, cx);
        self.sync_file_drawer(window, cx);
        self.sync_agent_bus(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn open_split_pane_with_cwd(
        &mut self,
        preferred_cwd: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.in_vault {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let TabKind::Terminal { panes, .. } = &tab.kind else {
            return;
        };
        if panes.len() >= 2 {
            return;
        }
        let (host_opt, inherited_cwd) = panes
            .first()
            .map(|p| {
                let t = p.terminal.read(cx);
                (t.host_profile().cloned(), t.cwd().map(PathBuf::from))
            })
            .unwrap_or((None, None));
        let cfg = self.terminal_config.clone();

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
            cx.new(|cx| TerminalSession::ssh(cfg, resolved, all_hosts, window, cx))
        } else {
            let shell = self.settings.read(cx).current.shell.clone();
            let cwd = preferred_cwd.or(inherited_cwd);
            cx.new(|cx| TerminalSession::local_with_cwd(cfg, &shell, cwd, cx))
        };
        let pane = self.make_pane(new_term.clone(), window, cx);
        if let Some(Tab {
            kind: TabKind::Terminal { panes, active_pane },
            ..
        }) = self.tabs.get_mut(self.active)
        {
            panes.push(pane);
            *active_pane = panes.len() - 1;
        }
        new_term.read(cx).focus_handle().focus(window, cx);
        self.sync_agent_bus(cx);
        self.save_session(cx);
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
            self.sync_agent_bus(cx);
            self.save_session(cx);
            cx.notify();
            return;
        }
        self.open_split_pane_with_cwd(None, window, cx);
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
        self.save_session(cx);
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
            self.save_session(cx);
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

    fn sync_telemetry_active(&self, cx: &Context<Self>) {
        for (idx, tab) in self.tabs.iter().enumerate() {
            if let TabKind::Terminal { panes, .. } = &tab.kind {
                let active = !self.in_vault && idx == self.active;
                for pane in panes {
                    pane.terminal.read(cx).set_telemetry_active(active);
                }
            }
        }
    }

    fn open_vault(&mut self, section: VaultSection, cx: &mut Context<Self>) {
        self.in_vault = true;
        self.vault_section = section;
        self.telemetry_open = false;
        self.sync_telemetry_active(cx);
        self.save_session(cx);
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

    fn ensure_hosts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<HostsPage> {
        if let Some(hosts) = &self.hosts {
            return hosts.clone();
        }
        let hosts = cx.new(|cx| HostsPage::new(self.settings.clone(), window, cx));
        let sub = cx.subscribe_in(&hosts, window, |this, _, event, window, cx| match event {
            HostsEvent::Connect(host) => this.connect_host(host.clone(), window, cx),
            HostsEvent::OpenSftp(host) => this.open_sftp(host.clone(), window, cx),
        });
        self.hosts = Some(hosts.clone());
        self._hosts_events = Some(sub);
        hosts
    }

    fn ensure_keychain(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<KeychainPage> {
        if let Some(keychain) = &self.keychain {
            return keychain.clone();
        }
        let keychain = cx.new(|cx| KeychainPage::new(self.settings.clone(), window, cx));
        self.keychain = Some(keychain.clone());
        keychain
    }

    fn ensure_snippets(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SnippetsPage> {
        if let Some(snippets) = &self.snippets {
            return snippets.clone();
        }
        let snippets = cx.new(|cx| SnippetsPage::new(self.settings.clone(), window, cx));
        let sub = cx.subscribe_in(&snippets, window, |this, _, event, window, cx| match event {
            SnippetsEvent::Run(snippet) => this.run_snippet(snippet, window, cx),
        });
        self.snippets = Some(snippets.clone());
        self._snippets_events = Some(sub);
        snippets
    }

    fn ensure_tunnels(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TunnelsPage> {
        if let Some(tunnels) = &self.tunnels {
            return tunnels.clone();
        }
        let tunnels = cx.new(|cx| TunnelsPage::new(self.settings.clone(), window, cx));
        self.tunnels = Some(tunnels.clone());
        tunnels
    }

    fn ensure_preferences(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Preferences> {
        if let Some(preferences) = &self.preferences {
            return preferences.clone();
        }
        let preferences = cx.new(|cx| {
            Preferences::new(
                self.settings.clone(),
                Arc::clone(&self.mcp_runtime),
                Arc::clone(&self.updater),
                Arc::clone(&self.sync_manager),
                window,
                cx,
            )
        });
        let sub = cx.subscribe_in(&preferences, window, |this, _, event, window, cx| match event {
            PreferencesEvent::RestartToUpdate => {
                this.install_update_and_quit(window, cx);
            }
        });
        self.preferences = Some(preferences.clone());
        self._preferences_events = Some(sub);
        preferences
    }

    fn ensure_quick_connect(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<QuickConnectPalette> {
        if let Some(quick_connect) = &self.quick_connect {
            return quick_connect.clone();
        }
        let quick_connect =
            cx.new(|cx| QuickConnectPalette::new(self.settings.clone(), window, cx));
        let sub = cx.subscribe_in(&quick_connect, window, |this, _, event, window, cx| {
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
        self.quick_connect = Some(quick_connect.clone());
        self._quick_connect_events = Some(sub);
        quick_connect
    }

    fn open_worktree_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let base_dir = self
            .active_terminal()
            .and_then(|t| {
                let term = t.read(cx);
                if term.is_ssh() {
                    None
                } else {
                    term.cwd().map(PathBuf::from)
                }
            })
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));

        let repo_root = Command::new("git")
            .arg("-C")
            .arg(&base_dir)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));

        let Some(repo_root) = repo_root else {
            crate::notify::warning(
                self.language.text(
                    "Current terminal directory is not inside a Git repository.",
                    "当前本地终端目录不在 Git 仓库中，无法创建 Worktree。",
                ),
                window,
                cx,
            );
            return;
        };

        self.worktree_repo_root = Some(repo_root);
        self.worktree_modal_open = true;
        if self.worktree_branch_input.is_none() {
            let placeholder = self
                .language
                .text("Branch / task name, e.g. feat/agent-auth", "分支或任务名，例如 feat/agent-auth");
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let sub = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.create_git_worktree_tab(window, cx);
                }
            });
            self.worktree_branch_input = Some(input);
            self._worktree_sub = Some(sub);
        }
        if let Some(input) = &self.worktree_branch_input {
            input.update(cx, |i, cx| {
                i.set_value("", window, cx);
                i.focus(window, cx);
            });
        }
        cx.notify();
    }

    fn create_git_worktree_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo_root) = self.worktree_repo_root.clone() else {
            return;
        };
        let branch = self
            .worktree_branch_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_owned())
            .unwrap_or_default();
        if branch.is_empty() {
            return;
        }
        let safe_slug: String = branch
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect();
        let repo_name = repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo");
        let parent = repo_root.parent().unwrap_or(&repo_root);
        let target_dir = parent.join(format!("{repo_name}-{safe_slug}"));

        let output = Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .args(["worktree", "add", "-b", &branch])
            .arg(&target_dir)
            .output();

        self.worktree_modal_open = false;
        match output {
            Ok(out) if out.status.success() => {
                crate::notify::success(
                    format!(
                        "{} {}",
                        self.language.text("Created Git worktree:", "已创建 Git Worktree："),
                        target_dir.display()
                    ),
                    window,
                    cx,
                );
                self.spawn_local_tab_in_cwd(Some(target_dir), true, window, cx);
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
                crate::notify::error(
                    if err.is_empty() {
                        "Failed to create git worktree.".to_owned()
                    } else {
                        err
                    },
                    window,
                    cx,
                );
            }
            Err(err) => {
                crate::notify::error(err.to_string(), window, cx);
            }
        }
        cx.notify();
    }

    fn render_worktree_modal(&self, colors: Colors, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.worktree_modal_open {
            return None;
        }
        let input = self.worktree_branch_input.as_ref()?;
        let repo_label = self
            .worktree_repo_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let language = self.language;

        Some(
            div()
                .id("worktree-modal-backdrop")
                .absolute()
                .inset_0()
                .bg(rgb(0x000000).alpha(0.36))
                .flex()
                .items_start()
                .justify_center()
                .pt(px(110.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.worktree_modal_open = false;
                        this.restore_active_focus(window, cx);
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .id("worktree-modal-card")
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.worktree_modal_open = false;
                                this.restore_active_focus(window, cx);
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }))
                        .w(px(460.))
                        .rounded(px(12.))
                        .bg(colors.popover_bg())
                        .border_1()
                        .border_color(colors.hairline_strong())
                        .shadow_2xl()
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
                                        .text_size(px(13.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(colors.foreground))
                                        .child(language.text(
                                            "New Isolated Git Worktree",
                                            "新建隔离 Git Worktree（并行 Agent）",
                                        )),
                                )
                                .child(
                                    Button::new("wt-close")
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::Close)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.worktree_modal_open = false;
                                            this.restore_active_focus(window, cx);
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(colors.secondary))
                                .truncate()
                                .child(format!(
                                    "{} {repo_label}",
                                    language.text("Repository:", "源仓库：")
                                )),
                        )
                        .child(Input::new(input).small())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(6.))
                                .child(
                                    Button::new("wt-cancel")
                                        .ghost()
                                        .xsmall()
                                        .label(language.text("Cancel", "取消"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.worktree_modal_open = false;
                                            this.restore_active_focus(window, cx);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("wt-create")
                                        .primary()
                                        .xsmall()
                                        .icon(Icon::Plus)
                                        .label(language.text("Create & Open Tab", "创建并打开终端"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.create_git_worktree_tab(window, cx);
                                        })),
                                ),
                        ),
                ),
        )
    }

    fn ensure_file_drawer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<FileDrawer> {
        if let Some(file_drawer) = &self.file_drawer {
            return file_drawer.clone();
        }
        let file_drawer = cx.new(|cx| FileDrawer::new(self.settings.clone(), window, cx));
        let sub = cx.subscribe_in(&file_drawer, window, |this, _, event, window, cx| match event {
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
        });
        self.file_drawer = Some(file_drawer.clone());
        self._file_drawer_events = Some(sub);
        file_drawer
    }

    pub fn open_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_preferences(window, cx)
            .update(cx, |p, cx| p.open_about_tab(cx));
        self.open_vault(VaultSection::Settings, cx);
    }

    pub fn open_updates_and_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_updates(window, cx);
        self.updater.check_for_updates(false);
    }

    pub fn install_update_and_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let should_quit = matches!(self.updater.status(), UpdateStatus::ReadyToRestart { .. });
        match self.updater.launch_installer_and_prepare_quit() {
            Ok(()) => {
                if should_quit {
                    self.save_session(cx);
                    if let Some(tunnels) = &self.tunnels {
                        tunnels.update(cx, |t, _| t.shutdown_all());
                    }
                    for tab in &self.tabs {
                        if let TabKind::Terminal { panes, .. } = &tab.kind {
                            for pane in panes {
                                pane.terminal.update(cx, |t, cx| t.shutdown(cx));
                            }
                        }
                    }
                    cx.quit();
                }
            }
            Err(err) => {
                crate::notify::error(err, window, cx);
            }
        }
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active = index;
            self.in_vault = false;
            self.telemetry_open = false;
            if let Some(term) = self.active_terminal() {
                term.update(cx, |t, cx| t.clear_attention(cx));
            }
            self.sync_telemetry_active(cx);
            self.restore_active_focus(window, cx);
            self.sync_file_drawer(window, cx);
            self.sync_agent_bus(cx);
            self.save_session(cx);
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
        self.dragging_tab_id = None;
        self.tab_drop_target = None;
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
        self.sync_telemetry_active(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.in_vault && !self.tabs.is_empty() {
            self.close_tab(self.active, window, cx);
        }
    }

    fn reorder_tab(
        &mut self,
        dragged_id: u64,
        target_id: u64,
        insert_after: bool,
        cx: &mut Context<Self>,
    ) {
        self.dragging_tab_id = None;
        self.tab_drop_target = None;
        let Some(from) = self.tabs.iter().position(|t| t.id == dragged_id) else {
            cx.notify();
            return;
        };
        let Some(target) = self.tabs.iter().position(|t| t.id == target_id) else {
            cx.notify();
            return;
        };
        let dest = reorder_destination(from, target, insert_after);
        if dest == from {
            cx.notify();
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(dest, tab);
        self.active = active_after_move(self.active, from, dest);
        self.save_session(cx);
        cx.notify();
    }

    fn tab_drop_target_for(
        &self,
        dragged_id: u64,
        target_id: u64,
        insert_after: bool,
    ) -> Option<TabDropTarget> {
        let from = self.tabs.iter().position(|t| t.id == dragged_id)?;
        let target = self.tabs.iter().position(|t| t.id == target_id)?;
        if reorder_destination(from, target, insert_after) == from {
            return None;
        }
        Some(TabDropTarget {
            target_id,
            insert_after,
        })
    }

    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_session(cx);
        if let Some(tunnels) = &self.tunnels {
            tunnels.update(cx, |t, _| t.shutdown_all());
        }
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
            self.ensure_quick_connect(window, cx)
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
        let Some(terminal) = terminal else {
            self.right_drawer = None;
            if let Some(drawer) = &self.file_drawer {
                drawer.update(cx, |drawer, cx| {
                    drawer.sync_target(None, None, None, window, cx);
                });
            }
            return;
        };
        let (host, cwd) = {
            let t = terminal.read(cx);
            (t.host_profile().cloned(), t.cwd().map(str::to_owned))
        };
        let ssh_session = host.is_some().then_some(terminal);
        self.ensure_file_drawer(window, cx).update(cx, |drawer, cx| {
            drawer.sync_target(host.as_ref(), cwd.as_deref(), ssh_session, window, cx);
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
            .child(Icon::Home.render(
                px(14.),
                if self.in_vault {
                    colors.foreground
                } else {
                    colors.secondary
                },
            ))
            .child(language.text("Workspace", "工作台"));

        let has_tab_drag = cx.has_active_drag();
        let dragging_tab_id = has_tab_drag.then_some(self.dragging_tab_id).flatten();
        let tab_drop_target = has_tab_drag.then_some(self.tab_drop_target).flatten();

        let tabs_row = div()
            .id("session-tabs-strip")
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .flex()
            .items_center()
            .gap(px(4.))
            .on_drag_move::<DraggedTab>(cx.listener(
                |this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    let dragged_id = event.drag(cx).tab_id;
                    let mut changed = false;
                    if this.dragging_tab_id != Some(dragged_id) {
                        this.dragging_tab_id = Some(dragged_id);
                        changed = true;
                    }
                    if !event.bounds.contains(&event.event.position)
                        && this.tab_drop_target.is_some()
                    {
                        this.tab_drop_target = None;
                        changed = true;
                    }
                    if changed {
                        cx.notify();
                    }
                },
            ))
            .on_drop::<DraggedTab>(cx.listener(|this, drag: &DraggedTab, _, cx| {
                if let Some(target) = this.tab_drop_target {
                    this.reorder_tab(drag.tab_id, target.target_id, target.insert_after, cx);
                } else {
                    this.dragging_tab_id = None;
                    this.tab_drop_target = None;
                    cx.notify();
                }
            }))
            .children(self.tabs.iter().enumerate().map(|(idx, tab)| {
                let tab_id = tab.id;
                let is_active = !self.in_vault && idx == self.active;
                let is_dragging = dragging_tab_id == Some(tab_id);
                let drop_side = tab_drop_target
                    .filter(|t| t.target_id == tab_id)
                    .map(|t| t.insert_after);
                let (dot_color, label, has_attention, tab_progress) = match &tab.kind {
                    TabKind::Terminal { panes, active_pane } => {
                        let any_attention = panes
                            .iter()
                            .any(|p| p.terminal.read(cx).unread_attention());
                        let any_running = panes
                            .iter()
                            .any(|p| p.terminal.read(cx).agent_activity() == AgentActivity::Running);
                        let any_error = panes
                            .iter()
                            .any(|p| p.terminal.read(cx).agent_activity() == AgentActivity::Error);
                        let active_prog = panes
                            .iter()
                            .map(|p| p.terminal.read(cx).progress())
                            .find(|p| p.is_active())
                            .unwrap_or(TerminalProgress::Hidden);
                        let term = panes
                            .get(*active_pane)
                            .or_else(|| panes.first())
                            .map(|p| p.terminal.read(cx));
                        let (is_ssh, connected, has_error, title) = match term {
                            Some(t) => (
                                t.is_ssh(),
                                t.is_connected(),
                                t.has_error(),
                                t.title().to_owned(),
                            ),
                            None => (false, false, false, "Terminal".into()),
                        };
                        let dot = if any_attention {
                            colors.warning()
                        } else if any_error || has_error {
                            colors.danger
                        } else if any_running {
                            colors.accent
                        } else if connected {
                            colors.success
                        } else if is_ssh {
                            colors.warning()
                        } else {
                            colors.muted
                        };
                        let split_suffix = if panes.len() > 1 { " · 2" } else { "" };
                        (dot, format!("{title}{split_suffix}"), any_attention, active_prog)
                    }
                    TabKind::Sftp { host_name, .. } => (
                        colors.accent,
                        format!("SFTP · {host_name}"),
                        false,
                        TerminalProgress::Hidden,
                    ),
                };

                let progress_bar = tab_progress.is_active().then(|| {
                    let (bar_color, pct) = match tab_progress {
                        TerminalProgress::Normal(p) => (colors.accent, (p as f32).clamp(8.0, 100.0)),
                        TerminalProgress::Error(p) => (colors.danger, p.unwrap_or(100) as f32),
                        TerminalProgress::Indeterminate => (colors.accent, 40.0),
                        TerminalProgress::Paused(p) => (colors.warning(), p.unwrap_or(65) as f32),
                        TerminalProgress::Hidden => (colors.accent, 0.0),
                    };
                    div()
                        .absolute()
                        .bottom_0()
                        .left(px(4.))
                        .right(px(4.))
                        .h(px(2.))
                        .rounded_full()
                        .bg(rgb(bar_color).alpha(0.22))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(gpui_kit::relative(pct / 100.0))
                                .bg(rgb(bar_color)),
                        )
                });

                let drop_indicator = drop_side.map(|insert_after| {
                    let bar = div()
                        .absolute()
                        .top(px(4.))
                        .bottom(px(4.))
                        .w(px(2.))
                        .rounded_full()
                        .bg(rgb(colors.accent));
                    if insert_after {
                        bar.right_0()
                    } else {
                        bar.left_0()
                    }
                });

                let drag_payload = DraggedTab {
                    tab_id,
                    label: SharedString::from(label.clone()),
                    dot_color,
                    colors,
                };

                div()
                    .id(("session-tab", tab_id))
                    .relative()
                    .h(px(26.))
                    .min_w(px(120.))
                    .max_w(px(210.))
                    .flex_shrink_0()
                    .pl(px(10.))
                    .pr(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .border_1()
                    .border_color(if has_attention {
                        rgb(colors.warning()).alpha(0.85)
                    } else if is_active {
                        colors.hairline()
                    } else {
                        transparent_black().into()
                    })
                    .bg(if is_active {
                        colors.surface_active_glass()
                    } else if has_attention {
                        rgb(colors.warning()).alpha(0.11)
                    } else {
                        transparent_black().into()
                    })
                    .opacity(if is_dragging { 0.45 } else { 1.0 })
                    .hover(|s| s.bg(colors.surface_hover_glass()))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_drag(drag_payload, |drag, _offset, _, cx| {
                        cx.new(|_| drag.clone())
                    })
                    .on_drag_move::<DraggedTab>(cx.listener(
                        move |this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            if !event.bounds.contains(&event.event.position) {
                                return;
                            }
                            let dragged_id = event.drag(cx).tab_id;
                            let insert_after = event.event.position.x >= event.bounds.center().x;
                            let next_target =
                                this.tab_drop_target_for(dragged_id, tab_id, insert_after);
                            if this.tab_drop_target != next_target
                                || this.dragging_tab_id != Some(dragged_id)
                            {
                                this.dragging_tab_id = Some(dragged_id);
                                this.tab_drop_target = next_target;
                                cx.notify();
                            }
                        },
                    ))
                    .on_drop::<DraggedTab>(cx.listener(move |this, drag: &DraggedTab, _, cx| {
                        let insert_after = this
                            .tab_drop_target
                            .filter(|t| t.target_id == tab_id)
                            .map(|t| t.insert_after)
                            .unwrap_or_else(|| {
                                let from = this.tabs.iter().position(|t| t.id == drag.tab_id);
                                let target = this.tabs.iter().position(|t| t.id == tab_id);
                                matches!((from, target), (Some(f), Some(t)) if f < t)
                            });
                        this.reorder_tab(drag.tab_id, tab_id, insert_after, cx);
                    }))
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
                            .font_weight(if is_active || has_attention {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(if is_active || has_attention {
                                colors.foreground
                            } else {
                                colors.secondary
                            }))
                            .child(label),
                    )
                    .child(
                        div()
                            .id(("tab-close", tab_id))
                            .size(px(16.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(colors.surface_hover_glass()))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(idx, window, cx);
                            }))
                            .child(Icon::Close.render(px(10.), colors.muted)),
                    )
                    .children(progress_bar)
                    .children(drop_indicator)
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
                    .on_drag_move::<DraggedTab>(cx.listener(
                        |this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            if !event.bounds.contains(&event.event.position) {
                                return;
                            }
                            let dragged_id = event.drag(cx).tab_id;
                            let next_target = this
                                .tabs
                                .last()
                                .and_then(|last| this.tab_drop_target_for(dragged_id, last.id, true));
                            if this.tab_drop_target != next_target
                                || this.dragging_tab_id != Some(dragged_id)
                            {
                                this.dragging_tab_id = Some(dragged_id);
                                this.tab_drop_target = next_target;
                                cx.notify();
                            }
                        },
                    ))
                    .on_drop::<DraggedTab>(cx.listener(|this, drag: &DraggedTab, _, cx| {
                        if let Some(last_id) = this.tabs.last().map(|t| t.id) {
                            this.reorder_tab(drag.tab_id, last_id, true, cx);
                        } else {
                            this.dragging_tab_id = None;
                            this.tab_drop_target = None;
                            cx.notify();
                        }
                    }))
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

        let attention_count = self.unread_attention_count(cx);
        let attention_pill = (attention_count > 0).then(|| {
            div()
                .id("titlebar-attention-pill")
                .h(px(26.))
                .flex_shrink_0()
                .px(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .bg(rgb(colors.warning()).alpha(0.16))
                .border_1()
                .border_color(rgb(colors.warning()).alpha(0.75))
                .hover(|s| s.bg(rgb(colors.warning()).alpha(0.24)))
                .flex()
                .items_center()
                .gap(px(5.))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.jump_next_attention(window, cx);
                }))
                .child(
                    div()
                        .size(px(6.5))
                        .rounded_full()
                        .bg(rgb(colors.warning())),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.foreground))
                        .child(format!(
                            "{attention_count} {} · ⇧⌘U",
                            language.text("waiting", "待处理")
                        )),
                )
        });

        div()
            .id("unified-titlebar")
            .h(px(38.))
            .w_full()
            .px(px(12.))
            .bg(colors.titlebar_bg(self.opacity))
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
            .children(attention_pill)
            .child(cmdk_pill)
    }

    fn render_new_tab_menu(&self, colors: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let hosts = self.settings.read(cx).current.hosts.clone();
        let mono_font: SharedString = if self.font_family.trim().is_empty() {
            if cfg!(target_os = "macos") {
                "Menlo".into()
            } else if cfg!(windows) {
                "Consolas".into()
            } else {
                "DejaVu Sans Mono".into()
            }
        } else {
            self.font_family.trim().to_owned().into()
        };
        let left_x = self
            .new_tab_menu_pos
            .map(|p| (f32::from(p.x) - 18.0).max(12.0))
            .unwrap_or(240.0);

        let menu = div()
            .id("new-tab-host-menu")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .top(px(40.))
            .left(px(left_x))
            .w(px(248.))
            .max_h(px(340.))
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
                    .flex_shrink_0()
                    .px(px(8.))
                    .py(px(6.))
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
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("Local Terminal", "本地终端")),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .text_color(rgb(colors.muted))
                            .child("⌘T"),
                    ),
            )
            .child(
                div()
                    .id("new-tab-menu-worktree")
                    .flex_shrink_0()
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(colors.accent_soft()))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.new_tab_menu_open = false;
                        this.open_worktree_modal(window, cx);
                    }))
                    .child(Icon::Split.render(px(14.), colors.success))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("New Git Worktree…", "新建 Git Worktree…")),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .text_color(rgb(colors.muted))
                            .child("⇧⌘G"),
                    ),
            )
            .child(
                div()
                    .h(px(1.))
                    .flex_shrink_0()
                    .mx(px(4.))
                    .my(px(3.))
                    .bg(colors.hairline()),
            )
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
                                .py(px(6.))
                                .text_size(px(11.5))
                                .line_height(px(15.))
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
                                    .flex_shrink_0()
                                    .px(px(8.))
                                    .py(px(5.))
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
                                                    .line_height(px(15.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(label),
                                            )
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(10.5))
                                                    .line_height(px(13.))
                                                    .font_family(mono_font.clone())
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
            .bg(colors.sidebar_bg(self.opacity))
            .border_r_1()
            .border_color(colors.hairline())
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .px(px(6.))
                    .pt(px(2.))
                    .pb(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(crate::icons::app_logo(px(26.)))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child("Portus"),
                    ),
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
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.vault_section = VaultSection::Hosts;
                            this.selected_group = None;
                            this.ensure_hosts(window, cx)
                                .update(cx, |h, cx| h.set_group_filter(None, cx));
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
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.vault_section = VaultSection::Hosts;
                                let next = if this.selected_group.as_deref() == Some(&group_click) {
                                    None
                                } else {
                                    Some(group_click.clone())
                                };
                                this.selected_group = next.clone();
                                this.ensure_hosts(window, cx)
                                    .update(cx, |h, cx| h.set_group_filter(next, cx));
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
                            self.updater
                                .status()
                                .has_pending_update()
                                .then(|| "↑".to_owned()),
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
        &mut self,
        host: &HostProfile,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = self.language;
        let host_id = host.id;
        let body = self.ensure_tunnels(window, cx).update(cx, |tunnels, cx| {
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
                                        this.ensure_tunnels(window, cx).update(cx, |t, cx| {
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
        let active_term = self.active_terminal();
        let can_sftp = self.file_drawer_terminal(cx).is_some();
        let (is_ssh, connected, has_error, host_label, telemetry, is_split, tunnel_count, has_host) =
            match active_term {
            Some(ref term_entity) => {
                let t = term_entity.read(cx);
                let is_ssh = t.is_ssh();
                let connected = t.is_connected();
                let has_error = t.has_error();
                let host_opt = t.host_profile();
                let label = match host_opt {
                    Some(h) => format!("{} · {}", h.protocol.label(), h.name),
                    None => language.text("Local · Shell", "本地终端").to_owned(),
                };
                let t_count = self
                    .tunnels
                    .as_ref()
                    .and_then(|tunnels| {
                        host_opt.map(|h| tunnels.read(cx).active_count_for_host(h.id, cx))
                    })
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
                    has_error,
                    label,
                    t.telemetry().cloned(),
                    split,
                    t_count,
                    host_opt.is_some(),
                )
            }
            None => (false, false, false, "SFTP".into(), None, false, 0, false),
        };

        let dot_color = if connected {
            colors.success
        } else if is_ssh || has_error {
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
            .bg(colors.titlebar_bg(self.opacity))
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
                            language.text("Files", "文件"),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors_for(self.opacity);

        let body = if self.in_vault {
            let main_view = match self.vault_section {
                VaultSection::Hosts => self.ensure_hosts(window, cx).into_any_element(),
                VaultSection::Keychain => self.ensure_keychain(window, cx).into_any_element(),
                VaultSection::Tunnels => self.ensure_tunnels(window, cx).into_any_element(),
                VaultSection::Snippets => self.ensure_snippets(window, cx).into_any_element(),
                VaultSection::Settings => self.ensure_preferences(window, cx).into_any_element(),
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
                                        let mut focused = false;
                                        if let Some(Tab {
                                            kind: TabKind::Terminal { panes, active_pane },
                                            ..
                                        }) = this.tabs.get_mut(this.active)
                                        {
                                            *active_pane = pidx;
                                            if let Some(p) = panes.get(pidx) {
                                                let term = p.terminal.clone();
                                                term.update(cx, |t, cx| {
                                                    t.clear_attention(cx);
                                                    t.focus_handle().focus(window, cx);
                                                });
                                            }
                                            focused = true;
                                        }
                                        if focused {
                                            this.sync_file_drawer(window, cx);
                                            this.sync_agent_bus(cx);
                                            this.save_session(cx);
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
                        Some(RightDrawer::Files) => {
                            Some(self.ensure_file_drawer(window, cx).into_any_element())
                        }
                        Some(RightDrawer::Snippets) => {
                            Some(self.snippets_drawer(colors, cx).into_any_element())
                        }
                        Some(RightDrawer::Tunnels) => host_profile.as_ref().map(|host| {
                            self.tunnels_drawer(host, colors, window, cx)
                                .into_any_element()
                        }),
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
                        .child(self.session_status_bar(colors, cx))
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
            .on_action(cx.listener(|this, _: &NewGitWorktree, window, cx| {
                this.open_worktree_modal(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_active(window, cx)))
            .on_action(
                cx.listener(|this, _: &NextTab, window, cx| this.step_tab(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PreviousTab, window, cx| this.step_tab(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &JumpNextAttention, window, cx| {
                this.jump_next_attention(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &JumpPrevPrompt, _, cx| {
                if let Some(terminal) = this.active_terminal() {
                    terminal.update(cx, |t, cx| {
                        t.jump_to_prompt(true, cx);
                    });
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &JumpNextPrompt, _, cx| {
                if let Some(terminal) = this.active_terminal() {
                    terminal.update(cx, |t, cx| {
                        t.jump_to_prompt(false, cx);
                    });
                    cx.stop_propagation();
                }
            }))
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
            .on_action(cx.listener(|this, _: &CheckForUpdates, window, cx| {
                this.open_updates_and_check(window, cx);
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
            .bg(colors.window_bg(self.opacity))
            .text_color(rgb(colors.foreground))
            .child(self.titlebar(colors, cx))
            .child(body);

        if self.new_tab_menu_open {
            root = root.child(self.render_new_tab_menu(colors, cx));
        }
        if self.quick_connect_open {
            root = root.child(self.ensure_quick_connect(window, cx));
        }
        if let Some(wt_modal) = self.render_worktree_modal(colors, cx) {
            root = root.child(wt_modal);
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

fn reorder_destination(from: usize, target: usize, insert_after: bool) -> usize {
    let slot = if insert_after { target + 1 } else { target };
    if slot > from { slot - 1 } else { slot }
}

fn active_after_move(active: usize, from: usize, to: usize) -> usize {
    if active == from {
        to
    } else if from < active && active <= to {
        active - 1
    } else if to <= active && active < from {
        active + 1
    } else {
        active
    }
}

#[cfg(test)]
mod tests {
    use super::{active_after_close, active_after_move, reorder_destination};

    #[test]
    fn closing_tabs_preserves_the_active_session_or_selects_a_neighbor() {
        assert_eq!(active_after_close(2, 0, 3), 1);
        assert_eq!(active_after_close(1, 3, 3), 1);
        assert_eq!(active_after_close(1, 1, 3), 1);
        assert_eq!(active_after_close(3, 3, 3), 2);
    }

    #[test]
    fn reordering_tabs_computes_insertion_index_and_tracks_active_tab() {
        // [A, B, C, D]: drag A before C -> [B, A, C, D]
        assert_eq!(reorder_destination(0, 2, false), 1);
        // [A, B, C, D]: drag A after C -> [B, C, A, D]
        assert_eq!(reorder_destination(0, 2, true), 2);
        // [A, B, C, D]: drag C before A -> [C, A, B, D]
        assert_eq!(reorder_destination(2, 0, false), 0);
        // [A, B, C, D]: drag C after A -> [A, C, B, D]
        assert_eq!(reorder_destination(2, 0, true), 1);
        // No-op adjacent drops stay in place
        assert_eq!(reorder_destination(1, 0, true), 1);
        assert_eq!(reorder_destination(1, 2, false), 1);
        assert_eq!(reorder_destination(1, 1, false), 1);
        assert_eq!(reorder_destination(1, 1, true), 1);

        // Active tab index updates across all move directions
        assert_eq!(active_after_move(1, 1, 3), 3);
        assert_eq!(active_after_move(1, 0, 2), 0);
        assert_eq!(active_after_move(1, 0, 1), 0);
        assert_eq!(active_after_move(1, 3, 0), 2);
        assert_eq!(active_after_move(1, 3, 1), 2);
        assert_eq!(active_after_move(1, 3, 2), 1);
    }
}
