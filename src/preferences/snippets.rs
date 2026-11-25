use std::collections::BTreeMap;

use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{InputEvent, InputState},
    },
    div, px, rgb, transparent_black,
};

use crate::icons::Icon;
use crate::settings::{Language, Settings, SettingsStore, Snippet};
use crate::theme::Colors;

use super::{danger_button, input_field, primary_button, secondary_button, toggle_switch};

pub enum SnippetsEvent {
    Run(Snippet),
}

impl EventEmitter<SnippetsEvent> for SnippetsPage {}

struct SnippetEditor {
    id: Option<u64>,
    name: Entity<InputState>,
    group: Entity<InputState>,
    command: Entity<InputState>,
    add_newline: bool,
}

impl SnippetEditor {
    fn new(
        snippet: Option<&Snippet>,
        id: Option<u64>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let (name_val, group_val, cmd_val, add_newline) = match snippet {
            Some(s) => (
                s.name.clone(),
                s.group.clone(),
                s.command.clone(),
                s.add_newline,
            ),
            None => (String::new(), String::new(), String::new(), true),
        };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Restart Nginx")
                .default_value(name_val)
        });
        let group = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Operations")
                .default_value(group_val)
        });
        let command = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("sudo systemctl restart nginx && systemctl status nginx")
                .default_value(cmd_val)
        });
        Self {
            id,
            name,
            group,
            command,
            add_newline,
        }
    }
}

pub struct SnippetsPage {
    store: Entity<SettingsStore>,
    search: Entity<InputState>,
    editor: Option<SnippetEditor>,
    _updates: [Subscription; 2],
}

impl SnippetsPage {
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = store.read(cx).current.language;
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Search snippets…", "搜索命令片段…"))
        });
        let updates = [
            cx.subscribe(&search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.observe(&store, |_, _, cx| cx.notify()),
        ];
        Self {
            store,
            search,
            editor: None,
            _updates: updates,
        }
    }

    fn language(&self, cx: &App) -> Language {
        self.store.read(cx).current.language
    }

    fn saved_snippet(&self, id: u64, cx: &App) -> Option<Snippet> {
        self.store
            .read(cx)
            .current
            .snippets
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    pub(crate) fn edit_snippet(
        &mut self,
        id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snippet = id.and_then(|id| self.saved_snippet(id, cx));
        let editor = SnippetEditor::new(snippet.as_ref(), snippet.as_ref().map(|s| s.id), window, cx);
        editor.name.update(cx, |field, cx| field.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    fn save(
        &mut self,
        change: impl FnOnce(&mut Settings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = self.store.update(cx, |store, cx| {
            let result = store.update(change);
            if result.is_ok() {
                cx.notify();
            }
            result
        });
        if let Err(error) = result {
            crate::notify::error(error, window, cx);
            false
        } else {
            true
        }
    }

    fn save_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else { return };
        let language = self.language(cx);
        let name = editor.name.read(cx).value().trim().to_owned();
        let group = editor.group.read(cx).value().trim().to_owned();
        let command = editor.command.read(cx).value().to_string();
        if name.is_empty() {
            crate::notify::error(
                language.text("Enter a snippet name.", "请输入片段名称。"),
                window,
                cx,
            );
            return;
        }
        if command.trim().is_empty() {
            crate::notify::error(
                language.text("Enter a command or script.", "请输入命令或脚本内容。"),
                window,
                cx,
            );
            return;
        }

        let allocation = self.store.read(cx).current.snippet_ids(1);
        let Some(id) = editor.id.or_else(|| allocation.map(|r| r.start)) else {
            return;
        };
        let snippet = Snippet {
            id,
            name,
            command,
            group,
            add_newline: editor.add_newline,
        };
        if self.save(
            |settings| {
                if let Some(existing) = settings.snippets.iter_mut().find(|s| s.id == id) {
                    *existing = snippet;
                } else {
                    settings.snippets.push(snippet);
                    settings.next_snippet_id = id + 1;
                }
            },
            window,
            cx,
        ) {
            if let Some(editor) = &mut self.editor {
                editor.id = Some(id);
            }
            crate::notify::success(
                language.text("Snippet saved.", "命令片段已保存。"),
                window,
                cx,
            );
            cx.notify();
        }
    }

    fn run_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(snippet) = self
            .editor
            .as_ref()
            .and_then(|e| e.id)
            .and_then(|id| self.saved_snippet(id, cx))
        {
            cx.emit(SnippetsEvent::Run(snippet));
        }
    }

    fn duplicate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut snippet) = self
            .editor
            .as_ref()
            .and_then(|e| e.id)
            .and_then(|id| self.saved_snippet(id, cx))
        else {
            return;
        };
        snippet
            .name
            .push_str(self.language(cx).text(" copy", " 副本"));
        let editor = SnippetEditor::new(Some(&snippet), None, window, cx);
        editor.name.update(cx, |field, cx| field.focus(window, cx));
        self.editor = Some(editor);
        cx.notify();
    }

    fn remove_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editor.as_ref().and_then(|e| e.id) else {
            return;
        };
        let language = self.language(cx);
        if self.save(|settings| settings.snippets.retain(|s| s.id != id), window, cx) {
            self.editor = None;
            crate::notify::success(
                language.text("Snippet removed.", "命令片段已删除。"),
                window,
                cx,
            );
            cx.notify();
        }
    }

    fn snippet_list(
        &self,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let selected = self.editor.as_ref().and_then(|e| e.id);
        let query = self.search.read(cx).value().trim().to_lowercase();
        let snippets = &self.store.read(cx).current.snippets;
        let total = snippets.len();

        let mut grouped: BTreeMap<String, Vec<&Snippet>> = BTreeMap::new();
        for snippet in snippets {
            let matches = query.is_empty()
                || snippet.name.to_lowercase().contains(&query)
                || snippet.command.to_lowercase().contains(&query)
                || snippet.group.to_lowercase().contains(&query);
            if matches {
                grouped
                    .entry(snippet.group.trim().to_owned())
                    .or_default()
                    .push(snippet);
            }
        }

        let mut entries = div()
            .id("snippet-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.));

        if grouped.is_empty() {
            entries = entries.child(
                div()
                    .px(px(10.))
                    .py(px(18.))
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(rgb(colors.muted))
                    .child(if query.is_empty() {
                        language.text("No snippets yet", "暂无命令片段")
                    } else {
                        language.text("No matching snippets", "没有匹配的命令片段")
                    }),
            );
        }

        for (group_name, mut items) in grouped {
            items.sort_by_cached_key(|s| s.name.to_lowercase());
            if !group_name.is_empty() {
                entries = entries.child(
                    div()
                        .px(px(8.))
                        .pt(px(8.))
                        .pb(px(2.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.muted))
                        .child(group_name),
                );
            }
            entries = entries.children(items.into_iter().map(|snippet| {
                let id = snippet.id;
                let is_selected = selected == Some(id);
                let preview = snippet
                    .command
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                Button::new(("snippet", id))
                    .ghost()
                    .relative()
                    .w_full()
                    .h(px(52.))
                    .px(px(10.))
                    .rounded_lg()
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
                    .justify_start()
                    .gap(px(10.))
                    .children(is_selected.then(|| {
                        div()
                            .absolute()
                            .left_0()
                            .top(px(10.))
                            .bottom(px(10.))
                            .w(px(2.5))
                            .rounded_r_full()
                            .bg(rgb(colors.accent))
                    }))
                    .child(
                        div()
                            .size(px(30.))
                            .rounded_lg()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if is_selected {
                                colors.accent_soft()
                            } else {
                                rgb(colors.surface)
                            })
                            .child(Icon::Code.render(
                                px(14.),
                                if is_selected { colors.accent } else { colors.muted },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(1.))
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(snippet.name.clone()),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.muted))
                                    .child(preview),
                            ),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.edit_snippet(Some(id), window, cx)),
                    )
            }));
        }

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(260.))
            .bg(rgb(colors.panel))
            .border_r_1()
            .border_color(colors.hairline())
            .child(
                div()
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
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .child(language.text("Snippets", "命令片段")),
                            )
                            .child(
                                div()
                                    .px(px(7.))
                                    .py(px(2.))
                                    .rounded_full()
                                    .bg(rgb(colors.surface))
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(total.to_string()),
                            ),
                    )
                    .child(
                        primary_button(
                            "add-snippet",
                            Some(Icon::Plus),
                            language.text("New Snippet", "新建片段"),
                            colors,
                        )
                        .w_full()
                        .h(px(30.))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.edit_snippet(None, window, cx)),
                        ),
                    )
                    .child(input_field(&self.search, Some(Icon::Search), colors)),
            )
            .child(entries)
    }

    fn empty_state(
        &self,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p(px(32.))
            .gap(px(12.))
            .child(
                div()
                    .size(px(52.))
                    .rounded_2xl()
                    .bg(colors.accent_soft())
                    .border_1()
                    .border_color(colors.accent_border())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::Code.render(px(24.), colors.accent)),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.foreground))
                    .child(language.text("Reusable Snippets", "可复用命令片段")),
            )
            .child(
                div()
                    .max_w(px(340.))
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(rgb(colors.muted))
                    .child(language.text(
                        "Save frequent shell commands and scripts, then run them inside any terminal session in one click.",
                        "保存常用运维命令与脚本，可在任意终端会话中一键下发执行。",
                    )),
            )
            .child(
                primary_button(
                    "empty-add-snippet",
                    Some(Icon::Plus),
                    language.text("New Snippet", "新建片段"),
                    colors,
                )
                .on_click(cx.listener(|this, _, window, cx| this.edit_snippet(None, window, cx))),
            )
    }

    fn snippet_form(
        &self,
        editor: &SnippetEditor,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let saved = editor.id.and_then(|id| self.saved_snippet(id, cx));
        let title = if saved.is_some() {
            language.text("Edit Snippet", "编辑命令片段")
        } else {
            language.text("New Snippet", "新建命令片段")
        };

        let mut actions = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(8.));

        let mut left = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.))
            .child(
                primary_button(
                    "save-snippet",
                    Some(Icon::Check),
                    language.text("Save", "保存"),
                    colors,
                )
                .on_click(cx.listener(|this, _, window, cx| this.save_snippet(window, cx))),
            )
            .child(
                secondary_button(
                    "cancel-snippet",
                    None,
                    language.text("Cancel", "取消"),
                    colors,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.editor = None;
                    cx.notify();
                })),
            );

        if saved.is_some() {
            left = left
                .child(
                    Button::new("run-snippet")
                        .ghost()
                        .small()
                        .h(px(32.))
                        .px(px(12.))
                        .rounded_lg()
                        .bg(colors.success_soft())
                        .border_1()
                        .border_color(colors.success_border())
                        .text_color(rgb(colors.success))
                        .icon(Icon::Play)
                        .label(language.text("Run in Terminal", "在终端运行"))
                        .on_click(cx.listener(|this, _, _, cx| this.run_selected(cx))),
                )
                .child(
                    secondary_button(
                        "duplicate-snippet",
                        Some(Icon::Duplicate),
                        language.text("Duplicate", "副本"),
                        colors,
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.duplicate_selected(window, cx)),
                    ),
                );
        }
        actions = actions.child(left);
        if saved.is_some() {
            let page = cx.entity().downgrade();
            actions = actions.child(
                danger_button(
                    "remove-snippet",
                    Some(Icon::Trash),
                    language.text("Remove", "删除"),
                    false,
                    colors,
                )
                .on_click(move |_, window, cx| {
                    let page = page.clone();
                    crate::notify::confirm(
                        language.text("Remove Snippet", "删除命令片段"),
                        language.text(
                            "Remove this snippet permanently?",
                            "确定要删除这个片段吗？",
                        ),
                        language.text("Remove", "确认删除"),
                        move |window, cx| {
                            let _ = page.update(cx, |this, cx| this.remove_snippet(window, cx));
                        },
                        window,
                        cx,
                    );
                }),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("snippet-fields")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(24.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .max_w(px(620.))
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .p(px(18.))
                                    .rounded_xl()
                                    .bg(rgb(colors.panel))
                                    .border_1()
                                    .border_color(rgb(colors.line))
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.))
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(5.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(colors.secondary))
                                                            .child(language.text("Name", "名称")),
                                                    )
                                                    .child(input_field(&editor.name, None, colors)),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(5.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(colors.secondary))
                                                            .child(language.text("Group", "分组")),
                                                    )
                                                    .child(input_field(&editor.group, None, colors)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(5.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.secondary))
                                                    .child(language.text("Command", "命令")),
                                            )
                                            .child(input_field(&editor.command, Some(Icon::Code), colors)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.))
                                                    .child(
                                                        div()
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(colors.foreground))
                                                            .child(language.text("Send Enter", "自动追加回车")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(rgb(colors.muted))
                                                            .child(language.text(
                                                                "Automatically execute the snippet after pasting it into the terminal.",
                                                                "下发到终端后自动发送换行符立即执行。",
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                toggle_switch(
                                                    "toggle-snippet-newline",
                                                    editor.add_newline,
                                                    colors,
                                                )
                                                .on_change(cx.listener(|this, checked, _, cx| {
                                                    if let Some(editor) = &mut this.editor {
                                                        editor.add_newline = *checked;
                                                    }
                                                    cx.notify();
                                                })),
                                            ),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(12.))
                    .bg(rgb(colors.panel))
                    .border_t_1()
                    .border_color(colors.hairline())
                    .child(actions),
            )
    }
}

impl Render for SnippetsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
        let language = saved.language;
        let mut detail = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        if let Some(editor) = &self.editor {
            detail = detail.child(self.snippet_form(editor, colors, language, cx));
        } else {
            detail = detail.child(self.empty_state(colors, language, cx));
        }
        div()
            .flex()
            .size_full()
            .min_h_0()
            .bg(rgb(colors.background))
            .text_color(rgb(colors.foreground))
            .child(self.snippet_list(colors, language, cx))
            .child(detail)
    }
}
