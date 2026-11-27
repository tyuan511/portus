use std::fs;
use std::path::{Path, PathBuf};

use gpui_kit::{
    AppContext as _, ClickEvent, Context, ElementId, Entity, EventEmitter, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Stateful, Div, Styled as _, Subscription, Window,
    component::{
        Sizable as _,
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
use crate::theme::Colors;

pub enum SftpViewEvent {
    ReturnToTerminal(HostProfile),
}

impl EventEmitter<SftpViewEvent> for SftpView {}

#[derive(Clone)]
struct LocalEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    item_count: Option<usize>,
}

pub struct SftpView {
    store: Entity<SettingsStore>,
    host: HostProfile,
    client: Option<SftpClient>,
    local_dir: PathBuf,
    local_entries: Vec<LocalEntry>,
    local_scan_seq: u64,
    selected_local: Option<PathBuf>,
    remote_dir: String,
    remote_entries: Vec<RemoteEntry>,
    selected_remote: Option<String>,
    mkdir_input: Entity<InputState>,
    mkdir_open: bool,
    status_text: Option<String>,
    prompt_sub: Option<Subscription>,
    _mkdir_events: Subscription,
    _updates: Subscription,
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
        let language = store.read(cx).current.language;
        let mkdir_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text("New folder name…", "输入新建文件夹名称…"))
        });
        let mkdir_events = cx.subscribe_in(&mkdir_input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.create_remote_dir(window, cx);
            }
        });
        let updates = cx.observe(&store, |_, _, cx| cx.notify());
        let mut view = Self {
            store,
            host,
            client: None,
            local_dir,
            local_entries: Vec::new(),
            local_scan_seq: 0,
            selected_local: None,
            remote_dir: "/".into(),
            remote_entries: Vec::new(),
            selected_remote: None,
            mkdir_input,
            mkdir_open: false,
            status_text: None,
            prompt_sub: None,
            _mkdir_events: mkdir_events,
            _updates: updates,
            connected: false,
        };
        view.reload_local(cx);
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
                    self.status_text = Some(target.clone());
                    crate::notify::info(target, window, cx);
                }
            }
            SftpEvent::Prompt(prompt) => {
                self.prompt_sub = crate::notify::push_ssh_prompt(prompt, language, window, cx);
            }
            SftpEvent::Ready { path, entries } => {
                self.connected = true;
                self.prompt_sub = None;
                self.status_text = None;
                self.remote_dir = path;
                self.remote_entries = entries;
            }
            SftpEvent::Listed { path, entries } => {
                self.remote_dir = path;
                self.remote_entries = entries;
            }
            SftpEvent::Status(msg) => {
                self.status_text = Some(msg.clone());
                crate::notify::success(msg, window, cx);
                self.reload_local(cx);
            }
            SftpEvent::Error(err) => {
                self.prompt_sub = None;
                let view = cx.entity().downgrade();
                let title = language.text("SFTP Connection Failed", "SFTP 连接失败");
                let reconnect_label = language.text("Reconnect", "重新连接");
                crate::notify::error_action(
                    title,
                    err,
                    reconnect_label,
                    Some(Icon::Refresh),
                    move |window, cx| {
                        let _ = view.update(cx, |this, cx| this.connect(window, cx));
                    },
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub fn host_profile(&self) -> &HostProfile {
        &self.host
    }

    fn reload_local(&mut self, cx: &mut Context<Self>) {
        self.local_scan_seq = self.local_scan_seq.wrapping_add(1);
        let seq = self.local_scan_seq;
        let dir = self.local_dir.clone();
        let bg = cx.background_executor().spawn(async move { read_local_dir(&dir) });
        cx.spawn(async move |view, cx| {
            let entries = bg.await;
            let _ = view.update(cx, |this, cx| {
                if this.local_scan_seq == seq {
                    this.local_entries = entries;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn cd_local(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.local_dir = path;
        self.selected_local = None;
        self.reload_local(cx);
        cx.notify();
    }

    fn refresh_local(&mut self, cx: &mut Context<Self>) {
        self.reload_local(cx);
    }

    fn cd_remote(&mut self, path: String, cx: &mut Context<Self>) {
        self.selected_remote = None;
        if let Some(client) = &self.client {
            client.send(SftpRequest::ListDir(path));
        }
        cx.notify();
    }

    fn refresh_remote(&mut self, cx: &mut Context<Self>) {
        if let Some(client) = &self.client {
            client.send(SftpRequest::ListDir(self.remote_dir.clone()));
        }
        cx.notify();
    }

    fn upload_file(&mut self, local: PathBuf, name: &str, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        if let Some(client) = &self.client {
            let language = self.store.read(cx).current.language;
            self.status_text = Some(format!(
                "{} {name}…",
                language.text("Uploading", "正在上传")
            ));
            let remote = join_remote_path(&self.remote_dir, name);
            client.send(SftpRequest::Upload { local, remote });
            cx.notify();
        }
    }

    fn upload_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        if !self.connected {
            return;
        }
        if let Some(entry) = self
            .selected_local
            .as_ref()
            .and_then(|p| self.local_entries.iter().find(|e| &e.path == p))
            .filter(|e| !e.is_dir)
            .cloned()
        {
            self.upload_file(entry.path, &entry.name, cx);
        } else {
            crate::notify::info(
                language.text("Select a local file to upload", "请先选中一个本地文件再上传"),
                window,
                cx,
            );
        }
    }

    fn download_file(&mut self, remote: String, name: &str, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        if let Some(client) = &self.client {
            let language = self.store.read(cx).current.language;
            self.status_text = Some(format!(
                "{} {name}…",
                language.text("Downloading", "正在下载")
            ));
            let local = self.local_dir.join(name);
            client.send(SftpRequest::Download { remote, local });
            cx.notify();
        }
    }

    fn download_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        if !self.connected {
            return;
        }
        if let Some(entry) = self
            .selected_remote
            .as_deref()
            .and_then(|p| self.remote_entries.iter().find(|e| e.path == p))
            .filter(|e| !e.is_dir)
            .cloned()
        {
            self.download_file(entry.path, &entry.name, cx);
        } else {
            crate::notify::info(
                language.text(
                    "Select a remote file to download",
                    "请先选中一个远程文件再下载",
                ),
                window,
                cx,
            );
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
            self.mkdir_open = false;
            cx.notify();
        }
    }
}

impl Render for SftpView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors_for(saved.opacity);
        let language = saved.language;
        let opacity = saved.opacity;
        let mono_font = saved.font_family.clone();

        let has_selected_local_file = self
            .selected_local
            .as_ref()
            .and_then(|p| self.local_entries.iter().find(|e| &e.path == p))
            .is_some_and(|e| !e.is_dir);
        let has_selected_remote_file = self
            .selected_remote
            .as_deref()
            .and_then(|p| self.remote_entries.iter().find(|e| e.path == p))
            .is_some_and(|e| !e.is_dir);
        let can_upload = self.connected && has_selected_local_file;
        let can_download = self.connected && has_selected_remote_file;

        let host_for_terminal = self.host.clone();

        let local_pane = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .px(px(14.))
            .py(px(12.))
            .gap(px(8.))
            .border_r_1()
            .border_color(colors.hairline())
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
                            .gap(px(6.))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .truncate()
                                    .child(language.text("Local", "本地 Local")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(
                                pane_icon_btn("local-up", Icon::ArrowUp, colors).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        if let Some(parent) = this.local_dir.parent() {
                                            this.cd_local(parent.to_path_buf(), cx);
                                        }
                                    }),
                                ),
                            )
                            .child(
                                pane_icon_btn("local-refresh", Icon::Refresh, colors).on_click(
                                    cx.listener(|this, _, _, cx| this.refresh_local(cx)),
                                ),
                            )
                            .child(
                                pane_action_btn(
                                    "local-upload",
                                    language.text("Upload →", "上传 →"),
                                    can_upload,
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.upload_selected(window, cx);
                                })),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .bg(colors.surface_glass())
                    .border_1()
                    .border_color(colors.hairline())
                    .font_family(mono_font.clone())
                    .text_size(px(11.))
                    .text_color(rgb(colors.secondary))
                    .truncate()
                    .child(self.local_dir.display().to_string()),
            )
            .child(
                div()
                    .id("sftp-local-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(self.local_entries.is_empty().then(|| {
                        div()
                            .py(px(28.))
                            .flex()
                            .justify_center()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(language.text("Empty directory", "当前目录为空"))
                    }))
                    .children(self.local_entries.iter().enumerate().map(|(ix, entry)| {
                        let path = entry.path.clone();
                        let name = entry.name.clone();
                        let display_name = if entry.is_dir {
                            format!("{name}/")
                        } else {
                            name.clone()
                        };
                        let meta_label = if entry.is_dir {
                            entry
                                .item_count
                                .map(|n| format!("{n} items"))
                                .unwrap_or_else(|| "dir".into())
                        } else {
                            format_size(entry.size)
                        };
                        let is_dir = entry.is_dir;
                        let is_selected = self.selected_local.as_ref() == Some(&entry.path);
                        let upload_path = entry.path.clone();
                        let upload_name = entry.name.clone();
                        let connected = self.connected;
                        let row_group = format!("sftp-local-row-{ix}");
                        let mono = mono_font.clone();

                        div()
                            .group(row_group.clone())
                            .h(px(29.))
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
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.foreground))
                                            .child(display_name),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .font_family(mono)
                                            .text_size(px(10.5))
                                            .text_color(rgb(colors.muted))
                                            .child(meta_label),
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
                            .children((!is_dir && connected).then(|| {
                                div()
                                    .id(("upload-btn", ix))
                                    .size(px(21.))
                                    .rounded(px(5.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .opacity(if is_selected { 0.9 } else { 0.0 })
                                    .group_hover(row_group, |s| s.opacity(1.0))
                                    .hover(|s| s.bg(colors.accent_soft()))
                                    .child(Icon::Upload.render(px(12.), colors.accent))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.upload_file(upload_path.clone(), &upload_name, cx);
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
            .px(px(14.))
            .py(px(12.))
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
                            .gap(px(6.))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .truncate()
                                    .child(format!(
                                        "{} · {}",
                                        language.text("Remote", "远程 Remote"),
                                        self.host.name
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(
                                pane_icon_btn("remote-up", Icon::ArrowUp, colors).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        let parent = parent_remote_path(&this.remote_dir);
                                        this.cd_remote(parent, cx);
                                    }),
                                ),
                            )
                            .child(
                                pane_icon_btn("remote-refresh", Icon::Refresh, colors).on_click(
                                    cx.listener(|this, _, _, cx| this.refresh_remote(cx)),
                                ),
                            )
                            .child(
                                pane_icon_btn_active(
                                    "remote-mkdir-toggle",
                                    Icon::Plus,
                                    self.mkdir_open,
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.mkdir_open = !this.mkdir_open;
                                    cx.notify();
                                })),
                            )
                            .child(
                                pane_action_btn(
                                    "remote-download",
                                    language.text("← Download", "← 下载"),
                                    can_download,
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.download_selected(window, cx);
                                })),
                            )
                            .child(
                                pane_action_btn(
                                    "sftp-back-terminal",
                                    language.text("Terminal", "返回终端"),
                                    false,
                                    colors,
                                )
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(SftpViewEvent::ReturnToTerminal(
                                        host_for_terminal.clone(),
                                    ));
                                })),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .bg(colors.surface_glass())
                    .border_1()
                    .border_color(colors.hairline())
                    .font_family(mono_font.clone())
                    .text_size(px(11.))
                    .text_color(rgb(colors.secondary))
                    .truncate()
                    .child(self.remote_dir.clone()),
            )
            .children(self.mkdir_open.then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.mkdir_input).small().text_xs()),
                    )
                    .child(
                        pane_action_btn(
                            "remote-mkdir-confirm",
                            language.text("Create", "创建"),
                            true,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.create_remote_dir(window, cx);
                        })),
                    )
                    .child(
                        pane_icon_btn("remote-mkdir-cancel", Icon::Close, colors).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.mkdir_open = false;
                                cx.notify();
                            }),
                        ),
                    )
            }))
            .child(
                div()
                    .id("sftp-remote-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children((!self.connected && self.remote_entries.is_empty()).then(|| {
                        div()
                            .py(px(28.))
                            .flex()
                            .justify_center()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(language.text("Connecting SFTP…", "正在连接 SFTP…"))
                    }))
                    .children((self.connected && self.remote_entries.is_empty()).then(|| {
                        div()
                            .py(px(28.))
                            .flex()
                            .justify_center()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(language.text("Empty directory", "当前目录为空"))
                    }))
                    .children(self.remote_entries.iter().enumerate().map(|(ix, entry)| {
                        let path = entry.path.clone();
                        let dl_path = entry.path.clone();
                        let dl_name = entry.name.clone();
                        let rm_path = entry.path.clone();
                        let is_dir = entry.is_dir;
                        let display_name = if is_dir {
                            format!("{}/", entry.name)
                        } else {
                            entry.name.clone()
                        };
                        let meta_label = if is_dir {
                            format_permissions(true, entry.permissions)
                        } else {
                            format_size(entry.size)
                        };
                        let is_selected = self.selected_remote.as_deref() == Some(entry.path.as_str());
                        let row_group = format!("sftp-remote-row-{ix}");
                        let mono = mono_font.clone();

                        div()
                            .group(row_group.clone())
                            .h(px(29.))
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
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.foreground))
                                            .child(display_name),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .font_family(mono)
                                            .text_size(px(10.5))
                                            .text_color(rgb(colors.muted))
                                            .child(meta_label),
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
                                    .id(("download-btn", ix))
                                    .size(px(21.))
                                    .rounded(px(5.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .opacity(if is_selected { 0.9 } else { 0.0 })
                                    .group_hover(row_group.clone(), |s| s.opacity(1.0))
                                    .hover(|s| s.bg(colors.accent_soft()))
                                    .child(Icon::Import.render(px(12.), colors.accent))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.download_file(dl_path.clone(), &dl_name, cx);
                                    }))
                            }))
                            .child(
                                div()
                                    .id(("remote-del", ix))
                                    .size(px(21.))
                                    .rounded(px(5.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .opacity(if is_selected { 0.75 } else { 0.0 })
                                    .group_hover(row_group, |s| s.opacity(1.0))
                                    .hover(|s| s.bg(colors.danger_soft()))
                                    .child(Icon::Trash.render(px(12.), colors.danger))
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

        let status_dot = if self.connected {
            colors.accent
        } else {
            colors.warning()
        };
        let status_left = if self.connected {
            format!(
                "{} · {} (sftp-v3)",
                language.text("SFTP Connected", "SFTP 已连接"),
                self.host.name
            )
        } else {
            format!(
                "{} · {}…",
                language.text("Connecting SFTP", "正在连接 SFTP"),
                self.host.name
            )
        };
        let status_right = self.status_text.clone().unwrap_or_else(|| {
            language
                .text(
                    "Transfer queue idle · Double-click folders, select files to transfer",
                    "传输队列空闲 · 双击目录进入，选中文件即可上传或下载",
                )
                .to_owned()
        });

        let status_bar = div()
            .h(px(26.))
            .w_full()
            .px(px(12.))
            .bg(colors.titlebar_bg(opacity))
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
                    .gap(px(7.))
                    .child(div().size(px(6.)).rounded_full().bg(rgb(status_dot)))
                    .child(status_left),
            )
            .child(
                div()
                    .text_color(rgb(colors.muted))
                    .truncate()
                    .child(status_right),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .text_color(rgb(colors.foreground))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .bg(colors.content_bg(opacity))
                    .flex()
                    .child(local_pane)
                    .child(remote_pane),
            )
            .child(status_bar)
    }
}

fn pane_action_btn(
    id: impl Into<ElementId>,
    label: &'static str,
    highlighted: bool,
    colors: Colors,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(23.))
        .px(px(9.))
        .rounded(px(6.))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if highlighted {
            colors.accent_border()
        } else {
            colors.hairline()
        })
        .bg(if highlighted {
            colors.accent_soft()
        } else {
            colors.surface_glass()
        })
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(if highlighted {
            colors.accent
        } else {
            colors.foreground
        }))
        .hover(move |s| {
            if highlighted {
                s.bg(colors.accent_soft())
            } else {
                s.bg(colors.surface_hover_glass())
                    .border_color(colors.hairline_strong())
            }
        })
        .child(label)
}

fn pane_icon_btn(id: impl Into<ElementId>, icon: Icon, colors: Colors) -> Stateful<Div> {
    pane_icon_btn_active(id, icon, false, colors)
}

fn pane_icon_btn_active(
    id: impl Into<ElementId>,
    icon: Icon,
    active: bool,
    colors: Colors,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(23.))
        .rounded(px(6.))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if active {
            colors.accent_border()
        } else {
            colors.hairline()
        })
        .bg(if active {
            colors.accent_soft()
        } else {
            colors.surface_glass()
        })
        .hover(move |s| {
            if active {
                s.bg(colors.accent_soft())
            } else {
                s.bg(colors.surface_hover_glass())
                    .border_color(colors.hairline_strong())
            }
        })
        .child(icon.render(
            px(12.),
            if active {
                colors.accent
            } else {
                colors.secondary
            },
        ))
}

fn read_local_dir(dir: &Path) -> Vec<LocalEntry> {
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
            let is_dir = meta.is_dir();
            let path = entry.path();
            Some(LocalEntry {
                name,
                path,
                is_dir,
                size: meta.len(),
                item_count: None,
            })
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
    entries
}

fn format_permissions(is_dir: bool, mode: Option<u32>) -> String {
    let Some(m) = mode else {
        return if is_dir { "dir".into() } else { String::new() };
    };
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    for shift in [6, 3, 0] {
        let bits = (m >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

pub(crate) fn format_size(bytes: u64) -> String {
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
