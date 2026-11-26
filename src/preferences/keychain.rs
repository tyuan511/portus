use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::InputState,
    },
    div, px, rgb, transparent_black,
};

use crate::icons::Icon;
use crate::settings::{KeychainEntry, KeychainKind, Language, SettingsStore};
use crate::theme::Colors;
use crate::vault::{decrypt_secret, encrypt_secret};

use super::{danger_button, input_field, primary_button, secondary_button};

struct KeychainEditor {
    id: Option<u64>,
    kind: KeychainKind,
    name: Entity<InputState>,
    key_path: Entity<InputState>,
    secret: Entity<InputState>,
    notes: Entity<InputState>,
}

impl KeychainEditor {
    fn new(
        entry: Option<&KeychainEntry>,
        language: Language,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let (id, kind, name_v, path_v, secret_v, notes_v) = match entry {
            Some(e) => (
                Some(e.id),
                e.kind,
                e.name.clone(),
                e.key_path.clone(),
                decrypt_secret(&e.secret).unwrap_or_default(),
                e.notes.clone(),
            ),
            None => (
                None,
                KeychainKind::SshKey,
                String::new(),
                "~/.ssh/id_ed25519".into(),
                String::new(),
                String::new(),
            ),
        };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("e.g. prod-ed25519", "例如 id_ed25519_prod"))
                .default_value(name_v)
        });
        let key_path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("~/.ssh/id_ed25519")
                .default_value(path_v)
        });
        let secret = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(language.text("Password (encrypted locally)", "密码（本地加密保存）"))
                .default_value(secret_v)
        });
        let notes = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Optional notes", "可选备注"))
                .default_value(notes_v)
        });
        Self {
            id,
            kind,
            name,
            key_path,
            secret,
            notes,
        }
    }
}

pub struct KeychainPage {
    store: Entity<SettingsStore>,
    editor: Option<KeychainEditor>,
    _updates: Subscription,
}

impl KeychainPage {
    pub fn new(store: Entity<SettingsStore>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let updates = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            store,
            editor: None,
            _updates: updates,
        }
    }

    fn open_editor(
        &mut self,
        entry: Option<&KeychainEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.store.read(cx).current.language;
        let editor = KeychainEditor::new(entry, language, window, cx);
        editor.name.update(cx, |input, cx| input.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.editor = None;
        cx.notify();
    }

    fn browse_key_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select SSH Private Key".into()),
        });
        cx.spawn_in(window, async move |page, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let display = path.to_string_lossy().into_owned();
            let _ = cx.update(|window, cx| {
                let _ = page.update(cx, |this, cx| {
                    if let Some(editor) = &this.editor {
                        editor.key_path.update(cx, |input, cx| {
                            input.set_value(display, window, cx);
                        });
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn save_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            return;
        };
        let language = self.store.read(cx).current.language;
        let name = editor.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            crate::notify::error(
                language.text("Please enter a credential name.", "请输入密钥或凭证名称。"),
                window,
                cx,
            );
            return;
        }
        let kind = editor.kind;
        let key_path = editor.key_path.read(cx).value().trim().to_owned();
        let raw_secret = editor.secret.read(cx).value().to_string();
        let notes = editor.notes.read(cx).value().trim().to_owned();
        let edit_id = editor.id;

        let encrypted_secret = if kind == KeychainKind::Password && !raw_secret.is_empty() {
            match encrypt_secret(&raw_secret) {
                Ok(enc) => enc,
                Err(err) => {
                    crate::notify::error(err, window, cx);
                    return;
                }
            }
        } else {
            String::new()
        };

        let result = self.store.update(cx, |store, cx| {
            let res = store.update(|settings| {
                if let Some(id) = edit_id {
                    if let Some(existing) = settings.keychain.iter_mut().find(|k| k.id == id) {
                        existing.name = name;
                        existing.kind = kind;
                        existing.key_path = if kind == KeychainKind::SshKey {
                            key_path
                        } else {
                            String::new()
                        };
                        existing.secret = encrypted_secret;
                        existing.notes = notes;
                    }
                } else if let Some(ids) = settings.keychain_ids(1) {
                    settings.next_keychain_id = ids.end;
                    settings.keychain.push(KeychainEntry {
                        id: ids.start,
                        name,
                        kind,
                        key_path: if kind == KeychainKind::SshKey {
                            key_path
                        } else {
                            String::new()
                        },
                        secret: encrypted_secret,
                        notes,
                    });
                }
            });
            cx.notify();
            res
        });

        match result {
            Ok(()) => {
                self.editor = None;
                crate::notify::success(
                    language.text("Credential saved.", "已保存密钥/凭证。"),
                    window,
                    cx,
                );
                cx.notify();
            }
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn delete_entry(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let result = self.store.update(cx, |store, cx| {
            let res = store.update(|settings| {
                settings.keychain.retain(|k| k.id != id);
                for host in &mut settings.hosts {
                    if host.keychain_id == Some(id) {
                        host.keychain_id = None;
                    }
                    if host.jump_relay_keychain_id == Some(id) {
                        host.jump_relay_keychain_id = None;
                    }
                }
            });
            cx.notify();
            res
        });
        match result {
            Ok(()) => crate::notify::info(
                language.text("Credential removed.", "已删除密钥/凭证。"),
                window,
                cx,
            ),
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn render_modal(&self, colors: Colors, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(editor) = &self.editor else {
            return div().into_any_element();
        };
        let is_edit = editor.id.is_some();
        let kind = editor.kind;
        let title = if is_edit {
            language.text("Edit Credential", "编辑密钥 / 凭证")
        } else {
            language.text("New Credential", "新建密钥 / 凭证")
        };

        div()
            .id("keychain-modal-backdrop")
            .absolute()
            .inset_0()
            .bg(colors.overlay_backdrop())
            .flex()
            .items_center()
            .justify_center()
            .p(px(20.))
            .on_click(cx.listener(|this, _, _, cx| this.close_editor(cx)))
            .child(
                div()
                    .id("keychain-modal-sheet")
                    .w(px(460.))
                    .max_w_full()
                    .rounded(px(13.))
                    .bg(colors.popover_bg())
                    .border_1()
                    .border_color(colors.hairline_strong())
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(18.))
                            .py(px(13.))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .child(title),
                            )
                            .child(
                                Button::new("kc-modal-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::Close)
                                    .on_click(cx.listener(|this, _, _, cx| this.close_editor(cx))),
                            ),
                    )
                    .child(
                        div()
                            .p(px(18.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div()
                                    .p(px(12.))
                                    .rounded(px(10.))
                                    .bg(colors.surface_glass())
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(4.))
                                            .p(px(2.))
                                            .rounded(px(7.))
                                            .bg(colors.surface_glass())
                                            .border_1()
                                            .border_color(colors.hairline())
                                            .child(self.kind_seg_btn(
                                                "kc-kind-ssh",
                                                language.text("SSH Private Key", "SSH 私钥"),
                                                KeychainKind::SshKey,
                                                kind == KeychainKind::SshKey,
                                                colors,
                                                cx,
                                            ))
                                            .child(self.kind_seg_btn(
                                                "kc-kind-pwd",
                                                language.text("Password", "密码凭证"),
                                                KeychainKind::Password,
                                                kind == KeychainKind::Password,
                                                colors,
                                                cx,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(rgb(colors.secondary))
                                                    .child(language.text("Name", "名称")),
                                            )
                                            .child(input_field(&editor.name, Some(Icon::Key), colors)),
                                    )
                                    .child(if kind == KeychainKind::SshKey {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(rgb(colors.secondary))
                                                    .child(language.text("Private Key File", "私钥文件路径")),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .child(input_field(&editor.key_path, None, colors)),
                                                    )
                                                    .child(
                                                        secondary_button(
                                                            "kc-browse-key",
                                                            Some(Icon::Folder),
                                                            language.text("Browse", "浏览"),
                                                            colors,
                                                        )
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.browse_key_file(window, cx);
                                                        })),
                                                    ),
                                            )
                                            .into_any_element()
                                    } else {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(rgb(colors.secondary))
                                                    .child(language.text("Password", "密码")),
                                            )
                                            .child(input_field(&editor.secret, Some(Icon::Lock), colors))
                                            .into_any_element()
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(rgb(colors.secondary))
                                                    .child(language.text("Notes", "备注")),
                                            )
                                            .child(input_field(&editor.notes, None, colors)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .px(px(18.))
                            .py(px(12.))
                            .border_t_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                secondary_button(
                                    "kc-cancel",
                                    None,
                                    language.text("Cancel", "取消"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.close_editor(cx))),
                            )
                            .child(
                                primary_button(
                                    "kc-save",
                                    Some(Icon::Check),
                                    language.text("Save", "保存"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_editor(window, cx);
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn kind_seg_btn(
        &self,
        id: &'static str,
        label: &'static str,
        target: KeychainKind,
        active: bool,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .flex_1()
            .h(px(26.))
            .rounded(px(5.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
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
                if let Some(editor) = &mut this.editor {
                    editor.kind = target;
                    cx.notify();
                }
            }))
            .child(label)
    }
}

impl Render for KeychainPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
        let vibrancy = saved.vibrancy;
        let opacity = saved.opacity;
        let language = saved.language;
        let entries = saved.keychain.clone();
        let hosts = saved.hosts.clone();

        let header = div()
            .px(px(22.))
            .py(px(14.))
            .border_b_1()
            .border_color(colors.hairline())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.foreground))
                    .child(language.text("Keychain", "密钥库")),
            )
            .child(
                primary_button(
                    "kc-add-btn",
                    Some(Icon::Plus),
                    language.text("New Credential", "新建密钥 / 凭证"),
                    colors,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_editor(None, window, cx);
                })),
            );

        let body = if entries.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .p(px(32.))
                .child(Icon::Key.badge(px(44.)))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.foreground))
                        .child(language.text("No credentials saved yet", "尚未添加密钥或凭证")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(colors.secondary))
                        .child(language.text(
                            "Add an SSH key or encrypted password to reuse across your hosts.",
                            "添加 SSH 私钥或加密密码后，可在新建/编辑主机时直接引用。",
                        )),
                )
                .into_any_element()
        } else {
            div()
                .id("keychain-scroll")
                .flex_1()
                .overflow_y_scroll()
                .px(px(22.))
                .py(px(16.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .px(px(12.))
                        .py(px(6.))
                        .border_b_1()
                        .border_color(colors.hairline())
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors.muted))
                        .child(div().w(px(200.)).child(language.text("Name", "名称")))
                        .child(div().w(px(110.)).child(language.text("Type", "类型")))
                        .child(
                            div()
                                .flex_1()
                                .child(language.text("Target / Status", "路径 / 状态")),
                        )
                        .child(div().w(px(90.)).child(language.text("Hosts", "关联主机")))
                        .child(div().w(px(72.))),
                )
                .children(entries.into_iter().map(|entry| {
                    let id = entry.id;
                    let linked_count = hosts.iter().filter(|h| h.keychain_id == Some(id)).count();
                    let kind_label = match entry.kind {
                        KeychainKind::SshKey => "SSH Key",
                        KeychainKind::Password => "Password",
                    };
                    let detail = match entry.kind {
                        KeychainKind::SshKey => {
                            if entry.key_path.is_empty() {
                                "—".to_owned()
                            } else {
                                entry.key_path.clone()
                            }
                        }
                        KeychainKind::Password => {
                            language.text("AES-256-GCM Encrypted", "AES-256-GCM 加密").to_owned()
                        }
                    };
                    let entry_icon = match entry.kind {
                        KeychainKind::SshKey => Icon::Key,
                        KeychainKind::Password => Icon::Lock,
                    };
                    let entry_for_edit = entry.clone();

                    div()
                        .id(("kc-row", id))
                        .px(px(12.))
                        .py(px(9.))
                        .rounded(px(8.))
                        .border_b_1()
                        .border_color(colors.hairline())
                        .hover(|s| s.bg(colors.surface_hover_glass()))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .w(px(200.))
                                .flex()
                                .items_center()
                                .gap(px(9.))
                                .child(entry_icon.badge(px(22.)))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(colors.foreground))
                                        .child(entry.name),
                                ),
                        )
                        .child(
                            div().w(px(110.)).flex().items_center().child(
                                div()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(colors.surface_active_glass())
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.secondary))
                                    .child(kind_label),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(rgb(colors.secondary))
                                .child(detail),
                        )
                        .child(
                            div()
                                .w(px(90.))
                                .text_size(px(11.5))
                                .text_color(rgb(colors.muted))
                                .child(format!(
                                    "{linked_count} {}",
                                    language.text("hosts", "台主机")
                                )),
                        )
                        .child(
                            div()
                                .w(px(72.))
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(4.))
                                .child(
                                    Button::new(("kc-edit", id))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::Edit)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_editor(Some(&entry_for_edit), window, cx);
                                        })),
                                )
                                .child(
                                    danger_button(("kc-del", id), Some(Icon::Trash), "", false, colors)
                                        .h(px(24.))
                                        .px(px(6.))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.delete_entry(id, window, cx);
                                        })),
                                ),
                        )
                }))
                .into_any_element()
        };

        div()
            .relative()
            .size_full()
            .bg(colors.content_bg(vibrancy, opacity))
            .text_color(rgb(colors.foreground))
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .children(self.editor.is_some().then(|| self.render_modal(colors, language, cx)))
    }
}
