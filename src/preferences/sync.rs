use std::sync::Arc;

use gpui_kit::{
    AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    component::input::InputState,
    div, px, rgb,
};

use crate::icons::Icon;
use crate::settings::{
    Language, S3SyncConfig, SettingsStore, SyncConflictStrategy, SyncProvider, WebDavSyncConfig,
};
use crate::sync::{SyncManager, SyncStatus, format_timestamp};
use crate::vault::{decrypt_secret, encrypt_secret};

use super::{
    group_title, input_field, inset_card, inset_col_row, inset_row, primary_button, row_text,
    secondary_button, seg_item, segmented_control, toggle_switch, value_pill,
};

pub struct SyncPage {
    store: Entity<SettingsStore>,
    sync_manager: Arc<SyncManager>,

    // S3 fields
    s3_endpoint: Entity<InputState>,
    s3_region: Entity<InputState>,
    s3_bucket: Entity<InputState>,
    s3_access_key: Entity<InputState>,
    s3_secret_key: Entity<InputState>,
    s3_object_key: Entity<InputState>,
    show_s3_secret: bool,

    // WebDAV fields
    webdav_endpoint: Entity<InputState>,
    webdav_username: Entity<InputState>,
    webdav_password: Entity<InputState>,
    webdav_remote_path: Entity<InputState>,
    show_webdav_password: bool,

    // E2EE passphrase
    passphrase: Entity<InputState>,
    show_passphrase: bool,

    _subs: Vec<Subscription>,
}

impl SyncPage {
    pub fn new(
        store: Entity<SettingsStore>,
        sync_manager: Arc<SyncManager>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (sync, language) = {
            let current = &store.read(cx).current;
            (current.sync.clone(), current.language)
        };

        let s3_secret_plain = decrypt_secret(&sync.s3.secret_access_key).unwrap_or_default();
        let webdav_pass_plain = decrypt_secret(&sync.webdav.password).unwrap_or_default();
        let pass_plain = decrypt_secret(&sync.encryption_passphrase).unwrap_or_default();

        let s3_endpoint = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://s3.us-east-1.amazonaws.com")
                .default_value(sync.s3.endpoint.clone())
        });
        let s3_region = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("us-east-1")
                .default_value(sync.s3.region.clone())
        });
        let s3_bucket = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("my-portus-sync")
                .default_value(sync.s3.bucket.clone())
        });
        let s3_access_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("AKIA...")
                .default_value(sync.s3.access_key_id.clone())
        });
        let s3_secret_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Secret Access Key", "Secret Access Key"))
                .masked(true)
                .default_value(s3_secret_plain)
        });
        let s3_object_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("portus/config.json")
                .default_value(sync.s3.object_key.clone())
        });

        let webdav_endpoint = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://dav.jianguoyun.com/dav/Portus")
                .default_value(sync.webdav.endpoint.clone())
        });
        let webdav_username = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Username or email", "用户名或邮箱"))
                .default_value(sync.webdav.username.clone())
        });
        let webdav_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Password or app password", "密码或应用专用密码"))
                .masked(true)
                .default_value(webdav_pass_plain)
        });
        let webdav_remote_path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("portus-config.json")
                .default_value(sync.webdav.remote_path.clone())
        });

        let passphrase = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text(
                    "Optional master passphrase to encrypt cloud backup (recommended)",
                    "可选：主密码用于端到端加密云端配置（强烈推荐）",
                ))
                .masked(true)
                .default_value(pass_plain)
        });

        let sub = cx.observe(&store, |_, _, cx| cx.notify());

        Self {
            store,
            sync_manager,
            s3_endpoint,
            s3_region,
            s3_bucket,
            s3_access_key,
            s3_secret_key,
            s3_object_key,
            show_s3_secret: false,
            webdav_endpoint,
            webdav_username,
            webdav_password,
            webdav_remote_path,
            show_webdav_password: false,
            passphrase,
            show_passphrase: false,
            _subs: vec![sub],
        }
    }

    fn language(&self, cx: &Context<Self>) -> Language {
        self.store.read(cx).current.language
    }

    fn persist_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let s3_endpoint = self.s3_endpoint.read(cx).value().trim().to_owned();
        let s3_region = self.s3_region.read(cx).value().trim().to_owned();
        let s3_bucket = self.s3_bucket.read(cx).value().trim().to_owned();
        let s3_access_key = self.s3_access_key.read(cx).value().trim().to_owned();
        let s3_secret_raw = self.s3_secret_key.read(cx).value().to_string();
        let s3_object_key = self.s3_object_key.read(cx).value().trim().to_owned();

        let webdav_endpoint = self.webdav_endpoint.read(cx).value().trim().to_owned();
        let webdav_username = self.webdav_username.read(cx).value().trim().to_owned();
        let webdav_pass_raw = self.webdav_password.read(cx).value().to_string();
        let webdav_remote_path = self.webdav_remote_path.read(cx).value().trim().to_owned();

        let passphrase_raw = self.passphrase.read(cx).value().to_string();

        let s3_secret_enc = match encrypt_secret(&s3_secret_raw) {
            Ok(v) => v,
            Err(err) => {
                crate::notify::error(err, window, cx);
                return false;
            }
        };
        let webdav_pass_enc = match encrypt_secret(&webdav_pass_raw) {
            Ok(v) => v,
            Err(err) => {
                crate::notify::error(err, window, cx);
                return false;
            }
        };
        let passphrase_enc = match encrypt_secret(&passphrase_raw) {
            Ok(v) => v,
            Err(err) => {
                crate::notify::error(err, window, cx);
                return false;
            }
        };

        let result = self.store.update(cx, |store, cx| {
            let res = store.update(|s| {
                let path_style = s.sync.s3.path_style;
                s.sync.s3 = S3SyncConfig {
                    endpoint: s3_endpoint,
                    region: if s3_region.is_empty() {
                        "us-east-1".to_owned()
                    } else {
                        s3_region
                    },
                    bucket: s3_bucket,
                    access_key_id: s3_access_key,
                    secret_access_key: s3_secret_enc,
                    object_key: if s3_object_key.is_empty() {
                        "portus/config.json".to_owned()
                    } else {
                        s3_object_key
                    },
                    path_style,
                };
                s.sync.webdav = WebDavSyncConfig {
                    endpoint: webdav_endpoint,
                    username: webdav_username,
                    password: webdav_pass_enc,
                    remote_path: if webdav_remote_path.is_empty() {
                        "portus-config.json".to_owned()
                    } else {
                        webdav_remote_path
                    },
                };
                s.sync.encryption_passphrase = passphrase_enc;
            });
            if res.is_ok() {
                cx.notify();
            }
            res
        });

        match result {
            Ok(()) => {
                cx.notify();
                true
            }
            Err(err) => {
                crate::notify::error(err, window, cx);
                false
            }
        }
    }

    fn save_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.persist_inputs(window, cx) {
            let lang = self.language(cx);
            crate::notify::success(
                lang.text("Sync configuration saved.", "同步配置已保存。"),
                window,
                cx,
            );
        }
    }

    fn set_provider(&mut self, provider: SyncProvider, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.persist_inputs(window, cx);
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| {
                s.sync.provider = provider;
                if provider == SyncProvider::None {
                    s.sync.auto_sync = false;
                }
            });
            if r.is_ok() {
                cx.notify();
            }
            r
        });
        if let Err(err) = res {
            crate::notify::error(err, window, cx);
        }
        cx.notify();
    }

    fn test_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.persist_inputs(window, cx) {
            return;
        }
        let sync = self.store.read(cx).current.sync.clone();
        self.sync_manager.test_connection(sync);
    }

    fn sync_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.persist_inputs(window, cx) {
            return;
        }
        let settings = self.store.read(cx).current.clone();
        self.sync_manager.sync_now(settings, false);
    }

    fn push_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.persist_inputs(window, cx) {
            return;
        }
        let settings = self.store.read(cx).current.clone();
        self.sync_manager.push(settings, false);
    }

    fn pull_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.persist_inputs(window, cx) {
            return;
        }
        let sync = self.store.read(cx).current.sync.clone();
        self.sync_manager.pull(sync.clone(), sync.conflict_strategy, false);
    }

    fn toggle_s3_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_s3_secret = !self.show_s3_secret;
        let masked = !self.show_s3_secret;
        self.s3_secret_key
            .update(cx, |input, cx| input.set_masked(masked, window, cx));
        cx.notify();
    }

    fn toggle_webdav_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_webdav_password = !self.show_webdav_password;
        let masked = !self.show_webdav_password;
        self.webdav_password
            .update(cx, |input, cx| input.set_masked(masked, window, cx));
        cx.notify();
    }

    fn toggle_passphrase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_passphrase = !self.show_passphrase;
        let masked = !self.show_passphrase;
        self.passphrase
            .update(cx, |input, cx| input.set_masked(masked, window, cx));
        cx.notify();
    }
}

fn field_label(text: &'static str, colors: crate::theme::Colors) -> gpui_kit::Div {
    div()
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(colors.secondary))
        .child(text)
}

impl Render for SyncPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = self.store.read(cx).current.clone();
        let colors = saved.theme.colors_for(saved.opacity);
        let language = saved.language;
        let sync = &saved.sync;
        let status = self.sync_manager.status();
        let busy = status.is_busy();

        let (status_label, status_color) = match &status {
            SyncStatus::Idle => (
                if sync.provider == SyncProvider::None {
                    language.text("Disabled", "未启用")
                } else if sync.last_sync_at.is_some() {
                    language.text("Ready", "就绪")
                } else {
                    language.text("Not Synced", "未同步")
                },
                colors.secondary,
            ),
            SyncStatus::Testing => (language.text("Testing…", "测试中…"), colors.accent),
            SyncStatus::Pushing => (language.text("Uploading…", "上传中…"), colors.accent),
            SyncStatus::Pulling => (language.text("Downloading…", "拉取中…"), colors.accent),
            SyncStatus::Syncing => (language.text("Syncing…", "同步中…"), colors.accent),
            SyncStatus::Synced { .. } => (language.text("Synced", "已同步"), colors.success),
            SyncStatus::Error(_) => (language.text("Sync Error", "同步失败"), colors.danger),
        };

        let last_sync_desc = match sync.last_sync_at {
            Some(ts) => format!(
                "{}{}",
                language.text("Last synced: ", "上次同步："),
                format_timestamp(ts)
            ),
            None => language
                .text(
                    "Sync hosts, keychain, snippets, tunnels, and all settings across devices",
                    "在多台设备间同步所有主机配置、密钥库、命令片段、端口转发和偏好设置",
                )
                .to_owned(),
        };

        let provider_seg = segmented_control(
            colors,
            [
                seg_item(
                    "sync-provider-none",
                    sync.provider == SyncProvider::None,
                    language.text("Off", "关闭"),
                    cx.listener(|this, _, window, cx| {
                        this.set_provider(SyncProvider::None, window, cx);
                    }),
                ),
                seg_item(
                    "sync-provider-s3",
                    sync.provider == SyncProvider::S3,
                    "S3",
                    cx.listener(|this, _, window, cx| {
                        this.set_provider(SyncProvider::S3, window, cx);
                    }),
                ),
                seg_item(
                    "sync-provider-webdav",
                    sync.provider == SyncProvider::WebDav,
                    "WebDAV",
                    cx.listener(|this, _, window, cx| {
                        this.set_provider(SyncProvider::WebDav, window, cx);
                    }),
                ),
            ],
        );

        let strategy_seg = segmented_control(
            colors,
            [
                seg_item(
                    "sync-strategy-overwrite",
                    sync.conflict_strategy == SyncConflictStrategy::Overwrite,
                    language.text("Full Overwrite", "完整覆盖"),
                    cx.listener(|this, _, window, cx| {
                        let res = this.store.update(cx, |store, cx| {
                            let r = store
                                .update(|s| s.sync.conflict_strategy = SyncConflictStrategy::Overwrite);
                            if r.is_ok() {
                                cx.notify();
                            }
                            r
                        });
                        if let Err(err) = res {
                            crate::notify::error(err, window, cx);
                        }
                    }),
                ),
                seg_item(
                    "sync-strategy-merge",
                    sync.conflict_strategy == SyncConflictStrategy::Merge,
                    language.text("Merge", "合并"),
                    cx.listener(|this, _, window, cx| {
                        let res = this.store.update(cx, |store, cx| {
                            let r = store
                                .update(|s| s.sync.conflict_strategy = SyncConflictStrategy::Merge);
                            if r.is_ok() {
                                cx.notify();
                            }
                            r
                        });
                        if let Err(err) = res {
                            crate::notify::error(err, window, cx);
                        }
                    }),
                ),
            ],
        );

        let mut overview_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("Sync Backend", "同步后端")),
                                    )
                                    .child(
                                        div()
                                            .px(px(7.))
                                            .py(px(1.))
                                            .rounded(px(999.))
                                            .bg(rgb(status_color).alpha(0.14))
                                            .text_color(rgb(status_color))
                                            .text_size(px(10.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(status_label),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .child(last_sync_desc),
                            ),
                    )
                    .child(provider_seg),
            );

        if let SyncStatus::Error(err) = &status {
            overview_card = overview_card.child(
                inset_row(true, colors).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.5))
                        .text_color(rgb(colors.danger))
                        .child(format!(
                            "{} {err}",
                            language.text("Error:", "错误：")
                        )),
                ),
            );
        }

        if sync.provider != SyncProvider::None {
            overview_card = overview_card
                .child(
                    inset_row(true, colors)
                        .child(row_text(
                            language.text("Automatic Sync", "自动同步"),
                            language.text(
                                "Sync on startup and automatically upload changes in the background",
                                "启动时自动同步，并在修改主机或设置后于后台自动上传",
                            ),
                            colors,
                        ))
                        .child(
                            toggle_switch("sync-auto-toggle", sync.auto_sync, colors).on_click(
                                cx.listener(|this, checked: &bool, window, cx| {
                                    let _ = this.persist_inputs(window, cx);
                                    let v = *checked;
                                    let res = this.store.update(cx, |store, cx| {
                                        let r = store.update(|s| s.sync.auto_sync = v);
                                        if r.is_ok() {
                                            cx.notify();
                                        }
                                        r
                                    });
                                    if let Err(err) = res {
                                        crate::notify::error(err, window, cx);
                                    }
                                }),
                            ),
                        ),
                )
                .child(
                    inset_row(true, colors)
                        .child(row_text(
                            language.text("Pull Strategy", "拉取策略"),
                            language.text(
                                "Overwrite replaces local config completely; Merge keeps local-only items",
                                "完整覆盖将完全以云端为准；合并模式保留仅存于本地的主机与片段",
                            ),
                            colors,
                        ))
                        .child(strategy_seg),
                );
        }

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(group_title(
                language.text("Cloud Configuration Sync", "云端配置同步"),
                colors,
            ))
            .child(overview_card);

        if sync.provider == SyncProvider::S3 {
            let s3_card = inset_card(colors)
                .child(
                    inset_col_row(false, colors)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(field_label(
                                    language.text(
                                        "Endpoint URL (AWS S3 / Cloudflare R2 / MinIO / OSS)",
                                        "服务端点 URL（支持 AWS S3 / Cloudflare R2 / MinIO / 阿里云 OSS 等）",
                                    ),
                                    colors,
                                ))
                                .child(input_field(&self.s3_endpoint, Some(Icon::Globe), colors)),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(5.))
                                        .child(field_label(
                                            language.text("Region", "区域 (Region)"),
                                            colors,
                                        ))
                                        .child(input_field(&self.s3_region, None, colors)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(5.))
                                        .child(field_label(
                                            language.text("Bucket", "存储桶 (Bucket)"),
                                            colors,
                                        ))
                                        .child(input_field(&self.s3_bucket, None, colors)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(field_label(
                                    language.text("Access Key ID", "Access Key ID"),
                                    colors,
                                ))
                                .child(input_field(&self.s3_access_key, Some(Icon::Key), colors)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(field_label(
                                    language.text("Secret Access Key", "Secret Access Key"),
                                    colors,
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div().flex_1().min_w_0().child(input_field(
                                                &self.s3_secret_key,
                                                Some(Icon::Lock),
                                                colors,
                                            )),
                                        )
                                        .child(
                                            secondary_button(
                                                "sync-toggle-s3-secret",
                                                None,
                                                if self.show_s3_secret {
                                                    language.text("Hide", "隐藏")
                                                } else {
                                                    language.text("Show", "显示")
                                                },
                                                colors,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.toggle_s3_secret(window, cx);
                                            })),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(field_label(
                                    language.text("Object Key (Remote Path)", "对象路径 (Object Key)"),
                                    colors,
                                ))
                                .child(input_field(&self.s3_object_key, Some(Icon::Folder), colors)),
                        ),
                )
                .child(
                    inset_row(true, colors)
                        .child(row_text(
                            language.text("Path-Style Addressing", "路径风格访问 (Path-Style)"),
                            language.text(
                                "Use endpoint/bucket/key instead of bucket.endpoint/key (recommended for R2 & MinIO)",
                                "使用 endpoint/bucket/key 格式而非子域名格式（推荐用于 R2 / MinIO 等兼容服务）",
                            ),
                            colors,
                        ))
                        .child(
                            toggle_switch("sync-s3-path-style", sync.s3.path_style, colors)
                                .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                    let v = *checked;
                                    let res = this.store.update(cx, |store, cx| {
                                        let r = store.update(|s| s.sync.s3.path_style = v);
                                        if r.is_ok() {
                                            cx.notify();
                                        }
                                        r
                                    });
                                    if let Err(err) = res {
                                        crate::notify::error(err, window, cx);
                                    }
                                })),
                        ),
                );

            root = root
                .child(group_title(
                    language.text("S3 Storage Configuration", "S3 存储配置"),
                    colors,
                ))
                .child(s3_card);
        } else if sync.provider == SyncProvider::WebDav {
            let webdav_card = inset_card(colors).child(
                inset_col_row(false, colors)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(field_label(
                                language.text(
                                    "WebDAV Server URL (Nextcloud / Jianguoyun / NAS)",
                                    "WebDAV 服务地址（支持坚果云 / Nextcloud / 群晖 NAS 等）",
                                ),
                                colors,
                            ))
                            .child(input_field(&self.webdav_endpoint, Some(Icon::Globe), colors)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(field_label(
                                language.text("Username", "用户名"),
                                colors,
                            ))
                            .child(input_field(&self.webdav_username, None, colors)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(field_label(
                                language.text("Password / App Password", "密码 / 应用专用密码"),
                                colors,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div().flex_1().min_w_0().child(input_field(
                                            &self.webdav_password,
                                            Some(Icon::Lock),
                                            colors,
                                        )),
                                    )
                                    .child(
                                        secondary_button(
                                            "sync-toggle-webdav-pass",
                                            None,
                                            if self.show_webdav_password {
                                                language.text("Hide", "隐藏")
                                            } else {
                                                language.text("Show", "显示")
                                            },
                                            colors,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.toggle_webdav_password(window, cx);
                                        })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(field_label(
                                language.text("Remote File Path", "远端文件路径"),
                                colors,
                            ))
                            .child(input_field(
                                &self.webdav_remote_path,
                                Some(Icon::Folder),
                                colors,
                            )),
                    ),
            );

            root = root
                .child(group_title(
                    language.text("WebDAV Configuration", "WebDAV 服务配置"),
                    colors,
                ))
                .child(webdav_card);
        }

        if sync.provider != SyncProvider::None {
            let e2ee_card = inset_card(colors).child(
                inset_col_row(false, colors)
                    .child(row_text(
                        language.text(
                            "End-to-End Encryption Passphrase (Argon2id + AES-256-GCM)",
                            "端到端加密主密码 (Argon2id + AES-256-GCM)",
                        ),
                        language.text(
                            "Encrypts all hosts, passwords, keychain, snippets, tunnels, and settings before upload. Use the same passphrase on all devices.",
                            "在上传前于本地加密全部主机、密码、密钥库、片段、隧道与设置。请在所有同步设备上填写相同密码。",
                        ),
                        colors,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div().flex_1().min_w_0().child(input_field(
                                    &self.passphrase,
                                    Some(Icon::Lock),
                                    colors,
                                )),
                            )
                            .child(
                                secondary_button(
                                    "sync-toggle-passphrase",
                                    None,
                                    if self.show_passphrase {
                                        language.text("Hide", "隐藏")
                                    } else {
                                        language.text("Show", "显示")
                                    },
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_passphrase(window, cx);
                                })),
                            ),
                    ),
            );

            let actions_card = inset_card(colors)
                .child(
                    inset_col_row(false, colors)
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    primary_button(
                                        "sync-now-btn",
                                        Some(Icon::Refresh),
                                        if busy {
                                            language.text("Working…", "处理中…")
                                        } else {
                                            language.text("Sync Now", "立即同步")
                                        },
                                        colors,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.sync_now(window, cx);
                                    })),
                                )
                                .child(
                                    secondary_button(
                                        "sync-push-btn",
                                        Some(Icon::Upload),
                                        language.text("Push to Cloud", "上传到云端"),
                                        colors,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.push_now(window, cx);
                                    })),
                                )
                                .child(
                                    secondary_button(
                                        "sync-pull-btn",
                                        Some(Icon::Import),
                                        language.text("Pull from Cloud", "从云端拉取"),
                                        colors,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pull_now(window, cx);
                                    })),
                                )
                                .child(
                                    secondary_button(
                                        "sync-test-btn",
                                        Some(Icon::Check),
                                        language.text("Test Connection", "测试连接"),
                                        colors,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.test_connection(window, cx);
                                    })),
                                )
                                .child(
                                    secondary_button(
                                        "sync-save-btn",
                                        None,
                                        language.text("Save", "保存配置"),
                                        colors,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_config(window, cx);
                                    })),
                                ),
                        )
                        .child(
                            div()
                                .pt(px(2.))
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.muted))
                                        .child(language.text("Included:", "同步范围：")),
                                )
                                .child(value_pill(
                                    format!(
                                        "{} {}",
                                        saved.hosts.len(),
                                        language.text("Hosts", "台主机")
                                    ),
                                    colors,
                                ))
                                .child(value_pill(
                                    format!(
                                        "{} {}",
                                        saved.keychain.len(),
                                        language.text("Keys", "项密钥")
                                    ),
                                    colors,
                                ))
                                .child(value_pill(
                                    format!(
                                        "{} {}",
                                        saved.snippets.len(),
                                        language.text("Snippets", "个片段")
                                    ),
                                    colors,
                                ))
                                .child(value_pill(
                                    format!(
                                        "{} {}",
                                        saved.tunnels.len(),
                                        language.text("Tunnels", "条隧道")
                                    ),
                                    colors,
                                ))
                                .child(value_pill(
                                    language.text("All Settings", "全部偏好设置"),
                                    colors,
                                )),
                        ),
                );

            root = root
                .child(group_title(
                    language.text("Encryption", "加密保护"),
                    colors,
                ))
                .child(e2ee_card)
                .child(group_title(
                    language.text("Actions", "同步操作"),
                    colors,
                ))
                .child(actions_card);
        }

        div()
            .id("sync-page-scroll")
            .size_full()
            .overflow_y_scroll()
            .px(px(24.))
            .pt(px(18.))
            .pb(px(32.))
            .child(div().w_full().max_w(px(680.)).child(root))
    }
}
