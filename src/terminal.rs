mod ime;

use std::sync::{Arc, Mutex};
use std::{future, path::Path};

use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, Styled as _, Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
        notification::Notification,
    },
    div, px, rgb,
};
use gpui_terminal::{
    GridSize, PtyCommand, PtyTransport, TerminalBuilder, TerminalEvent, TerminalPalette,
    TerminalStyle, TerminalView, Transport,
};
use ime::Composition;

use crate::icons::Icon;
use crate::protocols::{SerialTransport, TelnetTransport, spawn_mosh_pty};
use crate::settings::{HostProfile, HostProtocol, Language, ShellSettings};
use crate::ssh::{SshTransport, SshUiEvent};
use crate::theme::Colors;

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
    terminal: Option<Entity<TerminalView<SessionTransport>>>,
    focus: FocusHandle,
    log_buffer: Arc<Mutex<Vec<u8>>>,
    colors: Colors,
    palette: TerminalPalette,
    font_family: String,
    font_size: u8,
    language: Language,
    title: String,
    initial_title: String,
    ssh_host: Option<HostProfile>,
    ssh_hosts: Vec<HostProfile>,
    connected: bool,
    status: Option<TerminalStatus>,
    prompt_sub: Option<Subscription>,
    search_open: bool,
    search_input: Option<Entity<InputState>>,
    search_matches: Vec<(usize, usize, usize)>,
    search_index: usize,
    _search_sub: Option<Subscription>,
    composition: Composition,
    _events: Option<Subscription>,
    _shutdown: Option<Subscription>,
}

impl TerminalSession {
    pub fn local(
        colors: Colors,
        palette: TerminalPalette,
        font_family: String,
        font_size: u8,
        language: Language,
        shell: &ShellSettings,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = std::env::current_dir()
            .ok()
            .filter(|path| path.parent().is_some())
            .or_else(dirs::home_dir);
        let initial_title = cwd.as_deref().map(directory_title).unwrap_or_default();
        let mut shell = shell_command(shell);
        if let Some(cwd) = cwd {
            shell = shell.cwd(cwd);
        }
        let log_buffer = Arc::new(Mutex::new(Vec::new()));
        let log_hook = Arc::clone(&log_buffer);
        let result = TerminalBuilder::new()
            .on_output(move |bytes| append_log(&log_hook, bytes))
            .connect(|sink| PtyTransport::spawn(&shell, sink).map(SessionTransport::Local));

        let terminal = match result {
            Ok(terminal) => terminal,
            Err(error) => {
                return Self {
                    terminal: None,
                    focus: cx.focus_handle(),
                    log_buffer,
                    colors,
                    palette,
                    font_family,
                    font_size,
                    language,
                    title: initial_title.clone(),
                    initial_title,
                    ssh_host: None,
                    ssh_hosts: Vec::new(),
                    connected: false,
                    status: Some(TerminalStatus::StartFailed(error.to_string())),
                    prompt_sub: None,
                    search_open: false,
                    search_input: None,
                    search_matches: Vec::new(),
                    search_index: 0,
                    _search_sub: None,
                    composition: Composition::default(),
                    _events: None,
                    _shutdown: None,
                };
            }
        };

        let style = build_terminal_style(&font_family, font_size, palette);
        let view = cx.new(|cx| TerminalView::new(terminal, style, cx));
        let focus = view.read(cx).focus_handle(cx);
        let events = subscribe_terminal_events(&view, cx);
        let shutdown = cx.on_app_quit(|this, cx| {
            this.shutdown(cx);
            future::ready(())
        });

        Self {
            terminal: Some(view),
            focus,
            log_buffer,
            colors,
            palette,
            font_family,
            font_size,
            language,
            title: initial_title.clone(),
            initial_title,
            ssh_host: None,
            ssh_hosts: Vec::new(),
            connected: true,
            status: None,
            prompt_sub: None,
            search_open: false,
            search_input: None,
            search_matches: Vec::new(),
            search_index: 0,
            _search_sub: None,
            composition: Composition::default(),
            _events: Some(events),
            _shutdown: Some(shutdown),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ssh(
        colors: Colors,
        palette: TerminalPalette,
        font_family: String,
        font_size: u8,
        language: Language,
        host: HostProfile,
        all_hosts: Vec<HostProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_title = host.name.clone();
        let mut session = Self {
            terminal: None,
            focus: cx.focus_handle(),
            log_buffer: Arc::new(Mutex::new(Vec::new())),
            colors,
            palette,
            font_family,
            font_size,
            language,
            title: initial_title.clone(),
            initial_title,
            ssh_host: Some(host),
            ssh_hosts: all_hosts,
            connected: false,
            status: None,
            prompt_sub: None,
            search_open: false,
            search_input: None,
            search_matches: Vec::new(),
            search_index: 0,
            _search_sub: None,
            composition: Composition::default(),
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
        crate::notify::info(
            format!(
                "{} {}…",
                self.language.text("Connecting to", "正在连接"),
                endpoint
            ),
            window,
            cx,
        );

        let (ui_tx, ui_rx) = async_channel::unbounded::<SshUiEvent>();
        let log_hook = Arc::clone(&self.log_buffer);
        let protocol = host.protocol;
        let all_hosts = self.ssh_hosts.clone();
        let result = TerminalBuilder::new()
            .on_output(move |bytes| append_log(&log_hook, bytes))
            .connect(|sink| match protocol {
                HostProtocol::Ssh => {
                    SshTransport::spawn(host, all_hosts, sink, ui_tx).map(SessionTransport::Ssh)
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
                let style = build_terminal_style(&self.font_family, self.font_size, self.palette);
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
            SshUiEvent::Prompt(prompt) => {
                self.prompt_sub =
                    crate::notify::push_ssh_prompt(prompt, self.language, window, cx);
            }
            SshUiEvent::Error(error) => {
                self.connected = false;
                self.prompt_sub = None;
                let status = TerminalStatus::SshError(error);
                let msg = status.message(self.language, true);
                self.status = Some(status);
                let session = cx.entity().downgrade();
                let reconnect_label = self.language.text("Reconnect", "重新连接");
                use gpui_kit::component::WindowExt as _;
                let note = Notification::error(msg)
                    .autohide(false)
                    .action(move |_, _, cx| {
                        let session = session.clone();
                        Button::new("ssh-reconnect")
                            .primary()
                            .small()
                            .icon(Icon::Refresh)
                            .label(reconnect_label)
                            .on_click(cx.listener(move |note, _, window, cx| {
                                let _ = session.update(cx, |this, cx| this.reconnect(window, cx));
                                note.dismiss(window, cx);
                            }))
                    });
                window.push_notification(note, cx);
            }
        }
        cx.notify();
    }

    pub fn reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ssh_host.is_some() {
            self.connect_ssh(window, cx);
        }
    }

    pub fn is_ssh(&self) -> bool {
        self.ssh_host.is_some()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
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

    pub fn export_log(&self) -> Vec<u8> {
        self.log_buffer
            .lock()
            .map(|buf| buf.clone())
            .unwrap_or_default()
    }

    pub fn send_bytes(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if let Some(view) = &self.terminal
            && let Err(error) = view.read(cx).terminal().write(bytes)
        {
            self.status = Some(TerminalStatus::IoError(error.to_string()));
            cx.notify();
        }
    }

    pub fn clear_composition(&mut self, cx: &mut Context<Self>) {
        self.composition = Composition::default();
        cx.notify();
    }

    pub fn set_appearance(
        &mut self,
        colors: Colors,
        palette: TerminalPalette,
        font_family: String,
        font_size: u8,
        language: Language,
        cx: &mut Context<Self>,
    ) {
        self.colors = colors;
        self.palette = palette;
        self.font_family = font_family.clone();
        self.font_size = font_size;
        self.language = language;
        if let Some(view) = &self.terminal {
            let style = build_terminal_style(&font_family, font_size, palette);
            view.update(cx, |view, cx| view.set_style(style, cx));
        }
        cx.notify();
    }

    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.close_search(window, cx);
            return;
        }
        self.search_open = true;
        if self.search_input.is_none() {
            let placeholder = self
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
            view.read(cx)
                .terminal()
                .with_grid(|grid| {
                    let _ = grid;
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
        let query = input.read(cx).value().to_lowercase();
        if query.is_empty() {
            cx.notify();
            return;
        }
        if let Some(view) = &self.terminal {
            let matches = view.read(cx).terminal().with_grid(|grid| {
                let rows = grid.size().rows;
                let mut found = Vec::new();
                for row in 0..rows {
                    let text = grid.row_text(row);
                    let lower = text.to_lowercase();
                    let mut start = 0;
                    while let Some(pos) = lower[start..].find(&query) {
                        let byte_start = start + pos;
                        let col_start = lower[..byte_start].chars().count();
                        let col_len = query.chars().count();
                        let col_end = col_start + col_len.saturating_sub(1);
                        found.push((row, col_start, col_end));
                        start = byte_start + query.len().max(1);
                    }
                }
                found
            });
            self.search_matches = matches;
            if !self.search_matches.is_empty() {
                self.search_index = self.search_matches.len() - 1;
                self.highlight_search_match(cx);
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
        let Some(&(row, col_start, col_end)) = self.search_matches.get(self.search_index) else {
            return;
        };
        if let Some(view) = &self.terminal {
            view.update(cx, |view, cx| {
                view.terminal().with_grid(|grid| {
                    // Use Grid selection via mutable access? Note: with_grid gives &Grid.
                    // We can still notify the view so match count updates cleanly.
                    let _ = (grid, row, col_start, col_end);
                });
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
                .child(div().w(px(184.)).child(Input::new(input).small()))
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
        if let Some(view) = self.terminal.take()
            && view.read(cx).terminal().exit_report().is_none()
            && let Err(error) = view.read(cx).terminal().terminate()
        {
            eprintln!("Could not close the session. {error}");
        }
    }
}

fn build_terminal_style(
    font_family: &str,
    font_size: u8,
    palette: TerminalPalette,
) -> TerminalStyle {
    let family = if font_family.trim().is_empty() {
        default_font_family().to_owned()
    } else {
        font_family.trim().to_owned()
    };
    TerminalStyle::new(family, px(f32::from(font_size))).palette(palette)
}

const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;

fn append_log(buffer: &Mutex<Vec<u8>>, bytes: &[u8]) {
    if let Ok(mut guard) = buffer.lock() {
        guard.extend_from_slice(bytes);
        if guard.len() > MAX_LOG_BYTES {
            let drain = guard.len() - MAX_LOG_BYTES;
            guard.drain(..drain);
        }
    }
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
                this.composition = Composition::default();
                if this.status.is_none() || matches!(this.status, Some(TerminalStatus::Connecting(_)))
                {
                    this.status = Some(TerminalStatus::Exited(report.code));
                }
            }
            TerminalEvent::TransportFailed(error)
                if view.read(cx).terminal().exit_report().is_none() =>
            {
                this.connected = false;
                this.status = Some(TerminalStatus::IoError(error.to_string()));
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
            }
            TerminalEvent::ResetTitle => this.title.clone_from(&this.initial_title),
            _ => return,
        }
        cx.notify();
    })
}

fn shell_command(shell: &ShellSettings) -> PtyCommand {
    let mut command = PtyCommand::new(shell.executable());
    if cfg!(unix) && shell.login {
        command = command.arg("-l");
    }
    command
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("TERM_PROGRAM", "Bulwark")
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
        let colors = self.colors;
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(self.palette.background))
            .text_color(rgb(self.palette.foreground))
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
            if let Some(search_bar) = self.render_search_bar(colors, cx) {
                container = container.child(search_bar);
            }
            root = root.child(container);
        } else {
            root = root.track_focus(&self.focus);
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
}
