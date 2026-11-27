mod ime;

use std::{
    future,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, Styled as _,
    Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
    },
    div, px, rgb,
};
use gpui_terminal::{
    GridSize, PtyCommand, PtyTransport, SearchMatch, ShellMark, TerminalBuilder, TerminalEvent,
    TerminalProgress, TerminalStyle, TerminalView, Transport,
};
use ime::Composition;

use crate::agent_bus::{AgentSessionSnapshot, default_socket_path};
use crate::icons::Icon;
use crate::protocols::{SerialTransport, TelnetTransport, spawn_mosh_pty};
use crate::settings::{
    BellMode, CursorStyle, HostProfile, HostProtocol, Language, LineHeightPreset, ScrollbackLimit,
    Settings, ShellSettings,
};
use crate::ssh::{ServerTelemetry, SshControl, SshTransport, SshUiEvent};
use crate::theme::{Colors, Theme};

static NEXT_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

fn alloc_session_id() -> String {
    let seq = NEXT_SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("sess-{}-{seq}", std::process::id())
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentActivity {
    #[default]
    Idle,
    Running,
    NeedsAttention,
    Error,
}

impl AgentActivity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::NeedsAttention => "waiting",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug)]
pub enum TerminalSessionEvent {
    Notification {
        title: Option<String>,
        body: String,
    },
    CwdChanged,
    StateChanged,
    OpenPath(String),
    ToastInfo(String),
    ToastError(String),
}

impl EventEmitter<TerminalSessionEvent> for TerminalSession {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalConfig {
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
    pub language: Language,
}

impl TerminalConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            theme: settings.theme,
            opacity: settings.opacity,
            font_family: settings.font_family.clone(),
            font_size: settings.font_size,
            line_height: settings.line_height,
            cursor_style: settings.cursor_style,
            cursor_blink: settings.cursor_blink,
            copy_on_select: settings.copy_on_select,
            option_as_meta: settings.option_as_meta,
            bell: settings.bell,
            scrollback: settings.scrollback,
            language: settings.language,
        }
    }
}

pub enum SessionTransport {
    Local(PtyTransport),
    Ssh(SshTransport),
    Telnet(TelnetTransport),
    Serial(SerialTransport),
}

impl Transport for SessionTransport {
    fn write(&mut self, bytes: &[u8]) -> gpui_terminal::Result<()> {
        match self {
            Self::Local(inner) => inner.write(bytes),
            Self::Ssh(inner) => inner.write(bytes),
            Self::Telnet(inner) => inner.write(bytes),
            Self::Serial(inner) => inner.write(bytes),
        }
    }

    fn resize(&mut self, size: GridSize) -> gpui_terminal::Result<()> {
        match self {
            Self::Local(inner) => inner.resize(size),
            Self::Ssh(inner) => inner.resize(size),
            Self::Telnet(inner) => inner.resize(size),
            Self::Serial(inner) => inner.resize(size),
        }
    }

    fn terminate(&mut self) -> gpui_terminal::Result<()> {
        match self {
            Self::Local(inner) => inner.terminate(),
            Self::Ssh(inner) => inner.terminate(),
            Self::Telnet(inner) => inner.terminate(),
            Self::Serial(inner) => inner.terminate(),
        }
    }

    fn process_id(&self) -> Option<u32> {
        match self {
            Self::Local(inner) => inner.process_id(),
            Self::Ssh(inner) => inner.process_id(),
            Self::Telnet(inner) => inner.process_id(),
            Self::Serial(inner) => inner.process_id(),
        }
    }
}

pub enum TerminalStatus {
    Connecting(String),
    StartFailed(String),
    Exited(Option<i32>),
    IoError(String),
    SshError(String),
}

impl TerminalStatus {
    fn message(&self, language: Language, is_ssh: bool) -> String {
        match self {
            Self::Connecting(target) => format!(
                "{} {target}…",
                language.text("Connecting to", "正在连接")
            ),
            Self::StartFailed(error) => format!(
                "{} {error}",
                language.text("Could not start session.", "无法启动会话。")
            ),
            Self::Exited(Some(0)) => {
                if is_ssh {
                    language.text("SSH session closed.", "SSH 会话已关闭。").into()
                } else {
                    language.text("Shell exited.", "Shell 已退出。").into()
                }
            }
            Self::Exited(Some(code)) => match (language, is_ssh) {
                (Language::English, true) => format!("SSH session exited with status {code}."),
                (Language::ChineseSimplified, true) => {
                    format!("SSH 会话已退出，状态码为 {code}。")
                }
                (Language::English, false) => format!("Shell exited with status {code}."),
                (Language::ChineseSimplified, false) => {
                    format!("Shell 已退出，状态码为 {code}。")
                }
            },
            Self::Exited(None) => {
                if is_ssh {
                    language
                        .text("SSH connection closed.", "SSH 连接已断开。")
                        .into()
                } else {
                    language
                        .text("Shell session ended.", "Shell 会话已结束。")
                        .into()
                }
            }
            Self::IoError(error) => format!(
                "{} {error}",
                language.text("Terminal I/O error.", "终端读写失败。")
            ),
            Self::SshError(error) => error.clone(),
        }
    }
}

pub struct TerminalSession {
    session_id: String,
    terminal: Option<Entity<TerminalView<SessionTransport>>>,
    focus: FocusHandle,
    config: TerminalConfig,
    title: String,
    initial_title: String,
    cwd: Option<String>,
    progress: TerminalProgress,
    agent_activity: AgentActivity,
    unread_attention: bool,
    attention_banner: Option<(String, String)>,
    last_output_ms: Arc<AtomicU64>,
    ssh_host: Option<HostProfile>,
    ssh_hosts: Vec<HostProfile>,
    ssh_control: Option<SshControl>,
    connected: bool,
    telemetry: Option<ServerTelemetry>,
    status: Option<TerminalStatus>,
    prompt_sub: Option<Subscription>,
    search_open: bool,
    search_input: Option<Entity<InputState>>,
    search_matches: Vec<SearchMatch>,
    search_index: usize,
    _search_sub: Option<Subscription>,
    composition: Composition,
    bell_flash: bool,
    bell_seq: u64,
    _events: Option<Subscription>,
    _shutdown: Option<Subscription>,
}

impl TerminalSession {
    pub fn local(
        config: TerminalConfig,
        shell: &ShellSettings,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::local_with_cwd(config, shell, None, cx)
    }

    pub fn local_with_cwd(
        config: TerminalConfig,
        shell: &ShellSettings,
        preferred_cwd: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = alloc_session_id();
        let cwd = preferred_cwd
            .filter(|path| path.is_dir())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .filter(|path| path.parent().is_some())
            })
            .or_else(dirs::home_dir);
        let initial_title = cwd.as_deref().map(directory_title).unwrap_or_default();
        let cwd_string = cwd.as_deref().map(|p| p.to_string_lossy().into_owned());
        let mut shell = shell_command(shell, &session_id);
        if let Some(cwd) = cwd {
            shell = shell.cwd(cwd);
        }
        let last_output_ms = Arc::new(AtomicU64::new(0));
        let output_tracker = Arc::clone(&last_output_ms);
        let result = TerminalBuilder::new()
            .scrollback(config.scrollback.lines())
            .on_output(move |_| {
                output_tracker.store(now_epoch_ms(), Ordering::Relaxed);
            })
            .connect(|sink| PtyTransport::spawn(&shell, sink).map(SessionTransport::Local));

        let terminal = match result {
            Ok(terminal) => terminal,
            Err(error) => {
                return Self {
                    session_id,
                    terminal: None,
                    focus: cx.focus_handle(),
                    config,
                    title: initial_title.clone(),
                    initial_title,
                    cwd: cwd_string,
                    progress: TerminalProgress::Hidden,
                    agent_activity: AgentActivity::Error,
                    unread_attention: false,
                    attention_banner: None,
                    last_output_ms,
                    ssh_host: None,
                    ssh_hosts: Vec::new(),
                    ssh_control: None,
                    connected: false,
                    telemetry: None,
                    status: Some(TerminalStatus::StartFailed(error.to_string())),
                    prompt_sub: None,
                    search_open: false,
                    search_input: None,
                    search_matches: Vec::new(),
                    search_index: 0,
                    _search_sub: None,
                    composition: Composition::default(),
                    bell_flash: false,
                    bell_seq: 0,
                    _events: None,
                    _shutdown: None,
                };
            }
        };

        let style = build_terminal_style(&config);
        let view = cx.new(|cx| TerminalView::new(terminal, style, cx));
        let focus = view.read(cx).focus_handle(cx);
        let events = subscribe_terminal_events(&view, cx);
        let shutdown = cx.on_app_quit(|this, cx| {
            this.shutdown(cx);
            future::ready(())
        });

        Self {
            session_id,
            terminal: Some(view),
            focus,
            config,
            title: initial_title.clone(),
            initial_title,
            cwd: cwd_string,
            progress: TerminalProgress::Hidden,
            agent_activity: AgentActivity::Idle,
            unread_attention: false,
            attention_banner: None,
            last_output_ms,
            ssh_host: None,
            ssh_hosts: Vec::new(),
            ssh_control: None,
            connected: true,
            telemetry: None,
            status: None,
            prompt_sub: None,
            search_open: false,
            search_input: None,
            search_matches: Vec::new(),
            search_index: 0,
            _search_sub: None,
            composition: Composition::default(),
            bell_flash: false,
            bell_seq: 0,
            _events: Some(events),
            _shutdown: Some(shutdown),
        }
    }

    pub fn ssh(
        config: TerminalConfig,
        host: HostProfile,
        all_hosts: Vec<HostProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_title = host.name.clone();
        let mut session = Self {
            session_id: alloc_session_id(),
            terminal: None,
            focus: cx.focus_handle(),
            config,
            title: initial_title.clone(),
            initial_title,
            cwd: None,
            progress: TerminalProgress::Hidden,
            agent_activity: AgentActivity::Idle,
            unread_attention: false,
            attention_banner: None,
            last_output_ms: Arc::new(AtomicU64::new(0)),
            ssh_host: Some(host),
            ssh_hosts: all_hosts,
            ssh_control: None,
            connected: false,
            telemetry: None,
            status: None,
            prompt_sub: None,
            search_open: false,
            search_input: None,
            search_matches: Vec::new(),
            search_index: 0,
            _search_sub: None,
            composition: Composition::default(),
            bell_flash: false,
            bell_seq: 0,
            _events: None,
            _shutdown: None,
        };
        session.connect_ssh(window, cx);
        session
    }

    fn connect_ssh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self.ssh_host.clone() else {
            return;
        };
        self.shutdown(cx);
        self.prompt_sub = None;
        self.connected = false;
        let endpoint = host.endpoint();
        self.status = Some(TerminalStatus::Connecting(endpoint.clone()));

        let (ui_tx, ui_rx) = async_channel::bounded::<SshUiEvent>(64);
        let protocol = host.protocol;
        let all_hosts = self.ssh_hosts.clone();
        let (control, mut control_rx_opt) = if protocol == HostProtocol::Ssh {
            let (ctrl, rx) = SshControl::channel();
            (Some(ctrl), Some(rx))
        } else {
            (None, None)
        };
        let output_tracker = Arc::clone(&self.last_output_ms);
        let result = TerminalBuilder::new()
            .scrollback(self.config.scrollback.lines())
            .on_output(move |_| {
                output_tracker.store(now_epoch_ms(), Ordering::Relaxed);
            })
            .connect(|sink| match protocol {
                HostProtocol::Ssh => {
                    let rx = control_rx_opt.take().expect("ssh control rx");
                    SshTransport::spawn(host, all_hosts, sink, ui_tx, rx).map(SessionTransport::Ssh)
                }
                HostProtocol::Telnet => {
                    TelnetTransport::spawn(host.address, host.port, sink)
                        .map(SessionTransport::Telnet)
                }
                HostProtocol::Serial => {
                    SerialTransport::spawn(&host.address, host.baud_rate, sink)
                        .map(SessionTransport::Serial)
                }
                HostProtocol::Mosh => spawn_mosh_pty(&host, sink).map(SessionTransport::Local),
            });

        match result {
            Ok(terminal) => {
                self.ssh_control = control;
                let style = build_terminal_style(&self.config);
                let view = cx.new(|cx| TerminalView::new(terminal, style, cx));
                let focus = view.read(cx).focus_handle(cx);
                let events = subscribe_terminal_events(&view, cx);
                let shutdown = cx.on_app_quit(|this, cx| {
                    this.shutdown(cx);
                    future::ready(())
                });
                self.terminal = Some(view);
                self.focus = focus.clone();
                self._events = Some(events);
                self._shutdown = Some(shutdown);
                if protocol != HostProtocol::Ssh {
                    self.connected = true;
                    self.status = None;
                }
                focus.focus(window, cx);
            }
            Err(error) => {
                let msg = error.to_string();
                self.status = Some(TerminalStatus::StartFailed(msg.clone()));
                crate::notify::error(msg, window, cx);
                cx.notify();
                return;
            }
        }

        if protocol != HostProtocol::Ssh {
            cx.notify();
            return;
        }

        cx.spawn_in(window, async move |session, cx| {
            while let Ok(event) = ui_rx.recv().await {
                let keep_going = cx.update(|window, cx| {
                    session
                        .update(cx, |this, cx| {
                            this.handle_ssh_event(event, window, cx);
                        })
                        .is_ok()
                });
                if !keep_going.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn handle_ssh_event(
        &mut self,
        event: SshUiEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SshUiEvent::Connecting(target) => {
                if !self.connected {
                    self.status = Some(TerminalStatus::Connecting(target));
                }
            }
            SshUiEvent::Connected => {
                self.connected = true;
                self.status = None;
                self.prompt_sub = None;
                self.focus.focus(window, cx);
            }
            SshUiEvent::Telemetry(tele) => {
                self.telemetry = Some(tele);
            }
            SshUiEvent::Prompt(prompt) => {
                self.prompt_sub =
                    crate::notify::push_ssh_prompt(prompt, self.config.language, window, cx);
            }
            SshUiEvent::Error(error) => {
                self.connected = false;
                self.prompt_sub = None;
                let status = TerminalStatus::SshError(error);
                let msg = status.message(self.config.language, true);
                self.status = Some(status);
                let session = cx.entity().downgrade();
                let title = self
                    .config
                    .language
                    .text("SSH Connection Failed", "SSH 连接失败");
                let reconnect_label = self.config.language.text("Reconnect", "重新连接");
                crate::notify::error_action(
                    title,
                    msg,
                    reconnect_label,
                    Some(Icon::Refresh),
                    move |window, cx| {
                        let _ = session.update(cx, |this, cx| this.reconnect(window, cx));
                    },
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub fn reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ssh_host.is_some() {
            self.connect_ssh(window, cx);
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn progress(&self) -> TerminalProgress {
        self.progress
    }

    pub fn agent_activity(&self) -> AgentActivity {
        if self.agent_activity == AgentActivity::Idle {
            let last = self.last_output_ms.load(Ordering::Relaxed);
            if last > 0 && now_epoch_ms().saturating_sub(last) < 1200 {
                return AgentActivity::Running;
            }
        }
        self.agent_activity
    }

    pub fn unread_attention(&self) -> bool {
        self.unread_attention
    }

    pub fn attention_message(&self) -> Option<&str> {
        self.attention_banner.as_ref().map(|(_, body)| body.as_str())
    }

    pub fn clear_attention(&mut self, cx: &mut Context<Self>) {
        if self.unread_attention || self.attention_banner.is_some() {
            self.unread_attention = false;
            self.attention_banner = None;
            if self.agent_activity == AgentActivity::NeedsAttention {
                self.agent_activity = AgentActivity::Idle;
            }
            cx.emit(TerminalSessionEvent::StateChanged);
            cx.notify();
        }
    }

    pub fn apply_agent_notification(
        &mut self,
        title: Option<String>,
        body: String,
        mark_unread: bool,
        cx: &mut Context<Self>,
    ) {
        let resolved_title = title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| self.title.clone());
        self.agent_activity = AgentActivity::NeedsAttention;
        self.attention_banner = Some((resolved_title.clone(), body.clone()));
        if mark_unread {
            self.unread_attention = true;
        }
        cx.emit(TerminalSessionEvent::StateChanged);
        cx.notify();
    }

    pub fn apply_agent_status(
        &mut self,
        state: &str,
        message: Option<String>,
        progress: Option<u8>,
        cx: &mut Context<Self>,
    ) {
        match state.to_ascii_lowercase().as_str() {
            "running" | "busy" | "working" => {
                self.agent_activity = AgentActivity::Running;
                self.unread_attention = false;
                if let Some(pct) = progress {
                    self.progress = TerminalProgress::Normal(pct.min(100));
                } else if self.progress == TerminalProgress::Hidden {
                    self.progress = TerminalProgress::Indeterminate;
                }
            }
            "waiting" | "attention" | "approval" | "needs_attention" => {
                self.agent_activity = AgentActivity::NeedsAttention;
                self.unread_attention = true;
                if let Some(msg) = message {
                    self.attention_banner = Some((self.title.clone(), msg));
                }
                if let Some(pct) = progress {
                    self.progress = TerminalProgress::Paused(Some(pct.min(100)));
                }
            }
            "error" | "failed" => {
                self.agent_activity = AgentActivity::Error;
                self.unread_attention = true;
                if let Some(msg) = message {
                    self.attention_banner = Some((self.title.clone(), msg));
                }
                self.progress = TerminalProgress::Error(progress.map(|p| p.min(100)));
            }
            _ => {
                self.agent_activity = AgentActivity::Idle;
                self.progress = TerminalProgress::Hidden;
                if let Some(msg) = message {
                    self.attention_banner = Some((self.title.clone(), msg));
                }
            }
        }
        cx.emit(TerminalSessionEvent::StateChanged);
        cx.notify();
    }

    pub fn snapshot(&self) -> AgentSessionSnapshot {
        let progress = match self.progress {
            TerminalProgress::Normal(p) => Some(p),
            TerminalProgress::Error(p) | TerminalProgress::Paused(p) => p,
            TerminalProgress::Hidden | TerminalProgress::Indeterminate => None,
        };
        AgentSessionSnapshot {
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            is_ssh: self.is_ssh(),
            host: self.ssh_host.as_ref().map(|h| h.name.clone()),
            cwd: self.cwd.clone(),
            state: self.agent_activity().as_str().to_owned(),
            progress,
            attention_message: self.attention_message().map(str::to_owned),
            unread: self.unread_attention,
        }
    }

    pub fn is_ssh(&self) -> bool {
        self.ssh_host.is_some()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn has_error(&self) -> bool {
        matches!(
            self.status,
            Some(
                TerminalStatus::StartFailed(_)
                    | TerminalStatus::IoError(_)
                    | TerminalStatus::SshError(_)
            )
        )
    }

    pub fn telemetry(&self) -> Option<&ServerTelemetry> {
        self.telemetry.as_ref()
    }

    pub fn set_telemetry_active(&self, active: bool) {
        if let Some(ctrl) = &self.ssh_control {
            ctrl.set_telemetry_active(active);
        }
    }

    pub fn fetch_docker_logs(
        &self,
        container: String,
        tail: usize,
        timestamps: bool,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<String, String>>> {
        self.ssh_control
            .as_ref()
            .map(|ctrl| ctrl.fetch_docker_logs(container, tail, timestamps))
    }

    pub fn exec_remote_command(
        &self,
        command: String,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<String, String>>> {
        self.ssh_control
            .as_ref()
            .map(|ctrl| ctrl.exec_command(command))
    }

    pub fn upload_remote_bytes(
        &self,
        remote_path: String,
        data: Vec<u8>,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<String, String>>> {
        self.ssh_control
            .as_ref()
            .map(|ctrl| ctrl.upload_bytes(remote_path, data))
    }

    pub fn host_profile(&self) -> Option<&HostProfile> {
        self.ssh_host.as_ref()
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn send_bytes(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if let Some(view) = &self.terminal
            && let Err(error) = view.read(cx).terminal().write(bytes)
        {
            self.status = Some(TerminalStatus::IoError(error.to_string()));
            cx.notify();
        }
    }

    /// Navigates to the previous or next OSC 133 prompt mark in scrollback.
    pub fn jump_to_prompt(&mut self, previous: bool, cx: &mut Context<Self>) -> bool {
        let Some(view) = &self.terminal else {
            return false;
        };
        view.update(cx, |view, cx| {
            let moved = view.terminal().jump_to_prompt(previous);
            if moved {
                cx.notify();
            }
            moved
        })
    }

    fn handle_image_paste(&mut self, cx: &mut Context<Self>) {
        let Some(png) = crate::platform::read_clipboard_image_png() else {
            return;
        };
        let lang = self.config.language;
        let filename = format!("portus-paste-{}.png", now_epoch_ms());

        if self.is_ssh() {
            let remote_path = format!("/tmp/{filename}");
            let Some(rx) = self.upload_remote_bytes(remote_path.clone(), png) else {
                cx.emit(TerminalSessionEvent::ToastError(
                    lang.text(
                        "Cannot upload clipboard image: SSH session not ready.",
                        "无法上传剪贴板截图：SSH 会话尚未就绪。",
                    )
                    .into(),
                ));
                return;
            };
            cx.emit(TerminalSessionEvent::ToastInfo(
                lang.text(
                    "Uploading clipboard screenshot over SSH…",
                    "正在通过 SSH 上传剪贴板截图…",
                )
                .into(),
            ));
            cx.spawn(async move |session, cx| {
                let res = rx
                    .await
                    .unwrap_or_else(|_| Err("SSH session closed during upload.".to_owned()));
                let _ = session.update(cx, |this, cx| match res {
                    Ok(path) => {
                        let quoted = shell_words::quote(&path);
                        let bytes = format!("{quoted} ").into_bytes();
                        this.send_bytes(&bytes, cx);
                        cx.emit(TerminalSessionEvent::ToastInfo(format!(
                            "{} {path}",
                            this.config
                                .language
                                .text("Uploaded screenshot to", "截图已上传至")
                        )));
                    }
                    Err(err) => {
                        cx.emit(TerminalSessionEvent::ToastError(err));
                    }
                });
            })
            .detach();
            return;
        }

        let local_path = std::env::temp_dir().join(&filename);
        match std::fs::write(&local_path, &png) {
            Ok(()) => {
                let path_str = local_path.to_string_lossy().into_owned();
                let quoted = shell_words::quote(&path_str);
                let bytes = format!("{quoted} ").into_bytes();
                self.send_bytes(&bytes, cx);
                cx.emit(TerminalSessionEvent::ToastInfo(format!(
                    "{} {path_str}",
                    lang.text("Pasted screenshot path:", "已插入截图路径：")
                )));
            }
            Err(err) => {
                cx.emit(TerminalSessionEvent::ToastError(err.to_string()));
            }
        }
    }

    fn handle_dropped_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let lang = self.config.language;
        if !self.is_ssh() {
            let joined = paths
                .iter()
                .map(|p| shell_words::quote(&p.to_string_lossy()).into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            if !joined.is_empty() {
                let bytes = format!("{joined} ").into_bytes();
                self.send_bytes(&bytes, cx);
            }
            return;
        }

        // SSH mode: upload dropped local files directly to `/tmp/portus-uploads/<name>`
        // over the active SSH channel and paste their remote paths into the terminal.
        let Some(ctrl) = self.ssh_control.clone() else {
            cx.emit(TerminalSessionEvent::ToastError(
                lang.text(
                    "Cannot upload dropped files: SSH session not ready.",
                    "无法上传拖入的文件：SSH 会话尚未就绪。",
                )
                .into(),
            ));
            return;
        };
        cx.emit(TerminalSessionEvent::ToastInfo(
            lang.text(
                "Uploading dropped files over SSH…",
                "正在通过 SSH 上传拖入的文件…",
            )
            .into(),
        ));
        let bg = cx.background_executor().spawn(async move {
            let mut prepared = Vec::new();
            for path in paths {
                if !path.is_file() {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("upload.bin")
                    .to_owned();
                match std::fs::read(&path) {
                    Ok(bytes) => prepared.push((name, bytes)),
                    Err(err) => return Err(format!("{}: {err}", path.display())),
                }
            }
            Ok(prepared)
        });
        cx.spawn(async move |session, cx| {
            let prepared = match bg.await {
                Ok(files) => files,
                Err(err) => {
                    let _ = session.update(cx, |_, cx| {
                        cx.emit(TerminalSessionEvent::ToastError(err));
                    });
                    return;
                }
            };
            let mut uploaded_paths = Vec::new();
            for (name, bytes) in prepared {
                let remote = format!("/tmp/portus-uploads/{name}");
                let rx = ctrl.upload_bytes(remote, bytes);
                match rx.await {
                    Ok(Ok(path)) => uploaded_paths.push(path),
                    Ok(Err(err)) => {
                        let _ = session.update(cx, |_, cx| {
                            cx.emit(TerminalSessionEvent::ToastError(err));
                        });
                        return;
                    }
                    Err(_) => return,
                }
            }
            if !uploaded_paths.is_empty() {
                let _ = session.update(cx, |this, cx| {
                    let joined = uploaded_paths
                        .iter()
                        .map(|p| shell_words::quote(p).into_owned())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let bytes = format!("{joined} ").into_bytes();
                    this.send_bytes(&bytes, cx);
                    cx.emit(TerminalSessionEvent::ToastInfo(format!(
                        "{} {}",
                        this.config
                            .language
                            .text("Uploaded to remote:", "已上传至远端："),
                        uploaded_paths.join(", ")
                    )));
                });
            }
        })
        .detach();
    }

    pub fn set_config(&mut self, config: TerminalConfig, cx: &mut Context<Self>) {
        let scrollback_changed = self.config.scrollback != config.scrollback;
        self.config = config;
        if let Some(view) = &self.terminal {
            if scrollback_changed {
                view.read(cx)
                    .terminal()
                    .set_scrollback(self.config.scrollback.lines());
            }
            let style = build_terminal_style(&self.config);
            view.update(cx, |view, cx| view.set_style(style, cx));
        }
        cx.notify();
    }

    fn trigger_visual_bell(&mut self, cx: &mut Context<Self>) {
        self.bell_flash = true;
        self.bell_seq = self.bell_seq.wrapping_add(1);
        let seq = self.bell_seq;
        cx.spawn(async move |session, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(140))
                .await;
            let _ = session.update(cx, |this, cx| {
                if this.bell_seq == seq {
                    this.bell_flash = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.close_search(window, cx);
            return;
        }
        self.search_open = true;
        if self.search_input.is_none() {
            let placeholder = self
                .config
                .language
                .text("Find in terminal…", "在终端中搜索…");
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let sub = cx.subscribe(&state, |this, _, event, cx| match event {
                InputEvent::Change => this.refresh_search(cx),
                InputEvent::PressEnter { shift, .. } => this.step_search(*shift, cx),
                _ => {}
            });
            self.search_input = Some(state);
            self._search_sub = Some(sub);
        }
        if let Some(input) = &self.search_input {
            input.update(cx, |s, cx| s.focus(window, cx));
        }
        self.refresh_search(cx);
    }

    pub fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = false;
        self.search_matches.clear();
        self.search_index = 0;
        if let Some(view) = &self.terminal {
            view.update(cx, |view, cx| {
                view.terminal().clear_search_highlights();
                cx.notify();
            });
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        self.search_matches.clear();
        self.search_index = 0;
        let Some(input) = &self.search_input else {
            return;
        };
        let query = input.read(cx).value().to_string();
        if query.is_empty() {
            if let Some(view) = &self.terminal {
                view.update(cx, |view, cx| {
                    view.terminal().clear_search_highlights();
                    cx.notify();
                });
            }
            cx.notify();
            return;
        }
        if let Some(view) = &self.terminal {
            let matches = view.read(cx).terminal().search_all(&query);
            self.search_matches = matches;
            if !self.search_matches.is_empty() {
                self.search_index = self.search_matches.len() - 1;
                self.highlight_search_match(cx);
            } else {
                view.update(cx, |view, cx| {
                    view.terminal().clear_search_highlights();
                    cx.notify();
                });
            }
        }
        cx.notify();
    }

    fn step_search(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let len = self.search_matches.len();
        if len == 0 {
            return;
        }
        self.search_index = if backwards {
            if self.search_index == 0 {
                len - 1
            } else {
                self.search_index - 1
            }
        } else {
            (self.search_index + 1) % len
        };
        self.highlight_search_match(cx);
        cx.notify();
    }

    fn highlight_search_match(&self, cx: &mut Context<Self>) {
        let Some(&active) = self.search_matches.get(self.search_index) else {
            return;
        };
        let matches = self.search_matches.clone();
        let active_idx = Some(self.search_index);
        if let Some(view) = &self.terminal {
            view.update(cx, |view, cx| {
                view.terminal().set_search_highlights(matches, active_idx);
                view.terminal().scroll_to_line(active.line);
                cx.notify();
            });
        }
    }

    fn render_search_bar(&self, colors: Colors, cx: &Context<Self>) -> Option<impl IntoElement> {
        if !self.search_open {
            return None;
        }
        let input = self.search_input.as_ref()?;
        let count_text = if self.search_matches.is_empty() {
            "0/0".to_owned()
        } else {
            format!("{}/{}", self.search_index + 1, self.search_matches.len())
        };
        Some(
            div()
                .absolute()
                .top(px(8.))
                .right(px(10.))
                .px(px(8.))
                .py(px(6.))
                .rounded_xl()
                .bg(rgb(colors.panel))
                .border_1()
                .border_color(colors.hairline_strong())
                .shadow_xl()
                .flex()
                .items_center()
                .gap(px(5.))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.close_search(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(Icon::Search.render(px(13.5), colors.muted))
                .child(div().w(px(184.)).child(Input::new(input).small().text_xs()))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(colors.muted))
                        .min_w(px(30.))
                        .text_center()
                        .child(count_text),
                )
                .child(div().w(px(1.)).h(px(14.)).bg(colors.hairline()))
                .child(
                    Button::new("search-prev")
                        .ghost()
                        .xsmall()
                        .icon(Icon::ArrowUp)
                        .on_click(cx.listener(|this, _, _, cx| this.step_search(true, cx))),
                )
                .child(
                    Button::new("search-next")
                        .ghost()
                        .xsmall()
                        .icon(Icon::ArrowDown)
                        .on_click(cx.listener(|this, _, _, cx| this.step_search(false, cx))),
                )
                .child(
                    Button::new("search-close")
                        .ghost()
                        .xsmall()
                        .icon(Icon::Close)
                        .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx))),
                ),
        )
    }

    pub fn shutdown(&mut self, cx: &App) {
        self.ssh_control = None;
        if let Some(view) = self.terminal.take()
            && view.read(cx).terminal().exit_report().is_none()
            && let Err(error) = view.read(cx).terminal().terminate()
        {
            eprintln!("Could not close the session. {error}");
        }
    }
}

fn build_terminal_style(config: &TerminalConfig) -> TerminalStyle {
    let family = if config.font_family.trim().is_empty() {
        default_font_family().to_owned()
    } else {
        config.font_family.trim().to_owned()
    };
    TerminalStyle::new(family, px(f32::from(config.font_size)))
        .line_height(config.line_height.factor())
        .palette(config.theme.palette_for_opacity(config.opacity))
        .background_alpha(0.0)
        .cursor_style(config.cursor_style.to_terminal())
        .cursor_blink(config.cursor_blink)
        .copy_on_select(config.copy_on_select)
        .option_as_meta(config.option_as_meta)
}

fn default_font_family() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(windows) {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

fn subscribe_terminal_events(
    view: &Entity<TerminalView<SessionTransport>>,
    cx: &mut Context<TerminalSession>,
) -> Subscription {
    cx.subscribe(view, |this, view, event, cx| {
        match event {
            TerminalEvent::Exited(report) => {
                this.connected = false;
                this.progress = TerminalProgress::Hidden;
                this.agent_activity = if report.code.unwrap_or(0) == 0 {
                    AgentActivity::Idle
                } else {
                    AgentActivity::Error
                };
                this.composition = Composition::default();
                if this.status.is_none() || matches!(this.status, Some(TerminalStatus::Connecting(_)))
                {
                    this.status = Some(TerminalStatus::Exited(report.code));
                }
                cx.emit(TerminalSessionEvent::StateChanged);
            }
            TerminalEvent::TransportFailed(error)
                if view.read(cx).terminal().exit_report().is_none() =>
            {
                this.connected = false;
                this.agent_activity = AgentActivity::Error;
                this.status = Some(TerminalStatus::IoError(error.to_string()));
                cx.emit(TerminalSessionEvent::StateChanged);
            }
            TerminalEvent::Title(title) => {
                let title: String = title
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(256)
                    .collect();
                this.title = if title.trim().is_empty() {
                    this.initial_title.clone()
                } else {
                    title
                };
                cx.emit(TerminalSessionEvent::StateChanged);
            }
            TerminalEvent::ResetTitle => {
                this.title.clone_from(&this.initial_title);
                cx.emit(TerminalSessionEvent::StateChanged);
            }
            TerminalEvent::ClipboardStore(text) => {
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                }
                return;
            }
            TerminalEvent::CwdChanged(cwd) => {
                this.cwd = Some(cwd.clone());
                if !this.is_ssh() && this.title == this.initial_title {
                    this.title = directory_title(Path::new(cwd));
                    this.initial_title = this.title.clone();
                }
                cx.emit(TerminalSessionEvent::CwdChanged);
                cx.emit(TerminalSessionEvent::StateChanged);
            }
            TerminalEvent::Progress(progress) => {
                this.progress = *progress;
                match progress {
                    TerminalProgress::Normal(_) | TerminalProgress::Indeterminate => {
                        this.agent_activity = AgentActivity::Running;
                    }
                    TerminalProgress::Error(_) => {
                        this.agent_activity = AgentActivity::Error;
                    }
                    TerminalProgress::Paused(_) => {
                        this.agent_activity = AgentActivity::NeedsAttention;
                    }
                    TerminalProgress::Hidden => {
                        if this.agent_activity == AgentActivity::Running {
                            this.agent_activity = AgentActivity::Idle;
                        }
                    }
                }
                cx.emit(TerminalSessionEvent::StateChanged);
            }
            TerminalEvent::Notification { title, body } => {
                cx.emit(TerminalSessionEvent::Notification {
                    title: title.clone(),
                    body: body.clone(),
                });
            }
            TerminalEvent::ShellMark(mark) => match mark {
                ShellMark::CommandStart => {
                    this.agent_activity = AgentActivity::Running;
                    cx.emit(TerminalSessionEvent::StateChanged);
                }
                ShellMark::CommandFinished(code) => {
                    if code.is_some_and(|c| c != 0) {
                        this.agent_activity = AgentActivity::Error;
                    } else if this.agent_activity == AgentActivity::Running {
                        this.agent_activity = AgentActivity::Idle;
                    }
                    cx.emit(TerminalSessionEvent::StateChanged);
                }
                ShellMark::PromptStart | ShellMark::PromptEnd => {
                    if this.agent_activity == AgentActivity::Running {
                        this.agent_activity = AgentActivity::Idle;
                        cx.emit(TerminalSessionEvent::StateChanged);
                    }
                }
            },
            TerminalEvent::OpenUrl(uri) => {
                if let Some(path) = uri.strip_prefix("file://") {
                    let clean = path.find('/').map(|i| &path[i..]).unwrap_or(path);
                    cx.emit(TerminalSessionEvent::OpenPath(clean.to_owned()));
                } else {
                    crate::platform::open_external_url(uri);
                }
                return;
            }
            TerminalEvent::ImagePasteRequested => {
                this.handle_image_paste(cx);
                return;
            }
            TerminalEvent::FilesDropped(paths) => {
                this.handle_dropped_files(paths.clone(), cx);
                return;
            }
            TerminalEvent::Bell => match this.config.bell {
                BellMode::Off => return,
                BellMode::Visual => this.trigger_visual_bell(cx),
                BellMode::Sound => {
                    crate::platform::system_beep();
                    return;
                }
            },
            _ => return,
        }
        cx.notify();
    })
}

fn shell_command(shell: &ShellSettings, session_id: &str) -> PtyCommand {
    let mut command = PtyCommand::new(shell.executable());
    if cfg!(unix) && shell.login {
        command = command.arg("-l");
    }
    let socket = default_socket_path();
    command
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("TERM_PROGRAM", "Portus")
        .env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"))
        .env("PORTUS_SESSION_ID", session_id)
        .env("PORTUS_SOCKET", socket.as_os_str())
}

fn directory_title(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(relative) = path.strip_prefix(home)
    {
        return if relative.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", relative.display())
        };
    }
    path.display().to_string()
}

impl Render for TerminalSession {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.config.theme.colors_for(self.config.opacity);
        let palette = self.config.theme.palette_for_opacity(self.config.opacity);
        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_color(rgb(palette.foreground))
            .px(px(14.))
            .pt(px(10.))
            .pb(px(10.));

        if let Some(view) = &self.terminal {
            let mut container = div()
                .relative()
                .flex_1()
                .w_full()
                .min_h_0()
                .overflow_hidden()
                .child(view.clone())
                .child(self.ime_overlay(cx));
            if self.bell_flash {
                container = container.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(6.))
                        .bg(rgb(colors.accent).alpha(0.16))
                        .border_1()
                        .border_color(rgb(colors.accent).alpha(0.55)),
                );
            }
            if let Some((title, body)) = self.attention_banner.as_ref().filter(|_| self.unread_attention) {
                let title = title.clone();
                let body = body.clone();
                container = container.child(
                    div()
                        .id("agent-attention-banner")
                        .absolute()
                        .top(px(6.))
                        .left(px(8.))
                        .right(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(colors.popover_bg())
                        .border_1()
                        .border_color(rgb(colors.warning()).alpha(0.85))
                        .shadow_md()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(
                                    div()
                                        .size(px(7.))
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(rgb(colors.warning())),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(rgb(colors.foreground))
                                        .flex_shrink_0()
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(11.5))
                                        .text_color(rgb(colors.secondary))
                                        .child(body),
                                ),
                        )
                        .child(
                            Button::new("dismiss-agent-banner")
                                .ghost()
                                .xsmall()
                                .icon(Icon::Close)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_attention(cx);
                                })),
                        ),
                );
            }
            if let Some(search_bar) = self.render_search_bar(colors, cx) {
                container = container.child(search_bar);
            }
            root = root.child(container);
        } else {
            root = root.track_focus(&self.focus);
        }

        // Notification Ring (cmux-inspired glowing attention border)
        if self.unread_attention {
            let ring_color = if self.agent_activity == AgentActivity::Error {
                colors.danger
            } else {
                colors.warning()
            };
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(4.))
                    .border_2()
                    .border_color(rgb(ring_color).alpha(0.9)),
            );
        }

        // Ultra-thin top progress strip driven by OSC 9;4
        if self.progress.is_active() {
            let (bar_color, pct) = match self.progress {
                TerminalProgress::Normal(p) => (colors.accent, (p as f32).clamp(4.0, 100.0)),
                TerminalProgress::Error(p) => (colors.danger, p.unwrap_or(100) as f32),
                TerminalProgress::Indeterminate => (colors.accent, 35.0),
                TerminalProgress::Paused(p) => (colors.warning(), p.unwrap_or(60) as f32),
                TerminalProgress::Hidden => (colors.accent, 0.0),
            };
            root = root.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(2.5))
                    .bg(rgb(bar_color).alpha(0.18))
                    .child(
                        div()
                            .h_full()
                            .w(gpui_kit::relative(pct / 100.0))
                            .bg(rgb(bar_color)),
                    ),
            );
        }

        root
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use gpui_terminal::{GridSize, PtyCommand, PtyTransport, TerminalBuilder};

    #[test]
    #[ignore = "release-mode grid throughput benchmark"]
    fn benchmark_grid_output_fragments() {
        let payload = b"X\r\n".repeat(8 * 1024 * 1024);
        let sizes = [3, 96, 4 * 1024, 64 * 1024];
        for round in 0..5 {
            for slot in 0..sizes.len() {
                let chunk_size = sizes[(slot + round) % sizes.len()];
                let mut grid = gpui_terminal::Grid::new(GridSize { columns: 120, rows: 40 });
                let started = Instant::now();
                for chunk in payload.chunks(chunk_size) {
                    grid.feed(chunk);
                }
                let elapsed = started.elapsed();
                assert_eq!(grid.cursor(), (0, 39));
                assert_eq!(grid.row_text(38), "X");
                assert_eq!(grid.row_text(39), "");
                grid.scroll_display(i32::MAX / 2);
                assert_eq!(grid.display_offset(), 10_000);
                assert_eq!(grid.row_text(0), "X");
                eprintln!("grid_replay chunk={chunk_size} round={round} ms={:.3}", elapsed.as_secs_f64() * 1000.0);
            }
        }
    }

    #[test]
    fn local_pty_passes_input_and_size_and_reaps_the_shell() {
        let command = PtyCommand::new("/bin/sh").arg("-c").arg(
            "stty -echo; printf 'READY\\n'; IFS= read -r line; \
             printf 'INPUT=%s\\nSIZE=' \"$line\"; stty size; exec sleep 30",
        );
        let terminal = TerminalBuilder::new()
            .connect(|sink| PtyTransport::spawn(&command, sink))
            .unwrap();
        let screen = || {
            terminal.with_grid(|grid| {
                (0..grid.size().rows)
                    .map(|row| grid.row_text(row))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        };

        wait_for(|| screen().contains("READY"));
        terminal
            .resize(GridSize {
                columns: 100,
                rows: 30,
            })
            .unwrap();
        terminal.write(b"local-check\n").unwrap();
        wait_for(|| {
            let screen = screen();
            screen.contains("INPUT=local-check") && screen.contains("SIZE=30 100")
        });

        assert!(terminal.process_id().is_some());
        terminal.terminate().unwrap();
        wait_for(|| terminal.process_id().is_none() && terminal.exit_report().is_some());
    }

    fn wait_for(mut observed: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !observed() {
            assert!(Instant::now() < deadline, "timed out waiting for the PTY");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn grid_handles_sync_output_osc_search_prompts_and_hyperlinks() {
        use gpui_terminal::{Grid, KeyInput, KeyModes, TerminalProgress, key_to_bytes_with_modes};

        let mut grid = Grid::with_scrollback(GridSize { columns: 80, rows: 5 }, 100);

        // DEC Mode 2026 synchronized output toggles active state cleanly.
        grid.feed(b"\x1b[?2026h");
        assert!(grid.sync_update_active());
        grid.feed(b"\x1b[?2026l");
        assert!(!grid.sync_update_active());

        // OSC 133 prompt marks + scrollback search across history lines.
        grid.feed(b"\x1b]133;A\x07prompt-1\r\n");
        for i in 0..10 {
            grid.feed(format!("log line {i} needle-token\r\n").as_bytes());
        }
        grid.feed(b"\x1b]133;A\x07prompt-2\r\n");

        let matches = grid.search_all("needle-token");
        assert_eq!(matches.len(), 10);
        assert!(matches[0].line < 0, "oldest match should be in scrollback");
        assert!(grid.scroll_to_line(matches[0].line));
        assert!(grid.display_offset() > 0);
        assert!(grid.scroll_to_bottom());
        assert!(grid.jump_to_prompt(true));
        assert!(grid.display_offset() > 0);

        // OSC 8 hyperlink detection.
        let mut link_grid = Grid::new(GridSize { columns: 80, rows: 5 });
        link_grid.feed(b"\x1b]8;;https://portus.dev/docs\x07PortusDocs\x1b]8;;\x07");
        assert_eq!(
            link_grid.url_at(2, 0).as_deref(),
            Some("https://portus.dev/docs")
        );

        // Kitty keyboard protocol & legacy Shift+Enter encoding.
        let mut kitty_grid = Grid::new(GridSize { columns: 80, rows: 5 });
        kitty_grid.feed(b"\x1b[>1u");
        let modes = kitty_grid.key_modes();
        assert!(modes.disambiguate_esc);
        let shift_enter = KeyInput {
            key: "enter",
            key_char: None,
            control: false,
            alt: false,
            shift: true,
            option_as_meta: true,
        };
        assert_eq!(
            key_to_bytes_with_modes(shift_enter, modes),
            Some(b"\x1b[13;2u".to_vec())
        );
        let legacy_modes = KeyModes {
            shift_enter_newline: true,
            ..KeyModes::default()
        };
        assert_eq!(
            key_to_bytes_with_modes(shift_enter, legacy_modes),
            Some(b"\x1b\r".to_vec())
        );

        // OSC 9;4 progress parsing sanity via TerminalProgress.
        assert!(TerminalProgress::Normal(50).is_active());
        assert!(!TerminalProgress::Hidden.is_active());
    }
}
