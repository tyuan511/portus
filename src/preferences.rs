mod hosts;
mod snippets;
mod tunnels;

use gpui_kit::{
    App, AppContext as _, Context, ElementId, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    component::{
        IndexPath, Sizable as _,
        button::{Button, ButtonVariants as _},
        group_box::{GroupBox, GroupBoxVariants as _},
        input::{Input, InputState},
        select::{Select, SelectEvent, SelectItem, SelectState},
        slider::{Slider, SliderEvent, SliderState},
        switch::Switch,
        tab::{Tab, TabBar},
    },
    div, px, rgb,
};

use crate::icons::Icon;
use crate::settings::{
    DEFAULT_FONT_SIZE, Language, MAX_FONT_SIZE, MIN_FONT_SIZE, MIN_OPACITY, Settings,
    SettingsStore, ShellSettings,
};
use crate::theme::{Colors, Theme, ThemeMode};
use crate::vault::{export_vault, import_vault, merge_vault_into};
pub(crate) use hosts::{HostsEvent, HostsPage};
pub(crate) use snippets::{SnippetsEvent, SnippetsPage};
pub(crate) use tunnels::TunnelsPage;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Appearance,
    Shell,
    Language,
    Vault,
}

impl Section {
    const ALL: [Self; 4] = [Self::Appearance, Self::Shell, Self::Language, Self::Vault];

    fn label(self, language: Language) -> &'static str {
        match self {
            Self::Appearance => language.text("Appearance", "外观"),
            Self::Shell => "Shell",
            Self::Language => language.text("Language", "语言"),
            Self::Vault => language.text("Vault Sync", "加密备份"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Appearance => Icon::Palette,
            Self::Shell => Icon::Terminal,
            Self::Language => Icon::Globe,
            Self::Vault => Icon::Lock,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SelectChoice<V: Clone + PartialEq + 'static> {
    pub title: SharedString,
    pub value: V,
}

impl<V: Clone + PartialEq + 'static> SelectChoice<V> {
    pub(crate) fn new(title: impl Into<SharedString>, value: V) -> Self {
        Self {
            title: title.into(),
            value,
        }
    }
}

impl<V: Clone + PartialEq + 'static> SelectItem for SelectChoice<V> {
    type Value = V;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

fn font_select_items(language: Language) -> Vec<SelectChoice<String>> {
    [
        ("", language.text("System Default", "系统默认")),
        ("Menlo", "Menlo"),
        ("SF Mono", "SF Mono"),
        ("JetBrains Mono", "JetBrains Mono"),
        ("Fira Code", "Fira Code"),
        ("Cascadia Code", "Cascadia Code"),
    ]
    .into_iter()
    .map(|(value, label)| SelectChoice::new(label, value.to_owned()))
    .collect()
}

pub struct Preferences {
    section: Section,
    theme_mode: ThemeMode,
    last_dark: Theme,
    last_light: Theme,
    store: Entity<SettingsStore>,
    shell_path: Entity<InputState>,
    vault_passphrase: Entity<InputState>,
    opacity_slider: Entity<SliderState>,
    font_size_slider: Entity<SliderState>,
    font_select: Entity<SelectState<Vec<SelectChoice<String>>>>,
    login_shell: bool,
    _subs: [Subscription; 3],
}

impl Preferences {
    pub fn new(
        store: Entity<SettingsStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = &store.read(cx).current;
        let language = current.language;
        let current_theme = current.theme;
        let theme_mode = current_theme.mode();
        let last_dark = if theme_mode == ThemeMode::Dark {
            current_theme
        } else {
            Theme::Graphite
        };
        let last_light = if theme_mode == ThemeMode::Light {
            current_theme
        } else {
            Theme::Paper
        };
        let current_opacity = current.opacity;
        let current_font_size = current.font_size;
        let current_font_family = current.font_family.clone();
        let (shell_path, login_shell) = (current.shell.executable.clone(), current.shell.login);
        let shell_path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(
                    ShellSettings::system_executable()
                        .to_string_lossy()
                        .into_owned(),
                )
                .default_value(shell_path)
        });
        let vault_passphrase = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text("Master passphrase…", "输入主密码…"))
                .masked(true)
        });
        let opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(f32::from(MIN_OPACITY))
                .max(100.)
                .step(1.)
                .default_value(f32::from(current_opacity))
        });
        let font_size_slider = cx.new(|_| {
            SliderState::new()
                .min(f32::from(MIN_FONT_SIZE))
                .max(f32::from(MAX_FONT_SIZE))
                .step(1.)
                .default_value(f32::from(current_font_size))
        });
        let font_items = font_select_items(language);
        let selected_font_ix = font_items
            .iter()
            .position(|item| item.value == current_font_family)
            .map(|row| IndexPath::default().row(row))
            .or_else(|| Some(IndexPath::default().row(0)));
        let font_select =
            cx.new(|cx| SelectState::new(font_items, selected_font_ix, window, cx));

        let opacity_sub =
            cx.subscribe_in(&opacity_slider, window, |this, _, event: &SliderEvent, window, cx| {
                if let SliderEvent::Change(val) = event {
                    let next = val.end().round().clamp(f32::from(MIN_OPACITY), 100.) as u8;
                    if this.store.read(cx).current.opacity != next {
                        this.save(move |settings| settings.opacity = next, window, cx);
                    }
                }
            });
        let font_size_sub = cx.subscribe_in(
            &font_size_slider,
            window,
            |this, _, event: &SliderEvent, window, cx| {
                if let SliderEvent::Change(val) = event {
                    let next = val
                        .end()
                        .round()
                        .clamp(f32::from(MIN_FONT_SIZE), f32::from(MAX_FONT_SIZE))
                        as u8;
                    if this.store.read(cx).current.font_size != next {
                        this.save(move |settings| settings.font_size = next, window, cx);
                    }
                }
            },
        );
        let font_select_sub = cx.subscribe_in(
            &font_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<String>>>, window, cx| {
                if let SelectEvent::Confirm(Some(family)) = event
                    && this.store.read(cx).current.font_family != *family
                {
                    let next = family.clone();
                    this.save(move |settings| settings.font_family = next, window, cx);
                }
            },
        );
        if let Some(msg) = store.read(cx).load_error() {
            crate::notify::error(msg.to_owned(), window, cx);
        }
        Self {
            section: Section::Appearance,
            theme_mode,
            last_dark,
            last_light,
            store,
            shell_path,
            vault_passphrase,
            opacity_slider,
            font_size_slider,
            font_select,
            login_shell,
            _subs: [opacity_sub, font_size_sub, font_select_sub],
        }
    }

    fn select_theme(&mut self, choice: Theme, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_mode = choice.mode();
        match self.theme_mode {
            ThemeMode::Dark => self.last_dark = choice,
            ThemeMode::Light => self.last_light = choice,
        }
        self.save(|settings| settings.theme = choice, window, cx);
    }

    fn select_theme_mode(&mut self, mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_mode == mode {
            return;
        }
        self.theme_mode = mode;
        let target = match mode {
            ThemeMode::Dark => self.last_dark,
            ThemeMode::Light => self.last_light,
        };
        self.save(|settings| settings.theme = target, window, cx);
    }

    fn open_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        if let Some(msg) = self.store.read(cx).load_error() {
            crate::notify::error(msg.to_owned(), window, cx);
        }
        cx.notify();
    }

    fn language(&self, cx: &App) -> Language {
        self.store.read(cx).current.language
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
        match result {
            Ok(()) => {
                cx.notify();
                true
            }
            Err(error) => {
                crate::notify::error(error, window, cx);
                cx.notify();
                false
            }
        }
    }

    fn set_language(&mut self, language: Language, window: &mut Window, cx: &mut Context<Self>) {
        if self.save(|settings| settings.language = language, window, cx) {
            let current_font = self.store.read(cx).current.font_family.clone();
            let items = font_select_items(language);
            self.font_select.update(cx, |select, cx| {
                select.set_items(items, window, cx);
                select.set_selected_value(&current_font, window, cx);
            });
        }
    }

    fn save_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.language(cx);
        let shell = ShellSettings {
            executable: self.shell_path.read(cx).value().trim().to_owned(),
            login: self.login_shell,
        };
        if !shell.executable.is_empty() && !is_executable(&shell) {
            crate::notify::error(
                language.text(
                    "Enter the absolute path to an executable shell.",
                    "请输入可执行 Shell 的绝对路径。",
                ),
                window,
                cx,
            );
            return;
        }
        if self.save(move |settings| settings.shell = shell, window, cx) {
            crate::notify::success(
                language.text(
                    "Saved for new terminal tabs.",
                    "已保存，将在下次新建终端标签时生效。",
                ),
                window,
                cx,
            );
        }
    }

    fn appearance(
        &self,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let (theme, opacity) = (saved.theme, saved.opacity);
        let font_size = saved.font_size;

        let themes = Theme::for_mode(self.theme_mode);
        let mode_switch = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .p(px(3.))
            .rounded_lg()
            .bg(rgb(colors.background))
            .border_1()
            .border_color(rgb(colors.line))
            .children(
                [
                    (ThemeMode::Dark, Icon::Moon, language.text("Dark", "深色")),
                    (ThemeMode::Light, Icon::Sun, language.text("Light", "浅色")),
                ]
                .into_iter()
                .enumerate()
                .map(|(ix, (mode, icon, label))| {
                    let active = self.theme_mode == mode;
                    Button::new(("theme-mode", ix))
                        .h(px(28.))
                        .with_variant(if active {
                            gpui_kit::component::button::ButtonVariant::Primary
                        } else {
                            gpui_kit::component::button::ButtonVariant::Ghost
                        })
                        .small()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon.render(
                            px(13.),
                            if active {
                                colors.accent_fg
                            } else {
                                colors.muted
                            },
                        ))
                        .child(label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_theme_mode(mode, window, cx)
                        }))
                }),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                settings_card(colors)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("Theme", "外观与终端主题")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.muted))
                                            .child(language.text(
                                                "Choose Dark or Light mode, then pick a color scheme for both UI and shell.",
                                                "先选择深色或浅色模式，再挑选具体的界面与 Shell 配色方案。",
                                            )),
                                    ),
                            )
                            .child(mode_switch),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(9.))
                            .children(themes.chunks(5).map(|row| {
                                div().flex().gap(px(9.)).children(row.iter().copied().map(
                                    |choice| {
                                        let preview = choice.palette();
                                        let choice_colors = choice.colors();
                                        let selected = theme == choice;
                                        let name = choice.label(language);
                                        div()
                                            .id(choice.name())
                                            .flex_1()
                                            .min_w_0()
                                            .rounded_xl()
                                            .overflow_hidden()
                                            .border_1()
                                            .border_color(if selected {
                                                rgb(colors.accent)
                                            } else {
                                                rgb(colors.line)
                                            })
                                            .bg(rgb(choice_colors.panel))
                                            .cursor_pointer()
                                            .child(
                                                div()
                                                    .w_full()
                                                    .flex()
                                                    .flex_col()
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .h(px(62.))
                                                            .p(px(8.))
                                                            .bg(rgb(preview.background))
                                                            .flex()
                                                            .flex_col()
                                                            .justify_between()
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_between()
                                                                    .child(
                                                                        div()
                                                                            .flex()
                                                                            .items_center()
                                                                            .gap(px(3.5))
                                                                            .child(
                                                                                div()
                                                                                    .size(px(6.))
                                                                                    .rounded_full()
                                                                                    .bg(rgb(
                                                                                        preview.ansi[1],
                                                                                    )),
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .size(px(6.))
                                                                                    .rounded_full()
                                                                                    .bg(rgb(
                                                                                        preview.ansi[3],
                                                                                    )),
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .size(px(6.))
                                                                                    .rounded_full()
                                                                                    .bg(rgb(
                                                                                        preview.ansi[2],
                                                                                    )),
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .w(px(14.))
                                                                            .h(px(4.))
                                                                            .rounded_full()
                                                                            .bg(rgb(
                                                                                choice_colors.accent,
                                                                            )),
                                                                    ),
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap(px(1.5))
                                                                    .text_size(px(9.5))
                                                                    .child(
                                                                        div()
                                                                            .flex()
                                                                            .gap(px(3.))
                                                                            .child(
                                                                                div()
                                                                                    .text_color(rgb(
                                                                                        preview.ansi[4],
                                                                                    ))
                                                                                    .child("$"),
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .text_color(rgb(
                                                                                        preview
                                                                                            .foreground,
                                                                                    ))
                                                                                    .child("ssh prod"),
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .text_color(rgb(
                                                                                preview.ansi[2],
                                                                            ))
                                                                            .child("ok"),
                                                                    ),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .px(px(8.))
                                                            .py(px(6.))
                                                            .border_t_1()
                                                            .border_color(rgb(choice_colors.line))
                                                            .flex()
                                                            .items_center()
                                                            .justify_between()
                                                            .gap(px(4.))
                                                            .child(
                                                                div()
                                                                    .min_w_0()
                                                                    .flex_1()
                                                                    .text_size(px(11.5))
                                                                    .font_weight(if selected {
                                                                        FontWeight::SEMIBOLD
                                                                    } else {
                                                                        FontWeight::MEDIUM
                                                                    })
                                                                    .text_color(rgb(
                                                                        choice_colors.foreground,
                                                                    ))
                                                                    .truncate()
                                                                    .child(name),
                                                            )
                                                            .children(selected.then(|| {
                                                                div()
                                                                    .size(px(14.))
                                                                    .flex_shrink_0()
                                                                    .rounded_full()
                                                                    .bg(rgb(choice_colors.accent))
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_center()
                                                                    .child(Icon::Check.render(
                                                                        px(9.),
                                                                        choice_colors.accent_fg,
                                                                    ))
                                                            })),
                                                    ),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.select_theme(choice, window, cx);
                                            }))
                                    },
                                ))
                            })),
                    ),
            )
            .child(
                settings_card(colors)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("Window Opacity", "窗口不透明度")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.muted))
                                            .child(language.text(
                                                "Adjust translucency of the Bulwark window.",
                                                "调整主窗口的透明程度。",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded_full()
                                    .bg(colors.accent_soft())
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.accent))
                                    .child(format!("{opacity}%")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .pt(px(4.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.muted))
                                    .child(format!("{MIN_OPACITY}%")),
                            )
                            .child(div().flex_1().child(Slider::new(&self.opacity_slider)))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.muted))
                                    .child("100%"),
                            ),
                    ),
            )
            .child(
                settings_card(colors)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("Terminal Font & Size", "终端字体与字号")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.muted))
                                            .child(language.text(
                                                "Applies immediately across all active terminal tabs.",
                                                "立即应用到所有已打开的终端标签页。",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded_full()
                                    .bg(colors.accent_soft())
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.accent))
                                    .child(format!("{font_size} px")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text("Font family", "字体")),
                                    )
                                    .child(
                                        div()
                                            .w(px(240.))
                                            .child(Select::new(&self.font_select)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.muted))
                                            .child(format!("{MIN_FONT_SIZE} px")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .child(Slider::new(&self.font_size_slider)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.muted))
                                            .child(format!("{MAX_FONT_SIZE} px")),
                                    )
                                    .child(
                                        Button::new("font-size-reset")
                                            .ghost()
                                            .small()
                                            .label(language.text("Reset", "默认"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.font_size_slider.update(cx, |slider, cx| {
                                                    slider.set_value(
                                                        f32::from(DEFAULT_FONT_SIZE),
                                                        window,
                                                        cx,
                                                    );
                                                });
                                                this.save(
                                                    |settings| {
                                                        settings.font_size = DEFAULT_FONT_SIZE
                                                    },
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    ),
            )
    }

    fn shell(&self, colors: Colors, language: Language, cx: &Context<Self>) -> impl IntoElement {
        settings_card(colors)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("Shell Executable", "Shell 可执行文件")),
                    )
                    .child(input_field(&self.shell_path, Some(Icon::Terminal), colors))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(language.text(
                                "Leave blank to use your system default shell. Changes apply to new tabs.",
                                "留空时使用系统默认 Shell。修改对新建的终端标签生效。",
                            )),
                    ),
            )
            .child(div().h(px(1.)).w_full().bg(rgb(colors.line_subtle)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(language.text("Login Shell", "登录 Shell")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors.muted))
                                    .child(language.text(
                                        "Start local shells with -l to load profile scripts.",
                                        "以登录模式（-l）启动本地 Shell 以加载环境配置。",
                                    )),
                            ),
                    )
                    .child(
                        toggle_switch("toggle-login-shell", self.login_shell, colors).on_change(
                            cx.listener(|this, checked, _, cx| {
                                this.login_shell = *checked;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(div().h(px(1.)).w_full().bg(rgb(colors.line_subtle)))
            .child(
                div().flex().justify_end().child(
                    primary_button(
                        "save-shell",
                        Some(Icon::Check),
                        language.text("Save Shell Settings", "保存 Shell 设置"),
                        colors,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.save_shell(window, cx))),
                ),
            )
    }

    fn export_vault_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.language(cx);
        let passphrase = self.vault_passphrase.read(cx).value().to_string();
        let bytes = match export_vault(&self.store.read(cx).current, &passphrase) {
            Ok(bytes) => bytes,
            Err(err) => {
                crate::notify::error(err, window, cx);
                return;
            }
        };
        let default_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&default_dir, Some("bulwark.bulwark-vault"));
        let window_handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let res = std::fs::write(&path, bytes);
                let _ = window_handle.update(cx, |_, window, cx| match res {
                    Ok(()) => crate::notify::success(
                        language.text("Encrypted vault exported.", "加密备份已导出。"),
                        window,
                        cx,
                    ),
                    Err(err) => crate::notify::error(err.to_string(), window, cx),
                });
            }
        })
        .detach();
    }

    fn import_vault_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.language(cx);
        let passphrase = self.vault_passphrase.read(cx).value().to_string();
        if passphrase.is_empty() {
            crate::notify::error(
                language.text("Enter the vault master passphrase first.", "请先输入主密码。"),
                window,
                cx,
            );
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                language
                    .text("Choose an encrypted Bulwark vault", "选择加密备份文件")
                    .into(),
            ),
        });
        let window_handle = window.window_handle();
        cx.spawn(async move |view, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let res = std::fs::read(path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| import_vault(&bytes, &passphrase));
                let _ = window_handle.update(cx, |_, window, cx| match res {
                    Ok(payload) => {
                        let _ = view.update(cx, |this, cx| {
                            let mut counts = (0, 0, 0);
                            if this.save(
                                |settings| counts = merge_vault_into(settings, payload),
                                window,
                                cx,
                            ) {
                                let msg = match language {
                                    Language::English => format!(
                                        "Imported {} hosts, {} snippets, {} tunnels.",
                                        counts.0, counts.1, counts.2
                                    ),
                                    Language::ChineseSimplified => format!(
                                        "已导入 {} 台主机、{} 个片段、{} 条隧道。",
                                        counts.0, counts.1, counts.2
                                    ),
                                };
                                crate::notify::success(msg, window, cx);
                            }
                        });
                    }
                    Err(err) => crate::notify::error(err, window, cx),
                });
            }
        })
        .detach();
    }

    fn vault_section(
        &self,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        settings_card(colors)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text(
                                "End-to-End Encrypted Vault",
                                "端到端加密配置备份（Vault）",
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(language.text(
                                "Export or import hosts, snippets, and port forwarding rules encrypted with Argon2id + AES-256-GCM.",
                                "使用 Argon2id + AES-256-GCM 加密导出或导入主机、命令片段与端口转发规则，便于多设备同步。",
                            )),
                    ),
            )
            .child(input_field(&self.vault_passphrase, Some(Icon::Lock), colors))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        primary_button(
                            "export-vault",
                            Some(Icon::Import),
                            language.text("Export Encrypted Vault…", "导出加密备份…"),
                            colors,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.export_vault_file(window, cx)),
                        ),
                    )
                    .child(
                        secondary_button(
                            "import-vault",
                            Some(Icon::Folder),
                            language.text("Import Encrypted Vault…", "导入加密备份…"),
                            colors,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.import_vault_file(window, cx)),
                        ),
                    ),
            )
    }

    fn languages(
        &self,
        colors: Colors,
        language: Language,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        settings_card(colors)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("Interface Language", "界面语言")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(language.text(
                                "Applies immediately across menus, tabs, and dialogs.",
                                "立即应用到菜单、标签栏与所有页面。",
                            )),
                    ),
            )
            .child(
                div().flex().flex_col().gap(px(8.)).children(
                    [
                        (Language::English, "English", "English (United States)"),
                        (Language::ChineseSimplified, "简体中文", "Simplified Chinese"),
                    ]
                    .into_iter()
                    .map(|(choice, label, subtitle)| {
                        let selected = language == choice;
                        Button::new(label)
                            .ghost()
                            .w_full()
                            .h(px(52.))
                            .px(px(14.))
                            .rounded_lg()
                            .border_1()
                            .border_color(if selected {
                                colors.accent_border()
                            } else {
                                rgb(colors.line)
                            })
                            .bg(if selected {
                                colors.accent_soft()
                            } else {
                                rgb(colors.background)
                            })
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .child(label),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.muted))
                                            .child(subtitle),
                                    ),
                            )
                            .child(if selected {
                                div()
                                    .size(px(20.))
                                    .rounded_full()
                                    .bg(rgb(colors.accent))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::Check.render(px(12.), colors.accent_fg))
                            } else {
                                div()
                                    .size(px(20.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(rgb(colors.line))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_language(choice, window, cx)
                            }))
                    }),
                ),
            )
    }
}

impl Render for Preferences {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = &self.store.read(cx).current;
        let colors = saved.theme.colors();
        let language = saved.language;

        let selected_index = Section::ALL
            .iter()
            .position(|&s| s == self.section)
            .unwrap_or(0);
        let tabs = TabBar::new("settings-sections")
            .segmented()
            .selected_index(selected_index)
            .children(Section::ALL.into_iter().map(|section| {
                Tab::new()
                    .prefix(gpui_kit::component::Icon::new(section.icon()))
                    .label(section.label(language))
            }))
            .on_click(cx.listener(|this, ix: &usize, window, cx| {
                if let Some(&section) = Section::ALL.get(*ix) {
                    this.open_section(section, window, cx);
                }
            }));

        let mut content = div()
            .w_full()
            .max_w(px(760.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors.foreground))
                                    .child(language.text("Settings", "设置")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(rgb(colors.muted))
                                    .child(language.text(
                                        "Customize terminal appearance, shell behavior, and language.",
                                        "自定义终端外观、Shell 行为与界面语言。",
                                    )),
                            ),
                    )
                    .child(tabs),
            );

        content = content.child(match self.section {
            Section::Appearance => self.appearance(colors, language, cx).into_any_element(),
            Section::Shell => self.shell(colors, language, cx).into_any_element(),
            Section::Language => self.languages(colors, language, cx).into_any_element(),
            Section::Vault => self.vault_section(colors, language, cx).into_any_element(),
        });

        div()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(colors.background))
            .text_color(rgb(colors.foreground))
            .p(px(28.))
            .flex()
            .flex_col()
            .items_center()
            .child(content)
    }
}

fn settings_card(colors: Colors) -> GroupBox {
    GroupBox::new()
        .fill()
        .border_1()
        .border_color(colors.hairline())
        .rounded_xl()
}

pub(crate) fn toggle_switch(id: impl Into<ElementId>, checked: bool, _colors: Colors) -> Switch {
    Switch::new(id).checked(checked)
}

fn is_executable(shell: &ShellSettings) -> bool {
    let path = shell.custom_path();
    if !path.is_absolute() {
        return false;
    }
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(crate) fn input_field(
    field: &Entity<InputState>,
    icon: Option<Icon>,
    colors: Colors,
) -> Input {
    let mut input = Input::new(field);
    if let Some(icon) = icon {
        input = input.prefix(icon.render(px(14.), colors.muted));
    }
    input
}

pub(crate) fn primary_button(
    id: impl Into<ElementId>,
    icon: Option<Icon>,
    label: impl Into<SharedString>,
    _colors: Colors,
) -> Button {
    let label = label.into();
    let mut btn = Button::new(id)
        .primary()
        .small()
        .h(px(32.))
        .px(px(12.))
        .label(label);
    if let Some(icon) = icon {
        btn = btn.icon(icon);
    }
    btn
}

pub(crate) fn secondary_button(
    id: impl Into<ElementId>,
    icon: Option<Icon>,
    label: impl Into<SharedString>,
    _colors: Colors,
) -> Button {
    let label = label.into();
    let mut btn = Button::new(id)
        .outline()
        .small()
        .h(px(32.))
        .px(px(12.))
        .label(label);
    if let Some(icon) = icon {
        btn = btn.icon(icon);
    }
    btn
}

pub(crate) fn danger_button(
    id: impl Into<ElementId>,
    icon: Option<Icon>,
    label: impl Into<SharedString>,
    solid: bool,
    _colors: Colors,
) -> Button {
    let label = label.into();
    let mut btn = Button::new(id)
        .small()
        .h(px(32.))
        .px(px(12.))
        .label(label);
    if solid {
        btn = btn.danger();
    } else {
        btn = btn.ghost();
    }
    if let Some(icon) = icon {
        btn = btn.icon(icon);
    }
    btn
}
