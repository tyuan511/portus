use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use gpui_kit::{
    AppContext as _, ClickEvent, Context, Entity, EventEmitter, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
    },
    div, px, rgb, transparent_black,
};

use crate::file_icons::classify_file;
use crate::icons::Icon;
use crate::settings::{HostProfile, SettingsStore};
use crate::sftp::{
    RemoteEntry, SftpClient, SftpEvent, SftpRequest, join_remote_path, parent_remote_path,
};
use crate::sftp_view::format_size;
use crate::terminal::TerminalSession;
use crate::theme::Colors;

pub enum FileDrawerEvent {
    InsertPath(String),
    OpenFullSftp(HostProfile),
    Close,
}

impl EventEmitter<FileDrawerEvent> for FileDrawer {}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum DrawerTab {
    #[default]
    Files,
    Changes,
}

#[derive(Clone)]
enum Mode {
    Local {
        root: PathBuf,
    },
    Remote {
        host: HostProfile,
        root: String,
    },
}

#[derive(Clone)]
struct FileItem {
    name: String,
    path: String,
    is_dir: bool,
    is_parent: bool,
    size: u64,
}

#[derive(Clone, Debug)]
struct GitChangeItem {
    status: String,
    path: String,
}

pub struct FileDrawer {
    store: Entity<SettingsStore>,
    tab: DrawerTab,
    mode: Mode,
    ssh_session: Option<Entity<TerminalSession>>,
    client: Option<SftpClient>,
    sftp_events: Option<Task<()>>,
    connected: bool,
    local_entries: Vec<FileItem>,
    local_scan_seq: u64,
    remote_entries: Vec<RemoteEntry>,
    selected_path: Option<String>,
    mkdir_input: Entity<InputState>,
    mkdir_open: bool,
    prompt_sub: Option<Subscription>,
    git_branch: Option<String>,
    git_changes: Vec<GitChangeItem>,
    git_loading: bool,
    git_error: Option<String>,
    git_seq: u64,
    selected_diff_file: Option<String>,
    diff_content: String,
    diff_loading: bool,
    diff_seq: u64,
    _mkdir_events: Subscription,
    _updates: Subscription,
}

impl FileDrawer {
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = store.read(cx).current.language;
        let root = std::env::current_dir()
            .ok()
            .filter(|p| p.parent().is_some())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        let mkdir_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("New folder…", "新建文件夹…"))
        });
        let mkdir_events = cx.subscribe_in(&mkdir_input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.create_folder(window, cx);
            }
        });
        let updates = cx.observe(&store, |_, _, cx| cx.notify());
        let mut drawer = Self {
            store,
            tab: DrawerTab::Files,
            mode: Mode::Local { root },
            ssh_session: None,
            client: None,
            sftp_events: None,
            connected: true,
            local_entries: Vec::new(),
            local_scan_seq: 0,
            remote_entries: Vec::new(),
            selected_path: None,
            mkdir_input,
            mkdir_open: false,
            prompt_sub: None,
            git_branch: None,
            git_changes: Vec::new(),
            git_loading: false,
            git_error: None,
            git_seq: 0,
            selected_diff_file: None,
            diff_content: String::new(),
            diff_loading: false,
            diff_seq: 0,
            _mkdir_events: mkdir_events,
            _updates: updates,
        };
        drawer.reload_local(cx);
        drawer
    }

    fn reload_local(&mut self, cx: &mut Context<Self>) {
        let Mode::Local { root } = &self.mode else {
            return;
        };
        self.local_scan_seq = self.local_scan_seq.wrapping_add(1);
        let seq = self.local_scan_seq;
        let dir = root.clone();
        let bg = cx.background_executor().spawn(async move {
            let mut items = Vec::new();
            collect_local_dir(&dir, &mut items);
            items
        });
        cx.spawn(async move |view, cx| {
            let items = bg.await;
            let _ = view.update(cx, |this, cx| {
                if this.local_scan_seq == seq {
                    this.local_entries = items;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn sync_target(
        &mut self,
        host: Option<&HostProfile>,
        cwd: Option<&str>,
        ssh_session: Option<Entity<TerminalSession>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ssh_session = ssh_session;
        match (host, &mut self.mode) {
            (Some(target), Mode::Remote { host: current, root })
                if current.address == target.address
                    && current.port == target.port
                    && current.username == target.username =>
            {
                if let Some(dir) = cwd.filter(|d| !d.is_empty() && *d != root.as_str()) {
                    *root = dir.to_owned();
                    if self.connected
                        && let Some(client) = &self.client
                    {
                        client.send(SftpRequest::ListDir(root.clone()));
                    }
                    if self.tab == DrawerTab::Changes {
                        self.refresh_git_changes(cx);
                    }
                    cx.notify();
                }
                return;
            }
            (None, Mode::Local { root }) => {
                if let Some(dir) = cwd
                    .map(PathBuf::from)
                    .filter(|p| p.is_dir() && p != root)
                {
                    *root = dir;
                    self.reload_local(cx);
                    if self.tab == DrawerTab::Changes {
                        self.refresh_git_changes(cx);
                    }
                    cx.notify();
                }
                return;
            }
            _ => {}
        }

        self.shutdown_sftp();
        self.remote_entries.clear();
        self.selected_path = None;
        self.mkdir_open = false;
        self.git_branch = None;
        self.git_changes.clear();
        self.selected_diff_file = None;
        self.diff_content.clear();

        if let Some(host) = host {
            let initial_root = cwd
                .filter(|d| !d.is_empty())
                .unwrap_or("/")
                .to_owned();
            self.mode = Mode::Remote {
                host: host.clone(),
                root: initial_root,
            };
            self.connected = false;
            self.connect_sftp(host.clone(), window, cx);
        } else {
            let root = cwd
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .filter(|p| p.parent().is_some())
                })
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("/"));
            self.mode = Mode::Local { root };
            self.connected = true;
            self.reload_local(cx);
            if self.tab == DrawerTab::Changes {
                self.refresh_git_changes(cx);
            }
            cx.notify();
        }
    }

    pub fn focus_path(&mut self, raw_path: &str, cx: &mut Context<Self>) {
        self.tab = DrawerTab::Files;
        let path = Path::new(raw_path);
        match &mut self.mode {
            Mode::Local { root } => {
                if path.is_dir() {
                    *root = path.to_path_buf();
                    self.selected_path = None;
                    self.reload_local(cx);
                } else if let Some(parent) = path.parent().filter(|p| p.is_dir()) {
                    *root = parent.to_path_buf();
                    self.selected_path = Some(raw_path.to_owned());
                    self.reload_local(cx);
                }
            }
            Mode::Remote { root, .. } => {
                let target_dir = if raw_path.ends_with('/') {
                    raw_path.to_owned()
                } else {
                    parent_remote_path(raw_path)
                };
                *root = target_dir.clone();
                self.selected_path = Some(raw_path.to_owned());
                if let Some(client) = &self.client {
                    client.send(SftpRequest::ListDir(target_dir));
                }
            }
        }
        cx.notify();
    }

    pub fn shutdown_sftp(&mut self) {
        self.sftp_events = None;
        self.prompt_sub = None;
        if let Some(client) = self.client.take() {
            client.send(SftpRequest::Close);
        }
    }

    fn connect_sftp(&mut self, host: HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        crate::notify::info(
            format!(
                "{} {}…",
                language.text("Connecting", "正在连接"),
                host.endpoint()
            ),
            window,
            cx,
        );
        let all_hosts = self.store.read(cx).current.hosts.clone();
        let (tx, rx) = async_channel::unbounded::<SftpEvent>();
        let client = SftpClient::spawn(host, all_hosts, tx);
        self.client = Some(client);

        self.sftp_events = Some(cx.spawn_in(window, async move |view, cx| {
            while let Ok(ev) = rx.recv().await {
                let keep = cx.update(|window, cx| {
                    view.update(cx, |this, cx| this.handle_sftp_event(ev, window, cx))
                        .is_ok()
                });
                if !keep.unwrap_or(false) {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn handle_sftp_event(
        &mut self,
        event: SftpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.store.read(cx).current.language;
        match event {
            SftpEvent::Connecting(target) => {
                if !self.connected {
                    crate::notify::info(target, window, cx);
                }
            }
            SftpEvent::Prompt(prompt) => {
                self.prompt_sub = crate::notify::push_ssh_prompt(prompt, language, window, cx);
            }
            SftpEvent::Ready { path, entries } => {
                self.connected = true;
                self.prompt_sub = None;
                self.selected_path = None;
                if let Mode::Remote { root, .. } = &mut self.mode {
                    *root = path;
                }
                self.remote_entries = entries;
            }
            SftpEvent::Listed { path, entries } => {
                self.selected_path = None;
                if let Mode::Remote { root, .. } = &mut self.mode {
                    *root = path;
                }
                self.remote_entries = entries;
            }
            SftpEvent::Status(msg) => {
                crate::notify::success(msg, window, cx);
            }
            SftpEvent::Error(err) => {
                self.prompt_sub = None;
                crate::notify::error(err, window, cx);
            }
        }
        cx.notify();
    }

    fn go_up(&mut self, cx: &mut Context<Self>) {
        self.selected_path = None;
        let mut reload_local = false;
        match &mut self.mode {
            Mode::Local { root } => {
                if let Some(parent) = root.parent() {
                    *root = parent.to_path_buf();
                    reload_local = true;
                }
            }
            Mode::Remote { root, .. } => {
                let parent = parent_remote_path(root);
                if parent != *root {
                    *root = parent.clone();
                    if let Some(client) = &self.client {
                        client.send(SftpRequest::ListDir(parent));
                    }
                }
            }
        }
        if reload_local {
            self.reload_local(cx);
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.tab == DrawerTab::Changes {
            self.refresh_git_changes(cx);
            return;
        }
        match &self.mode {
            Mode::Local { .. } => self.reload_local(cx),
            Mode::Remote { root, .. } => {
                if let Some(client) = &self.client {
                    client.send(SftpRequest::ListDir(root.clone()));
                }
            }
        }
        cx.notify();
    }

    fn refresh_git_changes(&mut self, cx: &mut Context<Self>) {
        self.git_seq = self.git_seq.wrapping_add(1);
        let seq = self.git_seq;
        self.git_loading = true;
        self.git_error = None;

        match &self.mode {
            Mode::Local { root } => {
                let dir = root.clone();
                let bg = cx.background_executor().spawn(async move {
                    let output = Command::new("git")
                        .arg("-C")
                        .arg(&dir)
                        .args(["status", "--porcelain=v1", "-b"])
                        .output()
                        .map_err(|e| format!("Failed to run git: {e}"))?;
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                        return Err(if stderr.is_empty() {
                            "Not a Git repository.".to_owned()
                        } else {
                            stderr
                        });
                    }
                    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
                });
                cx.spawn(async move |view, cx| {
                    let result = bg.await;
                    let _ = view.update(cx, |this, cx| {
                        if this.git_seq != seq {
                            return;
                        }
                        this.apply_git_status_result(result, cx);
                    });
                })
                .detach();
            }
            Mode::Remote { root, .. } => {
                let Some(session) = &self.ssh_session else {
                    self.git_loading = false;
                    self.git_error = Some("No active SSH session.".to_owned());
                    cx.notify();
                    return;
                };
                let cmd = format!(
                    "git -C {} status --porcelain=v1 -b 2>&1",
                    shell_words::quote(root)
                );
                let Some(rx) = session.read(cx).exec_remote_command(cmd) else {
                    self.git_loading = false;
                    self.git_error = Some("SSH command channel unavailable.".to_owned());
                    cx.notify();
                    return;
                };
                cx.spawn(async move |view, cx| {
                    let result = rx
                        .await
                        .unwrap_or_else(|_| Err("SSH session closed.".to_owned()));
                    let _ = view.update(cx, |this, cx| {
                        if this.git_seq != seq {
                            return;
                        }
                        this.apply_git_status_result(result, cx);
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    fn apply_git_status_result(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        self.git_loading = false;
        match result {
            Ok(stdout) => {
                if stdout.contains("fatal: not a git repository") {
                    self.git_branch = None;
                    self.git_changes.clear();
                    self.git_error = Some("Not a Git repository.".to_owned());
                } else {
                    let (branch, items) = parse_git_porcelain(&stdout);
                    self.git_branch = branch;
                    self.git_changes = items;
                    self.git_error = None;
                    if let Some(first) = self.git_changes.first().cloned()
                        && self.selected_diff_file.is_none()
                    {
                        self.load_file_diff(first.path, first.status == "??", cx);
                    }
                }
            }
            Err(err) => {
                self.git_branch = None;
                self.git_changes.clear();
                self.git_error = Some(err);
            }
        }
        cx.notify();
    }

    fn load_file_diff(&mut self, rel_path: String, is_untracked: bool, cx: &mut Context<Self>) {
        self.diff_seq = self.diff_seq.wrapping_add(1);
        let seq = self.diff_seq;
        self.selected_diff_file = Some(rel_path.clone());
        self.diff_loading = true;
        self.diff_content.clear();

        match &self.mode {
            Mode::Local { root } => {
                let dir = root.clone();
                let bg = cx.background_executor().spawn(async move {
                    if is_untracked {
                        let full = dir.join(&rel_path);
                        let raw = fs::read_to_string(&full)
                            .unwrap_or_else(|_| "(binary or unreadable file)".to_owned());
                        let formatted = raw
                            .lines()
                            .take(200)
                            .map(|l| format!("+{l}"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        return Ok(formatted);
                    }
                    let output = Command::new("git")
                        .arg("-C")
                        .arg(&dir)
                        .args(["diff", "HEAD", "--", &rel_path])
                        .output()
                        .map_err(|e| e.to_string())?;
                    let text = String::from_utf8_lossy(&output.stdout).into_owned();
                    if text.trim().is_empty() {
                        let unstaged = Command::new("git")
                            .arg("-C")
                            .arg(&dir)
                            .args(["diff", "--", &rel_path])
                            .output()
                            .map_err(|e| e.to_string())?;
                        Ok(String::from_utf8_lossy(&unstaged.stdout).into_owned())
                    } else {
                        Ok(text)
                    }
                });
                cx.spawn(async move |view, cx| {
                    let result = bg.await;
                    let _ = view.update(cx, |this, cx| {
                        if this.diff_seq != seq {
                            return;
                        }
                        this.diff_loading = false;
                        this.diff_content = result.unwrap_or_else(|e| e);
                        cx.notify();
                    });
                })
                .detach();
            }
            Mode::Remote { root, .. } => {
                let Some(session) = &self.ssh_session else {
                    self.diff_loading = false;
                    return;
                };
                let cmd = if is_untracked {
                    format!(
                        "head -n 200 -- {} 2>&1 | sed 's/^/+/'",
                        shell_words::quote(&join_remote_path(root, &rel_path))
                    )
                } else {
                    format!(
                        "git -C {} diff HEAD -- {} 2>&1",
                        shell_words::quote(root),
                        shell_words::quote(&rel_path)
                    )
                };
                let Some(rx) = session.read(cx).exec_remote_command(cmd) else {
                    self.diff_loading = false;
                    return;
                };
                cx.spawn(async move |view, cx| {
                    let result = rx
                        .await
                        .unwrap_or_else(|_| Err("SSH session closed.".to_owned()));
                    let _ = view.update(cx, |this, cx| {
                        if this.diff_seq != seq {
                            return;
                        }
                        this.diff_loading = false;
                        this.diff_content = result.unwrap_or_else(|e| e);
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    fn enter_dir(&mut self, path: String, cx: &mut Context<Self>) {
        self.selected_path = None;
        let mut reload_local = false;
        match &mut self.mode {
            Mode::Local { root } => {
                *root = PathBuf::from(path);
                reload_local = true;
            }
            Mode::Remote { root, .. } => {
                *root = path.clone();
                if let Some(client) = &self.client {
                    client.send(SftpRequest::ListDir(path));
                }
            }
        }
        if reload_local {
            self.reload_local(cx);
        }
        cx.notify();
    }

    fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.mkdir_input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let mut reload_local = false;
        match &self.mode {
            Mode::Local { root } => {
                let target = root.join(&name);
                if let Err(err) = fs::create_dir_all(&target) {
                    crate::notify::error(err.to_string(), window, cx);
                } else {
                    reload_local = true;
                }
            }
            Mode::Remote { root, .. } => {
                if let Some(client) = &self.client {
                    client.send(SftpRequest::Mkdir(join_remote_path(root, &name)));
                }
            }
        }
        if reload_local {
            self.reload_local(cx);
        }
        self.mkdir_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.mkdir_open = false;
        cx.notify();
    }

    fn upload_to_remote(&mut self, cx: &mut Context<Self>) {
        let Mode::Remote { root, .. } = &self.mode else {
            return;
        };
        if !self.connected {
            return;
        }
        let remote_dir = root.clone();
        let language = self.store.read(cx).current.language;
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(language.text("Upload files to server", "选择要上传的文件").into()),
        });
        cx.spawn(async move |view, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                let _ = view.update(cx, |this, _| {
                    if let Some(client) = &this.client {
                        for local in paths {
                            let name = local
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            let remote = join_remote_path(&remote_dir, &name);
                            client.send(SftpRequest::Upload { local, remote });
                        }
                    }
                });
            }
        })
        .detach();
    }

    fn download_remote_file(&mut self, remote: String, name: String, cx: &mut Context<Self>) {
        let default_dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&default_dir, Some(&name));
        cx.spawn(async move |view, cx| {
            if let Ok(Ok(Some(local))) = receiver.await {
                let _ = view.update(cx, |this, _| {
                    if let Some(client) = &this.client {
                        client.send(SftpRequest::Download { remote, local });
                    }
                });
            }
        })
        .detach();
    }

    fn visible_items(&self) -> Vec<FileItem> {
        let mut items = Vec::new();
        match &self.mode {
            Mode::Local { root } => {
                if let Some(parent) = root.parent() {
                    items.push(FileItem {
                        name: "..".into(),
                        path: parent.to_string_lossy().into_owned(),
                        is_dir: true,
                        is_parent: true,
                        size: 0,
                    });
                }
                items.extend(self.local_entries.iter().cloned());
            }
            Mode::Remote { root, .. } => {
                if root != "/" {
                    items.push(FileItem {
                        name: "..".into(),
                        path: parent_remote_path(root),
                        is_dir: true,
                        is_parent: true,
                        size: 0,
                    });
                }
                for entry in &self.remote_entries {
                    items.push(FileItem {
                        name: entry.name.clone(),
                        path: entry.path.clone(),
                        is_dir: entry.is_dir,
                        is_parent: false,
                        size: entry.size,
                    });
                }
            }
        }
        items
    }

    fn render_changes_body(
        &self,
        colors: Colors,
        language: crate::settings::Language,
        mono_font: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let branch_label = self
            .git_branch
            .as_deref()
            .unwrap_or(language.text("Git Changes", "Git 变更"))
            .to_owned();
        let selected_file = self.selected_diff_file.clone();
        let changes = self.git_changes.clone();
        let diff_lines: Vec<String> = self
            .diff_content
            .lines()
            .take(180)
            .map(str::to_owned)
            .collect();

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .bg(colors.surface_glass())
                    .border_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_family(mono_font.clone())
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.foreground))
                            .truncate()
                            .child(format!("⎇ {branch_label}")),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(rgb(colors.muted))
                            .child(format!(
                                "{} {}",
                                changes.len(),
                                language.text("changed", "项变更")
                            )),
                    ),
            )
            .children(self.git_error.as_ref().map(|err| {
                div()
                    .py(px(16.))
                    .px(px(8.))
                    .text_center()
                    .text_size(px(11.5))
                    .text_color(rgb(colors.muted))
                    .child(err.clone())
            }))
            .children((self.git_error.is_none() && !self.git_loading && changes.is_empty()).then(
                || {
                    div()
                        .py(px(20.))
                        .text_center()
                        .text_size(px(11.5))
                        .text_color(rgb(colors.muted))
                        .child(language.text("Working tree clean", "工作区干净，暂无变更"))
                },
            ))
            .children((!changes.is_empty()).then(|| {
                div()
                    .id("git-changes-list")
                    .flex_shrink_0()
                    .max_h(px(190.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(changes.into_iter().enumerate().map(|(ix, item)| {
                        let is_selected = selected_file.as_deref() == Some(item.path.as_str());
                        let status_color = match item.status.trim() {
                            "??" | "A" => colors.success,
                            "D" => colors.danger,
                            _ => colors.warning(),
                        };
                        let click_path = item.path.clone();
                        let untracked = item.status == "??";
                        let insert_path = item.path.clone();
                        let mono = mono_font.clone();
                        div()
                            .id(("git-change-row", ix))
                            .flex_shrink_0()
                            .h(px(26.))
                            .px(px(7.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .bg(if is_selected {
                                colors.accent_soft()
                            } else {
                                transparent_black().into()
                            })
                            .hover(|s| s.bg(colors.surface_hover_glass()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.load_file_diff(click_path.clone(), untracked, cx);
                            }))
                            .child(
                                div()
                                    .w(px(18.))
                                    .flex_shrink_0()
                                    .font_family(mono)
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(status_color))
                                    .child(item.status),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.foreground))
                                    .child(item.path),
                            )
                            .child(
                                Button::new(("git-insert-path", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::Terminal)
                                    .tooltip(language.text("Insert path", "插入路径到终端"))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.emit(FileDrawerEvent::InsertPath(insert_path.clone()));
                                    })),
                            )
                    }))
            }))
            .children(selected_file.map(|file| {
                let mono = mono_font.clone();
                div()
                    .flex_1()
                    .min_h_0()
                    .rounded(px(7.))
                    .bg(colors.term_bg(100))
                    .border_1()
                    .border_color(colors.hairline())
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(8.))
                            .py(px(4.))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .font_family(mono.clone())
                            .text_size(px(10.5))
                            .text_color(rgb(colors.secondary))
                            .truncate()
                            .child(file),
                    )
                    .child(
                        div()
                            .id(("git-diff-scroll", self.diff_seq))
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .p(px(6.))
                            .font_family(mono)
                            .text_size(px(10.5))
                            .line_height(px(16.))
                            .child(
                                div()
                                    .min_w_full()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .children(if self.diff_loading {
                                        vec![
                                            div()
                                                .flex_shrink_0()
                                                .text_color(rgb(colors.muted))
                                                .child(language.text(
                                                    "Loading diff…",
                                                    "正在加载 Diff…",
                                                ))
                                                .into_any_element(),
                                        ]
                                    } else if diff_lines.is_empty() {
                                        vec![
                                            div()
                                                .flex_shrink_0()
                                                .text_color(rgb(colors.muted))
                                                .child(language.text(
                                                    "No diff preview",
                                                    "无可展示的差异",
                                                ))
                                                .into_any_element(),
                                        ]
                                    } else {
                                        diff_lines
                                            .into_iter()
                                            .map(|line| {
                                                let (fg, bg) = if line.starts_with('+')
                                                    && !line.starts_with("+++")
                                                {
                                                    (
                                                        colors.success,
                                                        Some(rgb(colors.success).alpha(0.11)),
                                                    )
                                                } else if line.starts_with('-')
                                                    && !line.starts_with("---")
                                                {
                                                    (
                                                        colors.danger,
                                                        Some(rgb(colors.danger).alpha(0.11)),
                                                    )
                                                } else if line.starts_with("@@") {
                                                    (
                                                        colors.accent,
                                                        Some(rgb(colors.accent).alpha(0.10)),
                                                    )
                                                } else {
                                                    (colors.secondary, None)
                                                };
                                                let mut row = div()
                                                    .flex_shrink_0()
                                                    .min_h(px(18.))
                                                    .px(px(4.))
                                                    .py(px(1.))
                                                    .rounded(px(2.))
                                                    .text_color(rgb(fg))
                                                    .whitespace_nowrap()
                                                    .child(if line.is_empty() {
                                                        " ".to_owned()
                                                    } else {
                                                        line
                                                    });
                                                if let Some(bg) = bg {
                                                    row = row.bg(bg);
                                                }
                                                row.into_any_element()
                                            })
                                            .collect()
                                    }),
                            ),
                    )
            }))
    }
}

impl Render for FileDrawer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors_for(saved.opacity);
        let language = saved.language;
        let mono_font = saved.font_family.clone();

        let is_remote = matches!(self.mode, Mode::Remote { .. });
        let current_path = match &self.mode {
            Mode::Local { root } => root.display().to_string(),
            Mode::Remote { root, .. } => root.clone(),
        };
        let remote_host = match &self.mode {
            Mode::Remote { host, .. } => Some(host.clone()),
            Mode::Local { .. } => None,
        };
        let drawer_title = match &self.mode {
            Mode::Remote { host, .. } => format!("{} · {}", language.text("Files", "文件"), host.name),
            Mode::Local { .. } => language.text("Files", "文件").to_owned(),
        };
        let items = self.visible_items();
        let selected_path = self.selected_path.clone();

        let header = div()
            .h(px(38.))
            .px(px(12.))
            .border_b_1()
            .border_color(colors.hairline())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.foreground))
                    .truncate()
                    .child(drawer_title),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .children((self.tab == DrawerTab::Files).then(|| {
                        Button::new("drawer-files-up")
                            .ghost()
                            .xsmall()
                            .tooltip(language.text("Parent folder", "上级目录"))
                            .icon(Icon::ArrowUp)
                            .on_click(cx.listener(|this, _, _, cx| this.go_up(cx)))
                    }))
                    .children((self.tab == DrawerTab::Files).then(|| {
                        Button::new("drawer-files-mkdir")
                            .ghost()
                            .xsmall()
                            .tooltip(language.text("New folder", "新建文件夹"))
                            .icon(Icon::Plus)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mkdir_open = !this.mkdir_open;
                                cx.notify();
                            }))
                    }))
                    .child(
                        Button::new("drawer-files-refresh")
                            .ghost()
                            .xsmall()
                            .tooltip(language.text("Refresh", "刷新"))
                            .icon(Icon::Refresh)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
                    .children(remote_host.map(|host| {
                        Button::new("drawer-open-sftp")
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(
                                "Open dual-pane SFTP",
                                "升级为双栏全屏 SFTP",
                            ))
                            .icon(Icon::Expand)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(FileDrawerEvent::OpenFullSftp(host.clone()))
                            }))
                    }))
                    .child(
                        Button::new("drawer-files-close")
                            .ghost()
                            .xsmall()
                            .tooltip(language.text("Close drawer", "关闭抽屉"))
                            .icon(Icon::Close)
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(FileDrawerEvent::Close);
                            })),
                    ),
            );

        let tab_switch = {
            let tab_btn = |id: &'static str, kind: DrawerTab, label: &'static str| {
                let active = self.tab == kind;
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
                        this.tab = kind;
                        if kind == DrawerTab::Changes {
                            this.refresh_git_changes(cx);
                        }
                        cx.notify();
                    }))
                    .child(label)
            };
            div()
                .p(px(2.))
                .rounded(px(7.))
                .bg(colors.surface_glass())
                .border_1()
                .border_color(colors.hairline())
                .flex()
                .gap(px(2.))
                .child(tab_btn(
                    "fd-tab-files",
                    DrawerTab::Files,
                    language.text("Files", "文件浏览"),
                ))
                .child(tab_btn(
                    "fd-tab-changes",
                    DrawerTab::Changes,
                    language.text("Git Changes", "Git 变更"),
                ))
        };

        let body = if self.tab == DrawerTab::Changes {
            self.render_changes_body(colors, language, mono_font.clone(), cx)
                .into_any_element()
        } else {
            let file_list = div()
                .id("file-drawer-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(items.into_iter().enumerate().map(|(ix, item)| {
                    let icon = classify_file(&item.name, item.is_dir, false);
                    let path = item.path.clone();
                    let dl_path = item.path.clone();
                    let dl_name = item.name.clone();
                    let insert_path = item.path.clone();
                    let is_dir = item.is_dir;
                    let is_parent = item.is_parent;
                    let is_selected = selected_path.as_deref() == Some(item.path.as_str());
                    let display_name = if is_dir && !is_parent {
                        format!("{}/", item.name)
                    } else {
                        item.name.clone()
                    };
                    let size_label = if is_parent {
                        String::new()
                    } else if is_dir {
                        "dir".to_owned()
                    } else {
                        format_size(item.size)
                    };
                    let row_group = format!("drawer-file-row-{ix}");
                    let mono = mono_font.clone();

                    div()
                        .group(row_group.clone())
                        .flex_shrink_0()
                        .h(px(28.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .bg(if is_selected {
                            colors.accent_soft()
                        } else {
                            transparent_black().into()
                        })
                        .hover(move |s| {
                            if is_selected {
                                s.bg(colors.accent_soft())
                            } else {
                                s.bg(colors.surface_hover_glass())
                            }
                        })
                        .child(
                            div()
                                .id(("file-row", ix))
                                .flex()
                                .items_center()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .gap(px(8.))
                                .cursor_pointer()
                                .child(icon.render(px(15.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(rgb(if is_parent {
                                            colors.secondary
                                        } else {
                                            colors.foreground
                                        }))
                                        .child(display_name),
                                )
                                .children((!size_label.is_empty()).then(|| {
                                    div()
                                        .flex_shrink_0()
                                        .font_family(mono)
                                        .text_size(px(10.5))
                                        .text_color(rgb(colors.muted))
                                        .child(size_label)
                                }))
                                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                    if is_parent {
                                        this.selected_path = Some(path.clone());
                                        if event.click_count() >= 2 {
                                            this.go_up(cx);
                                        } else {
                                            cx.notify();
                                        }
                                    } else if is_dir {
                                        this.selected_path = Some(path.clone());
                                        if event.click_count() >= 2 {
                                            this.enter_dir(path.clone(), cx);
                                        } else {
                                            cx.notify();
                                        }
                                    } else {
                                        this.selected_path = Some(path.clone());
                                        cx.emit(FileDrawerEvent::InsertPath(insert_path.clone()));
                                        cx.notify();
                                    }
                                })),
                        )
                        .children((!is_dir && is_remote).then(|| {
                            div()
                                .id(("file-dl", ix))
                                .size(px(20.))
                                .rounded(px(4.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .opacity(if is_selected { 0.9 } else { 0.0 })
                                .group_hover(row_group, |s| s.opacity(1.0))
                                .hover(|s| s.bg(colors.accent_soft()))
                                .child(Icon::Import.render(px(12.), colors.accent))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.download_remote_file(dl_path.clone(), dl_name.clone(), cx)
                                }))
                        }))
                }));

            let drop_zone = (is_remote && self.connected).then(|| {
                div()
                    .id("drawer-upload-zone")
                    .mt(px(4.))
                    .px(px(12.))
                    .py(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(colors.hairline_strong())
                    .bg(colors.surface_glass())
                    .hover(|s| {
                        s.bg(colors.surface_hover_glass())
                            .border_color(colors.accent_border())
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .text_size(px(11.))
                    .text_color(rgb(colors.muted))
                    .on_click(cx.listener(|this, _, _, cx| this.upload_to_remote(cx)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_color(rgb(colors.secondary))
                            .child(Icon::Upload.render(px(12.), colors.accent))
                            .child(language.text(
                                "Click to upload local files",
                                "点击选择本地文件直接上传",
                            )),
                    )
                    .child(language.text(
                        "Click a file to insert its path into terminal",
                        "点击文件可将路径插入终端",
                    ))
            });

            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(6.))
                        .bg(colors.surface_glass())
                        .border_1()
                        .border_color(colors.hairline())
                        .font_family(mono_font)
                        .text_size(px(11.))
                        .text_color(rgb(colors.secondary))
                        .truncate()
                        .child(current_path),
                )
                .children(self.mkdir_open.then(|| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.mkdir_input).small().text_xs()),
                        )
                        .child(
                            Button::new("fd-mkdir-btn")
                                .outline()
                                .xsmall()
                                .icon(Icon::Check)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.create_folder(window, cx)
                                })),
                        )
                }))
                .child(file_list)
                .children(drop_zone)
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(tab_switch)
                    .child(body),
            )
    }
}

fn parse_git_porcelain(output: &str) -> (Option<String>, Vec<GitChangeItem>) {
    let mut branch = None;
    let mut items = Vec::new();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            let name = rest.split("...").next().unwrap_or(rest).trim();
            if !name.is_empty() {
                branch = Some(name.to_owned());
            }
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let status = line[..2].trim().to_owned();
        let raw_path = line[3..].trim();
        let path = raw_path
            .rsplit(" -> ")
            .next()
            .unwrap_or(raw_path)
            .trim_matches('"')
            .to_owned();
        if !path.is_empty() {
            items.push(GitChangeItem {
                status: if status.is_empty() { "M".into() } else { status },
                path,
            });
        }
    }
    (branch, items)
}

fn collect_local_dir(dir: &Path, out: &mut Vec<FileItem>) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<(String, PathBuf, bool, u64)> = read_dir
        .filter_map(|r| r.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let meta = e.metadata().ok()?;
            Some((name, e.path(), meta.is_dir(), meta.len()))
        })
        .collect();
    items.sort_by_cached_key(|a| (!a.2, a.0.to_lowercase()));

    for (name, path, is_dir, size) in items {
        out.push(FileItem {
            name,
            path: path.to_string_lossy().into_owned(),
            is_dir,
            is_parent: false,
            size,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::parse_git_porcelain;

    #[test]
    fn parses_git_porcelain_branch_and_changes() {
        let sample = "## main...origin/main [ahead 1]\n M src/terminal.rs\n?? src/agent_bus.rs\nR  old.rs -> new.rs\n";
        let (branch, items) = parse_git_porcelain(sample);
        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].status, "M");
        assert_eq!(items[0].path, "src/terminal.rs");
        assert_eq!(items[1].status, "??");
        assert_eq!(items[1].path, "src/agent_bus.rs");
        assert_eq!(items[2].status, "R");
        assert_eq!(items[2].path, "new.rs");
    }
}
