use std::fs;
use std::path::{Path, PathBuf};

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

pub enum FileDrawerEvent {
    InsertPath(String),
    OpenFullSftp(HostProfile),
    Close,
}

impl EventEmitter<FileDrawerEvent> for FileDrawer {}

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

struct FileItem {
    name: String,
    path: String,
    is_dir: bool,
    is_parent: bool,
    size: u64,
}

pub struct FileDrawer {
    store: Entity<SettingsStore>,
    mode: Mode,
    client: Option<SftpClient>,
    sftp_events: Option<Task<()>>,
    connected: bool,
    remote_entries: Vec<RemoteEntry>,
    selected_path: Option<String>,
    mkdir_input: Entity<InputState>,
    mkdir_open: bool,
    prompt_sub: Option<Subscription>,
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
        Self {
            store,
            mode: Mode::Local { root },
            client: None,
            sftp_events: None,
            connected: true,
            remote_entries: Vec::new(),
            selected_path: None,
            mkdir_input,
            mkdir_open: false,
            prompt_sub: None,
            _mkdir_events: mkdir_events,
            _updates: updates,
        }
    }

    pub fn sync_target(
        &mut self,
        host: Option<&HostProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (host, &self.mode) {
            (Some(target), Mode::Remote { host: current, .. })
                if current.address == target.address
                    && current.port == target.port
                    && current.username == target.username =>
            {
                return;
            }
            (None, Mode::Local { .. }) => return,
            _ => {}
        }

        self.shutdown_sftp();
        self.remote_entries.clear();
        self.selected_path = None;
        self.mkdir_open = false;

        if let Some(host) = host {
            self.mode = Mode::Remote {
                host: host.clone(),
                root: "/".into(),
            };
            self.connected = false;
            self.connect_sftp(host.clone(), window, cx);
        } else {
            let root = std::env::current_dir()
                .ok()
                .filter(|p| p.parent().is_some())
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("/"));
            self.mode = Mode::Local { root };
            self.connected = true;
            cx.notify();
        }
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
        match &mut self.mode {
            Mode::Local { root } => {
                if let Some(parent) = root.parent() {
                    *root = parent.to_path_buf();
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
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Mode::Remote { root, .. } = &self.mode
            && let Some(client) = &self.client
        {
            client.send(SftpRequest::ListDir(root.clone()));
        }
        cx.notify();
    }

    fn enter_dir(&mut self, path: String, cx: &mut Context<Self>) {
        self.selected_path = None;
        match &mut self.mode {
            Mode::Local { root } => {
                *root = PathBuf::from(path);
            }
            Mode::Remote { root, .. } => {
                *root = path.clone();
                if let Some(client) = &self.client {
                    client.send(SftpRequest::ListDir(path));
                }
            }
        }
        cx.notify();
    }

    fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.mkdir_input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        match &self.mode {
            Mode::Local { root } => {
                let target = root.join(&name);
                if let Err(err) = fs::create_dir_all(&target) {
                    crate::notify::error(err.to_string(), window, cx);
                }
            }
            Mode::Remote { root, .. } => {
                if let Some(client) = &self.client {
                    client.send(SftpRequest::Mkdir(join_remote_path(root, &name)));
                }
            }
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
                collect_local_dir(root, &mut items);
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
}

impl Render for FileDrawer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
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
            Mode::Remote { host, .. } => format!("SFTP · {}", host.name),
            Mode::Local { .. } => language.text("Local Files", "本地文件").to_owned(),
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
                    .child(
                        Button::new("drawer-files-up")
                            .ghost()
                            .xsmall()
                            .tooltip(language.text("Parent folder", "上级目录"))
                            .icon(Icon::ArrowUp)
                            .on_click(cx.listener(|this, _, _, cx| this.go_up(cx))),
                    )
                    .child(
                        Button::new("drawer-files-mkdir")
                            .ghost()
                            .xsmall()
                            .tooltip(language.text("New folder", "新建文件夹"))
                            .icon(Icon::Plus)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mkdir_open = !this.mkdir_open;
                                cx.notify();
                            })),
                    )
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
                    .children(drop_zone),
            )
    }
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
    items.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });

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
