use std::fs;
use std::path::PathBuf;

use gpui_kit::{
    AppContext as _, ClickEvent, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputState},
        notification::Notification,
    },
    div, px, rgb, transparent_black,
};

use crate::file_icons::classify_file;
use crate::icons::Icon;
use crate::settings::{HostProfile, SettingsStore};
use crate::sftp::{
    RemoteEntry, SftpClient, SftpEvent, SftpRequest, join_remote_path, parent_remote_path,
};

#[derive(Clone)]
struct LocalEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
}

pub struct SftpView {
    store: Entity<SettingsStore>,
    host: HostProfile,
    client: Option<SftpClient>,
    local_dir: PathBuf,
    local_entries: Vec<LocalEntry>,
    selected_local: Option<PathBuf>,
    remote_dir: String,
    remote_entries: Vec<RemoteEntry>,
    selected_remote: Option<String>,
    mkdir_input: Entity<InputState>,
    prompt_sub: Option<Subscription>,
    connected: bool,
}

impl SftpView {
    pub fn new(
        store: Entity<SettingsStore>,
        host: HostProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let local_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let local_entries = read_local_dir(&local_dir);
        let language = store.read(cx).current.language;
        let mkdir_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text("New folder…", "新建文件夹…"))
        });
        let mut view = Self {
            store,
            host,
            client: None,
            local_dir,
            local_entries,
            selected_local: None,
            remote_dir: "/".into(),
            remote_entries: Vec::new(),
            selected_remote: None,
            mkdir_input,
            prompt_sub: None,
            connected: false,
        };
        view.connect(window, cx);
        view
    }

    pub fn shutdown(&mut self) {
        self.prompt_sub = None;
        if let Some(client) = self.client.take() {
            client.send(SftpRequest::Close);
        }
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shutdown();
        self.connected = false;
        self.remote_entries.clear();
        let language = self.store.read(cx).current.language;
        crate::notify::info(
            format!(
                "{} {}…",
                language.text("Connecting SFTP to", "正在连接 SFTP："),
                self.host.endpoint()
            ),
            window,
            cx,
        );
        let all_hosts = self.store.read(cx).current.hosts.clone();
        let (tx, rx) = async_channel::unbounded::<SftpEvent>();
        let client = SftpClient::spawn(self.host.clone(), all_hosts, tx);
        self.client = Some(client);

        cx.spawn_in(window, async move |view, cx| {
            while let Ok(ev) = rx.recv().await {
                let keep = cx.update(|window, cx| {
                    view.update(cx, |this, cx| this.handle_event(ev, window, cx))
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

    fn handle_event(&mut self, event: SftpEvent, window: &mut Window, cx: &mut Context<Self>) {
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
                self.remote_dir = path;
                self.remote_entries = entries;
            }
            SftpEvent::Listed { path, entries } => {
                self.remote_dir = path;
                self.remote_entries = entries;
            }
            SftpEvent::Status(msg) => {
                crate::notify::success(msg, window, cx);
                self.local_entries = read_local_dir(&self.local_dir);
            }
            SftpEvent::Error(err) => {
                self.prompt_sub = None;
                let view = cx.entity().downgrade();
                let reconnect_label = language.text("Reconnect", "重连");
                use gpui_kit::component::WindowExt as _;
                let note = Notification::error(err)
                    .autohide(false)
                    .action(move |_, _, cx| {
                        let view = view.clone();
                        Button::new("sftp-reconnect")
                            .primary()
                            .small()
                            .label(reconnect_label)
                            .on_click(cx.listener(move |note, _, window, cx| {
                                let _ = view.update(cx, |this, cx| this.connect(window, cx));
                                note.dismiss(window, cx);
                            }))
                    });
                window.push_notification(note, cx);
            }
        }
        cx.notify();
    }

    fn cd_local(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.local_dir = path;
        self.local_entries = read_local_dir(&self.local_dir);
        self.selected_local = None;
        cx.notify();
    }

    fn cd_remote(&mut self, path: String, cx: &mut Context<Self>) {
        self.selected_remote = None;
        if let Some(client) = &self.client {
            client.send(SftpRequest::ListDir(path));
        }
        cx.notify();
    }

    fn upload_file(&self, local: PathBuf, name: &str) {
        if let Some(client) = &self.client {
            let remote = join_remote_path(&self.remote_dir, name);
            client.send(SftpRequest::Upload { local, remote });
        }
    }

    fn download_file(&self, remote: String, name: &str) {
        if let Some(client) = &self.client {
            let local = self.local_dir.join(name);
            client.send(SftpRequest::Download { remote, local });
        }
    }

    fn create_remote_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.mkdir_input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        if let Some(client) = &self.client {
            let path = join_remote_path(&self.remote_dir, &name);
            client.send(SftpRequest::Mkdir(path));
            self.mkdir_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
    }
}

impl Render for SftpView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
        let language = saved.language;

        let local_pane = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(rgb(colors.panel))
            .border_1()
            .border_color(colors.hairline_strong())
            .overflow_hidden()
            .child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .child(Icon::HardDrive.render(px(14.), colors.accent))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("Local", "本地文件")),
                                    ),
                            )
                            .child(
                                Button::new("local-up")
                                    .outline()
                                    .xsmall()
                                    .icon(Icon::ArrowUp)
                                    .label(language.text("Up", "上级"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(parent) = this.local_dir.parent() {
                                            this.cd_local(parent.to_path_buf(), cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px(px(9.))
                            .py(px(5.))
                            .rounded_md()
                            .bg(rgb(colors.background))
                            .border_1()
                            .border_color(colors.hairline())
                            .text_size(px(11.5))
                            .text_color(rgb(colors.secondary))
                            .truncate()
                            .child(self.local_dir.display().to_string()),
                    ),
            )
            .child(
                div()
                    .id("sftp-local-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(self.local_entries.iter().enumerate().map(|(ix, entry)| {
                        let path = entry.path.clone();
                        let name = entry.name.clone();
                        let is_dir = entry.is_dir;
                        let is_selected = self.selected_local.as_ref() == Some(&entry.path);
                        let upload_path = entry.path.clone();
                        let upload_name = entry.name.clone();
                        let connected = self.connected;
                        let row_group = format!("sftp-local-row-{ix}");
                        div()
                            .group(row_group.clone())
                            .h(px(34.))
                            .px(px(8.))
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .gap(px(8.))
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
                            .hover(move |s| {
                                if is_selected {
                                    s.bg(colors.accent_subtle())
                                } else {
                                    s.bg(colors.hover_bg())
                                }
                            })
                            .child(
                                div()
                                    .id(("local-item", ix))
                                    .flex()
                                    .items_center()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .child(classify_file(&name, is_dir, false).render(px(15.)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(12.5))
                                            .text_color(rgb(colors.foreground))
                                            .child(name),
                                    )
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, _, cx| {
                                            this.selected_local = Some(path.clone());
                                            if is_dir && event.click_count() >= 2 {
                                                this.cd_local(path.clone(), cx);
                                            } else {
                                                cx.notify();
                                            }
                                        },
                                    )),
                            )
                            .children((!is_dir).then(|| {
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(format_size(entry.size))
                            }))
                            .children((!is_dir && connected).then(|| {
                                Button::new(("upload-btn", ix))
                                    .ghost()
                                    .xsmall()
                                    .h(px(24.))
                                    .px(px(7.))
                                    .rounded_md()
                                    .bg(colors.accent_soft())
                                    .text_color(rgb(colors.accent))
                                    .opacity(if is_selected { 1.0 } else { 0.0 })
                                    .group_hover(row_group, |s| s.opacity(1.0))
                                    .icon(Icon::Upload)
                                    .label(language.text("Upload", "上传"))
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        this.upload_file(upload_path.clone(), &upload_name);
                                    }))
                            }))
                    })),
            );

        let remote_pane = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(rgb(colors.panel))
            .border_1()
            .border_color(colors.hairline_strong())
            .overflow_hidden()
            .child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .size(px(7.))
                                            .rounded_full()
                                            .flex_shrink_0()
                                            .bg(rgb(if self.connected {
                                                colors.success
                                            } else {
                                                colors.accent
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .truncate()
                                            .child(format!(
                                                "{} · {}",
                                                language.text("Remote", "远程主机"),
                                                self.host.name
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .w(px(126.))
                                            .child(Input::new(&self.mkdir_input).small()),
                                    )
                                    .child(
                                        Button::new("remote-mkdir")
                                            .outline()
                                            .xsmall()
                                            .icon(Icon::Plus)
                                            .label(language.text("Folder", "新建"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.create_remote_dir(window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("remote-up")
                                            .outline()
                                            .xsmall()
                                            .icon(Icon::ArrowUp)
                                            .label(language.text("Up", "上级"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let parent = parent_remote_path(&this.remote_dir);
                                                this.cd_remote(parent, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .px(px(9.))
                            .py(px(5.))
                            .rounded_md()
                            .bg(rgb(colors.background))
                            .border_1()
                            .border_color(colors.hairline())
                            .text_size(px(11.5))
                            .text_color(rgb(colors.secondary))
                            .truncate()
                            .child(self.remote_dir.clone()),
                    ),
            )
            .child(
                div()
                    .id("sftp-remote-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(self.remote_entries.iter().enumerate().map(|(ix, entry)| {
                        let path = entry.path.clone();
                        let dl_path = entry.path.clone();
                        let dl_name = entry.name.clone();
                        let rm_path = entry.path.clone();
                        let is_dir = entry.is_dir;
                        let is_selected = self.selected_remote.as_deref() == Some(entry.path.as_str());
                        let row_group = format!("sftp-remote-row-{ix}");
                        div()
                            .group(row_group.clone())
                            .h(px(34.))
                            .px(px(8.))
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .gap(px(8.))
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
                            .hover(move |s| {
                                if is_selected {
                                    s.bg(colors.accent_subtle())
                                } else {
                                    s.bg(colors.hover_bg())
                                }
                            })
                            .child(
                                div()
                                    .id(("remote-item", ix))
                                    .flex()
                                    .items_center()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .child(classify_file(&entry.name, is_dir, false).render(px(15.)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(12.5))
                                            .text_color(rgb(colors.foreground))
                                            .child(entry.name.clone()),
                                    )
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, _, cx| {
                                            this.selected_remote = Some(path.clone());
                                            if is_dir && event.click_count() >= 2 {
                                                this.cd_remote(path.clone(), cx);
                                            } else {
                                                cx.notify();
                                            }
                                        },
                                    )),
                            )
                            .children((!is_dir).then(|| {
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(format_size(entry.size))
                            }))
                            .children((!is_dir).then(|| {
                                Button::new(("download-btn", ix))
                                    .ghost()
                                    .xsmall()
                                    .h(px(24.))
                                    .px(px(7.))
                                    .rounded_md()
                                    .bg(colors.accent_soft())
                                    .text_color(rgb(colors.accent))
                                    .opacity(if is_selected { 1.0 } else { 0.0 })
                                    .group_hover(row_group.clone(), |s| s.opacity(1.0))
                                    .icon(Icon::Import)
                                    .label(language.text("Download", "下载"))
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        this.download_file(dl_path.clone(), &dl_name);
                                    }))
                            }))
                            .child(
                                Button::new(("remote-del", ix))
                                    .ghost()
                                    .xsmall()
                                    .opacity(if is_selected { 0.8 } else { 0.0 })
                                    .group_hover(row_group, |s| s.opacity(1.0))
                                    .icon(Icon::Trash)
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        if let Some(client) = &this.client {
                                            client.send(SftpRequest::Remove {
                                                path: rm_path.clone(),
                                                is_dir,
                                            });
                                        }
                                    })),
                            )
                    })),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors.background))
            .text_color(rgb(colors.foreground))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .flex()
                    .gap(px(14.))
                    .child(local_pane)
                    .child(remote_pane),
            )
    }
}

fn read_local_dir(dir: &PathBuf) -> Vec<LocalEntry> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<LocalEntry> = read_dir
        .filter_map(|res| res.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let meta = entry.metadata().ok()?;
            Some(LocalEntry {
                name,
                path: entry.path(),
                is_dir: meta.is_dir(),
                size: meta.len(),
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
