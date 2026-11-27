mod hosts;
mod keychain;
mod mcp;
mod snippets;
mod tunnels;

use std::sync::Arc;

use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, ElementId, Entity, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window,
    component::{
        IndexPath, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
        select::{Select, SelectEvent, SelectItem, SelectState},
        slider::{Slider, SliderEvent, SliderState},
        switch::Switch,
    },
    div, prelude::FluentBuilder as _, px, rgb, transparent_black,
};

use crate::icons::Icon;
use crate::mcp::McpRuntime;
use crate::settings::{
    BellMode, CursorStyle, DEFAULT_FONT_SIZE, HostKeyPolicy, Language, LineHeightPreset,
    MAX_FONT_SIZE, MIN_FONT_SIZE, MIN_OPACITY, ScrollbackLimit, Settings, SettingsStore,
    SftpConflictPolicy, ShellSettings, default_shortcuts,
};
use crate::theme::{Colors, Theme, ThemeMode};
use crate::vault::{export_vault, import_vault, merge_vault_into};
pub(crate) use hosts::{HostsEvent, HostsPage};
pub(crate) use keychain::KeychainPage;
pub(crate) use mcp::McpPage;
pub(crate) use snippets::{SnippetsEvent, SnippetsPage};
pub(crate) use tunnels::TunnelsPage;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SettingsTab {
    #[default]
    Appearance,
    Terminal,
    Connection,
    Mcp,
    Security,
    Shortcuts,
}

impl SettingsTab {
    const ALL: [Self; 6] = [
        Self::Appearance,
        Self::Terminal,
        Self::Connection,
        Self::Mcp,
        Self::Security,
        Self::Shortcuts,
    ];

    fn label(self, language: Language) -> &'static str {
        match self {
            Self::Appearance => language.text("Appearance", "外观"),
            Self::Terminal => language.text("Terminal & Shell", "终端与 Shell"),
            Self::Connection => language.text("Connection & SFTP", "连接与传输"),
            Self::Mcp => language.text("MCP Server", "MCP 服务"),
            Self::Security => language.text("Security & Vault", "安全与备份"),
            Self::Shortcuts => language.text("Shortcuts & About", "快捷键与关于"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Appearance => Icon::Palette,
            Self::Terminal => Icon::Terminal,
            Self::Connection => Icon::Network,
            Self::Mcp => Icon::Mcp,
            Self::Security => Icon::Lock,
            Self::Shortcuts => Icon::Keyboard,
        }
    }

    fn badge_color(self) -> u32 {
        match self {
            Self::Appearance => 0xaf52de,
            Self::Terminal => 0x2c2c2e,
            Self::Connection => 0x34c759,
            Self::Mcp => 0x5856d6,
            Self::Security => 0xff3b30,
            Self::Shortcuts => 0x636366,
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
        ("", language.text("System Default (System Mono)", "系统默认 (System Mono)")),
        ("SF Mono", "SF Mono"),
        ("Menlo", "Menlo"),
        ("JetBrains Mono", "JetBrains Mono"),
        ("Fira Code", "Fira Code"),
        ("Cascadia Code", "Cascadia Code"),
    ]
    .into_iter()
    .map(|(value, label)| SelectChoice::new(label, value.to_owned()))
    .collect()
}

fn scrollback_select_items(language: Language) -> Vec<SelectChoice<ScrollbackLimit>> {
    [
        (ScrollbackLimit::Lines5000, language.text("5,000 lines", "5,000 行")),
        (ScrollbackLimit::Lines10000, language.text("10,000 lines", "10,000 行")),
        (ScrollbackLimit::Lines50000, language.text("50,000 lines", "50,000 行")),
        (ScrollbackLimit::Unlimited, language.text("Unlimited", "无限制")),
    ]
    .into_iter()
    .map(|(v, l)| SelectChoice::new(l, v))
    .collect()
}

fn keep_alive_select_items(language: Language) -> Vec<SelectChoice<u16>> {
    [
        (0, language.text("Off", "关闭")),
        (15, language.text("Every 15s", "每 15 秒")),
        (30, language.text("Every 30s", "每 30 秒")),
        (60, language.text("Every 60s", "每 60 秒")),
    ]
    .into_iter()
    .map(|(v, l)| SelectChoice::new(l, v))
    .collect()
}

fn connect_timeout_select_items(language: Language) -> Vec<SelectChoice<u16>> {
    [
        (10, language.text("10 sec", "10 秒")),
        (15, language.text("15 sec", "15 秒")),
        (30, language.text("30 sec", "30 秒")),
        (60, language.text("60 sec", "60 秒")),
    ]
    .into_iter()
    .map(|(v, l)| SelectChoice::new(l, v))
    .collect()
}

fn auto_lock_select_items(language: Language) -> Vec<SelectChoice<u16>> {
    [
        (0, language.text("Never", "从不")),
        (5, language.text("5 min", "5 分钟")),
        (15, language.text("15 min", "15 分钟")),
        (30, language.text("30 min", "30 分钟")),
        (60, language.text("1 hour", "1 小时")),
    ]
    .into_iter()
    .map(|(v, l)| SelectChoice::new(l, v))
    .collect()
}

fn selected_index<V: Clone + PartialEq + 'static>(
    items: &[SelectChoice<V>],
    target: &V,
) -> Option<IndexPath> {
    items
        .iter()
        .position(|item| item.value == *target)
        .or(if items.is_empty() { None } else { Some(0) })
        .map(|row| IndexPath::default().row(row))
}

pub struct Preferences {
    focus_handle: FocusHandle,
    active_tab: SettingsTab,
    theme_mode: ThemeMode,
    last_dark: Theme,
    last_light: Theme,
    store: Entity<SettingsStore>,
    shell_path: Entity<InputState>,
    sftp_download_dir: Entity<InputState>,
    vault_passphrase: Entity<InputState>,
    show_passphrase: bool,
    opacity_slider: Entity<SliderState>,
    font_size_slider: Entity<SliderState>,
    font_select: Entity<SelectState<Vec<SelectChoice<String>>>>,
    scrollback_select: Entity<SelectState<Vec<SelectChoice<ScrollbackLimit>>>>,
    keep_alive_select: Entity<SelectState<Vec<SelectChoice<u16>>>>,
    connect_timeout_select: Entity<SelectState<Vec<SelectChoice<u16>>>>,
    auto_lock_select: Entity<SelectState<Vec<SelectChoice<u16>>>>,
    login_shell: bool,
    recording_shortcut_id: Option<String>,
    mcp_runtime: Arc<McpRuntime>,
    mcp_page: Entity<McpPage>,
    _subs: Vec<Subscription>,
}

impl Focusable for Preferences {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Preferences {
    pub fn new(
        store: Entity<SettingsStore>,
        mcp_runtime: Arc<McpRuntime>,
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
        let current_scrollback = current.scrollback;
        let current_keep_alive = current.keep_alive_secs;
        let current_connect_timeout = current.connect_timeout_secs;
        let current_auto_lock = current.auto_lock_mins;
        let current_download_dir = current.sftp_download_dir.clone();
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
        let sftp_download_dir = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("~/Downloads")
                .default_value(current_download_dir)
        });
        let vault_passphrase = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text(
                    "Enter master passphrase for export or import…",
                    "输入主密码（用于加密导出或解密导入）…",
                ))
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
        let font_ix = selected_index(&font_items, &current_font_family);
        let font_select = cx.new(|cx| SelectState::new(font_items, font_ix, window, cx));

        let sb_items = scrollback_select_items(language);
        let sb_ix = selected_index(&sb_items, &current_scrollback);
        let scrollback_select = cx.new(|cx| SelectState::new(sb_items, sb_ix, window, cx));

        let ka_items = keep_alive_select_items(language);
        let ka_ix = selected_index(&ka_items, &current_keep_alive);
        let keep_alive_select = cx.new(|cx| SelectState::new(ka_items, ka_ix, window, cx));

        let ct_items = connect_timeout_select_items(language);
        let ct_ix = selected_index(&ct_items, &current_connect_timeout);
        let connect_timeout_select = cx.new(|cx| SelectState::new(ct_items, ct_ix, window, cx));

        let al_items = auto_lock_select_items(language);
        let al_ix = selected_index(&al_items, &current_auto_lock);
        let auto_lock_select = cx.new(|cx| SelectState::new(al_items, al_ix, window, cx));

        let opacity_sub = cx.subscribe_in(
            &opacity_slider,
            window,
            |this, _, event: &SliderEvent, window, cx| {
                if let SliderEvent::Change(val) = event {
                    let next = val.end().round().clamp(f32::from(MIN_OPACITY), 100.) as u8;
                    if this.store.read(cx).current.opacity != next {
                        this.save(move |settings| settings.opacity = next, window, cx);
                    }
                }
            },
        );
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
        let scrollback_sub = cx.subscribe_in(
            &scrollback_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<ScrollbackLimit>>>, window, cx| {
                if let SelectEvent::Confirm(Some(limit)) = event
                    && this.store.read(cx).current.scrollback != *limit
                {
                    let next = *limit;
                    this.save(move |settings| settings.scrollback = next, window, cx);
                }
            },
        );
        let keep_alive_sub = cx.subscribe_in(
            &keep_alive_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<u16>>>, window, cx| {
                if let SelectEvent::Confirm(Some(secs)) = event
                    && this.store.read(cx).current.keep_alive_secs != *secs
                {
                    let next = *secs;
                    this.save(move |settings| settings.keep_alive_secs = next, window, cx);
                }
            },
        );
        let connect_timeout_sub = cx.subscribe_in(
            &connect_timeout_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<u16>>>, window, cx| {
                if let SelectEvent::Confirm(Some(secs)) = event
                    && this.store.read(cx).current.connect_timeout_secs != *secs
                {
                    let next = *secs;
                    this.save(move |settings| settings.connect_timeout_secs = next, window, cx);
                }
            },
        );
        let auto_lock_sub = cx.subscribe_in(
            &auto_lock_select,
            window,
            |this, _, event: &SelectEvent<Vec<SelectChoice<u16>>>, window, cx| {
                if let SelectEvent::Confirm(Some(mins)) = event
                    && this.store.read(cx).current.auto_lock_mins != *mins
                {
                    let next = *mins;
                    this.save(move |settings| settings.auto_lock_mins = next, window, cx);
                }
            },
        );
        let download_dir_sub = cx.subscribe_in(
            &sftp_download_dir,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let next = state.read(cx).value().trim().to_owned();
                    if !next.is_empty() && this.store.read(cx).current.sftp_download_dir != next {
                        this.save(move |settings| settings.sftp_download_dir = next, window, cx);
                    }
                }
            },
        );
        let passphrase_sub = cx.subscribe_in(
            &vault_passphrase,
            window,
            |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        );

        if let Some(msg) = store.read(cx).load_error() {
            crate::notify::error(msg.to_owned(), window, cx);
        }

        let mcp_page = cx.new(|cx| McpPage::new(store.clone(), Arc::clone(&mcp_runtime), window, cx));

        Self {
            focus_handle: cx.focus_handle(),
            active_tab: SettingsTab::Appearance,
            theme_mode,
            last_dark,
            last_light,
            store,
            shell_path,
            sftp_download_dir,
            vault_passphrase,
            show_passphrase: false,
            opacity_slider,
            font_size_slider,
            font_select,
            scrollback_select,
            keep_alive_select,
            connect_timeout_select,
            auto_lock_select,
            login_shell,
            recording_shortcut_id: None,
            mcp_runtime,
            mcp_page,
            _subs: vec![
                opacity_sub,
                font_size_sub,
                font_select_sub,
                scrollback_sub,
                keep_alive_sub,
                connect_timeout_sub,
                auto_lock_sub,
                download_dir_sub,
                passphrase_sub,
            ],
        }
    }

    pub fn notify_mcp(&mut self, cx: &mut Context<Self>) {
        self.mcp_page.update(cx, |_, cx| cx.notify());
        cx.notify();
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
            let current = self.store.read(cx).current.clone();
            let font_items = font_select_items(language);
            self.font_select.update(cx, |select, cx| {
                select.set_items(font_items, window, cx);
                select.set_selected_value(&current.font_family, window, cx);
            });
            let sb_items = scrollback_select_items(language);
            self.scrollback_select.update(cx, |select, cx| {
                select.set_items(sb_items, window, cx);
                select.set_selected_value(&current.scrollback, window, cx);
            });
            let ka_items = keep_alive_select_items(language);
            self.keep_alive_select.update(cx, |select, cx| {
                select.set_items(ka_items, window, cx);
                select.set_selected_value(&current.keep_alive_secs, window, cx);
            });
            let ct_items = connect_timeout_select_items(language);
            self.connect_timeout_select.update(cx, |select, cx| {
                select.set_items(ct_items, window, cx);
                select.set_selected_value(&current.connect_timeout_secs, window, cx);
            });
            let al_items = auto_lock_select_items(language);
            self.auto_lock_select.update(cx, |select, cx| {
                select.set_items(al_items, window, cx);
                select.set_selected_value(&current.auto_lock_mins, window, cx);
            });
            self.vault_passphrase.update(cx, |input, cx| {
                input.set_placeholder(
                    language.text(
                        "Enter master passphrase for export or import…",
                        "输入主密码（用于加密导出或解密导入）…",
                    ),
                    window,
                    cx,
                );
            });
        }
    }

    fn reset_font_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.font_size_slider.update(cx, |slider, cx| {
            slider.set_value(f32::from(DEFAULT_FONT_SIZE), window, cx);
        });
        self.save(|s| s.font_size = DEFAULT_FONT_SIZE, window, cx);
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
                    "Shell 设置已保存，将在下次新建终端标签时生效。",
                ),
                window,
                cx,
            );
        }
    }

    fn choose_download_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.language(cx);
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                language
                    .text("Choose Default Download Folder", "选择默认下载目录")
                    .into(),
            ),
        });
        let window_handle = window.window_handle();
        cx.spawn(async move |view, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let path_str = path.to_string_lossy().into_owned();
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        let value = path_str.clone();
                        this.sftp_download_dir.update(cx, |input, cx| {
                            input.set_value(value, window, cx);
                        });
                        this.save(
                            move |s| s.sftp_download_dir = path_str,
                            window,
                            cx,
                        );
                    });
                });
            }
        })
        .detach();
    }

    fn toggle_passphrase_visibility(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_passphrase = !self.show_passphrase;
        let masked = !self.show_passphrase;
        self.vault_passphrase.update(cx, |input, cx| {
            input.set_masked(masked, window, cx);
        });
        cx.notify();
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
        let receiver = cx.prompt_for_new_path(&default_dir, Some("portus.portus-vault"));
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
                    .text("Choose an encrypted Portus vault", "选择加密备份文件")
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

    fn reset_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.language(cx);
        self.recording_shortcut_id = None;
        if self.save(|s| s.shortcuts = default_shortcuts(), window, cx) {
            crate::notify::success(
                language.text("Default shortcuts restored.", "已恢复全部默认快捷键。"),
                window,
                cx,
            );
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target_id) = self.recording_shortcut_id.clone() else {
            return;
        };
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.recording_shortcut_id = None;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if matches!(
            key,
            "shift" | "control" | "alt" | "platform" | "meta" | "super"
        ) {
            return;
        }
        let formatted = format_keystroke(event);
        self.recording_shortcut_id = None;
        let language = self.language(cx);
        self.save(
            move |settings| {
                if let Some(binding) = settings.shortcuts.iter_mut().find(|b| b.id == target_id) {
                    binding.keys = formatted.clone();
                } else {
                    settings.shortcuts.push(crate::settings::ShortcutBinding {
                        id: target_id,
                        keys: formatted.clone(),
                    });
                }
            },
            window,
            cx,
        );
        crate::notify::info(
            language.text("Shortcut updated.", "快捷键已更新。"),
            window,
            cx,
        );
        cx.stop_propagation();
    }

    fn open_config_folder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.language(cx);
        let Some(dir) = self.store.read(cx).config_dir().map(|p| p.to_path_buf()) else {
            crate::notify::error(
                language.text("Config directory unavailable.", "无法定位配置目录。"),
                window,
                cx,
            );
            return;
        };
        if let Err(err) = std::fs::create_dir_all(&dir) {
            crate::notify::error(err.to_string(), window, cx);
            return;
        }
        #[cfg(target_os = "macos")]
        let res = std::process::Command::new("open").arg(&dir).spawn();
        #[cfg(target_os = "linux")]
        let res = std::process::Command::new("xdg-open").arg(&dir).spawn();
        #[cfg(target_os = "windows")]
        let res = std::process::Command::new("explorer").arg(&dir).spawn();
        match res {
            Ok(_) => crate::notify::info(
                language.text("Opened config directory.", "已在文件管理器中打开配置目录。"),
                window,
                cx,
            ),
            Err(err) => crate::notify::error(err.to_string(), window, cx),
        }
    }

    fn export_diagnostics(&self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.language(cx);
        let saved = &self.store.read(cx).current;
        let report = format!(
            "Portus Diagnostics v{}\nOS: {}\nArch: {}\nTheme: {:?}\nLanguage: {:?}\nHosts: {}\nKeychain entries: {}\nSnippets: {}\nTunnels: {}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            saved.theme,
            saved.language,
            saved.hosts.len(),
            saved.keychain.len(),
            saved.snippets.len(),
            saved.tunnels.len(),
        );
        let default_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&default_dir, Some("portus-diagnostics.txt"));
        let window_handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let res = std::fs::write(&path, report);
                let _ = window_handle.update(cx, |_, window, cx| match res {
                    Ok(()) => crate::notify::success(
                        language.text("Sanitized diagnostics exported.", "已导出脱敏诊断日志。"),
                        window,
                        cx,
                    ),
                    Err(err) => crate::notify::error(err.to_string(), window, cx),
                });
            }
        })
        .detach();
    }
}

impl Render for Preferences {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = self.store.read(cx).current.clone();
        let colors = saved.theme.colors();
        let language = saved.language;

        let header = div()
            .px(px(22.))
            .py(px(14.))
            .border_b_1()
            .border_color(colors.hairline())
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors.foreground))
                    .child(language.text("Settings", "设置")),
            );

        let nav = div()
            .w(px(192.))
            .flex_shrink_0()
            .h_full()
            .px(px(8.))
            .py(px(12.))
            .border_r_1()
            .border_color(colors.hairline())
            .bg(rgb(if colors.dark { 0xffffff } else { 0x000000 }).alpha(0.02))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(SettingsTab::ALL.into_iter().enumerate().map(|(ix, tab)| {
                let active = self.active_tab == tab;
                div()
                    .id(("settings-nav", ix))
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(if active {
                        colors.accent_soft()
                    } else {
                        transparent_black().into()
                    })
                    .hover(|s| {
                        if active {
                            s
                        } else {
                            s.bg(colors.surface_hover_glass())
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.active_tab = tab;
                        this.recording_shortcut_id = None;
                        cx.notify();
                    }))
                    .child(tab.icon().badge_with_color(px(22.), tab.badge_color()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(if active {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(if active {
                                colors.accent
                            } else {
                                colors.secondary
                            }))
                            .child(tab.label(language)),
                    )
                    .children((tab == SettingsTab::Mcp).then(|| {
                        let pending = self.mcp_runtime.pending_count();
                        (pending > 0).then(|| {
                            div()
                                .px(px(6.))
                                .py(px(1.))
                                .rounded_full()
                                .bg(colors.warning_soft())
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(colors.warning()))
                                .child(pending.to_string())
                        })
                    }).flatten())
            }));

        let right_pane = if self.active_tab == SettingsTab::Mcp {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_hidden()
                .child(self.mcp_page.clone())
                .into_any_element()
        } else {
            let pane_content = match self.active_tab {
                SettingsTab::Appearance => self.render_appearance(&saved, colors, language, cx),
                SettingsTab::Terminal => self.render_terminal(&saved, colors, language, cx),
                SettingsTab::Connection => self.render_connection(&saved, colors, language, cx),
                SettingsTab::Security => self.render_security(&saved, colors, language, cx),
                SettingsTab::Shortcuts => self.render_shortcuts(&saved, colors, language, cx),
                SettingsTab::Mcp => unreachable!(),
            };
            div()
                .id("settings-scroll")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .px(px(24.))
                .pt(px(18.))
                .pb(px(32.))
                .child(
                    div()
                        .w_full()
                        .max_w(px(680.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(pane_content),
                )
                .into_any_element()
        };

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .bg(colors.content_bg(saved.vibrancy, saved.opacity))
            .text_color(rgb(colors.foreground))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .overflow_hidden()
                    .child(nav)
                    .child(right_pane),
            )
    }
}

impl Preferences {
    fn render_appearance(
        &self,
        saved: &Settings,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let themes = Theme::for_mode(self.theme_mode);

        let mode_seg = segmented_control(colors, [
            seg_item(
                "pref-mode-dark",
                self.theme_mode == ThemeMode::Dark,
                language.text("Dark (10)", "深色 (10)"),
                cx.listener(|this, _, window, cx| {
                    this.select_theme_mode(ThemeMode::Dark, window, cx);
                }),
            ),
            seg_item(
                "pref-mode-light",
                self.theme_mode == ThemeMode::Light,
                language.text("Light (10)", "浅色 (10)"),
                cx.listener(|this, _, window, cx| {
                    this.select_theme_mode(ThemeMode::Light, window, cx);
                }),
            ),
        ]);

        let theme_grid = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(themes.chunks(5).enumerate().map(|(row_ix, chunk)| {
                div()
                    .flex()
                    .gap(px(8.))
                    .children(chunk.iter().enumerate().map(|(col_ix, &choice)| {
                        let idx = row_ix * 5 + col_ix;
                        let c = choice.colors();
                        let pal = choice.palette();
                        let selected = saved.theme == choice;
                        div()
                            .id(("pref-theme-card", idx))
                            .flex_1()
                            .min_w_0()
                            .rounded(px(9.))
                            .overflow_hidden()
                            .cursor_pointer()
                            .bg(rgb(c.panel))
                            .border_1()
                            .border_color(if selected {
                                rgb(colors.accent).into()
                            } else {
                                colors.hairline()
                            })
                            .hover(|s| {
                                if selected {
                                    s
                                } else {
                                    s.border_color(colors.hairline_strong())
                                }
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_theme(choice, window, cx);
                            }))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(54.))
                                    .px(px(8.))
                                    .py(px(7.))
                                    .bg(rgb(pal.background))
                                    .text_color(rgb(pal.foreground))
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
                                                    .gap(px(3.))
                                                    .child(dot(pal.ansi[1]))
                                                    .child(dot(pal.ansi[3]))
                                                    .child(dot(pal.ansi[2])),
                                            )
                                            .child(
                                                div()
                                                    .w(px(13.))
                                                    .h(px(3.5))
                                                    .rounded(px(99.))
                                                    .bg(rgb(c.accent)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .text_size(px(9.))
                                            .line_height(px(11.5))
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(px(3.))
                                                    .child(
                                                        div()
                                                            .text_color(rgb(pal.ansi[4]))
                                                            .child("$"),
                                                    )
                                                    .child("ssh prod"),
                                            )
                                            .child(
                                                div()
                                                    .text_color(rgb(pal.ansi[2]))
                                                    .child("ok"),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(7.))
                                    .py(px(5.))
                                    .bg(rgb(c.panel))
                                    .border_t_1()
                                    .border_color(rgb(0x808080).alpha(0.18))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(10.5))
                                            .font_weight(if selected {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::MEDIUM
                                            })
                                            .text_color(rgb(c.foreground))
                                            .child(choice.label(language)),
                                    )
                                    .when(selected, |this| {
                                        this.child(
                                            div()
                                                .size(px(13.))
                                                .rounded_full()
                                                .bg(rgb(c.accent))
                                                .text_color(rgb(c.accent_fg))
                                                .text_size(px(8.5))
                                                .font_weight(FontWeight::BOLD)
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child("✓"),
                                        )
                                    }),
                            )
                    }))
            }));

        let theme_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(row_text(
                        language.text("Appearance & Theme Mode", "外观模式与终端配色"),
                        language.text(
                            "Switch between Dark and Light modes; selecting a palette syncs UI and terminal immediately",
                            "切换深色或浅色模式，选择配色后立即同步至全局界面与终端",
                        ),
                        colors,
                    ))
                    .child(mode_seg),
            )
            .child(inset_col_row(true, colors).child(theme_grid));

        let material_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(row_text(
                        language.text("macOS Native Vibrancy", "macOS 原生毛玻璃通透感"),
                        language.text(
                            "Enable system NSVisualEffectView blur behind sidebar and titlebar",
                            "在侧边栏与标题栏后方启用系统级 NSVisualEffectView 模糊质感",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-vibrancy", saved.vibrancy, colors).on_click(
                            cx.listener(|this, checked: &bool, window, cx| {
                                let enabled = *checked;
                                this.save(|s| s.vibrancy = enabled, window, cx);
                            }),
                        ),
                    ),
            )
            .child(
                inset_col_row(true, colors)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(row_text(
                                language.text("Window Opacity", "窗口不透明度"),
                                language.text(
                                    "Adjust main window and terminal background translucency (50%–100%)",
                                    "调整 Portus 主窗口与终端背景的透明程度（50%–100%）",
                                ),
                                colors,
                            ))
                            .child(value_pill(format!("{}%", saved.opacity), colors)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .w(px(34.))
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child("50%"),
                            )
                            .child(div().flex_1().child(Slider::new(&self.opacity_slider)))
                            .child(
                                div()
                                    .w(px(34.))
                                    .text_right()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child("100%"),
                            ),
                    ),
            );

        let lang_card = inset_card(colors).child(
            inset_col_row(false, colors)
                .child(row_text(
                    language.text("Display Language", "显示语言"),
                    language.text(
                        "Applies immediately across menus, tabs, and all management pages",
                        "立即应用到菜单、标签栏与所有管理页面",
                    ),
                    colors,
                ))
                .child(language_option_row(
                    "pref-lang-zh",
                    "简体中文",
                    "Simplified Chinese",
                    language == Language::ChineseSimplified,
                    colors,
                    cx.listener(|this, _, window, cx| {
                        this.set_language(Language::ChineseSimplified, window, cx);
                    }),
                ))
                .child(language_option_row(
                    "pref-lang-en",
                    "English",
                    "English (United States)",
                    language == Language::English,
                    colors,
                    cx.listener(|this, _, window, cx| {
                        this.set_language(Language::English, window, cx);
                    }),
                )),
        );

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(group_title(
                language.text("Theme & Palette", "主题与配色"),
                colors,
            ))
            .child(theme_card)
            .child(group_title(
                language.text("Window Material & Opacity", "窗口材质与透明度"),
                colors,
            ))
            .child(material_card)
            .child(group_title(
                language.text("Interface Language", "界面语言"),
                colors,
            ))
            .child(lang_card)
    }

    fn render_terminal(
        &self,
        saved: &Settings,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let pal = saved.theme.palette();
        let preview_size = px(f32::from(saved.font_size));
        let preview_lh = px(f32::from(saved.font_size) * saved.line_height.factor());

        let cursor_el = match saved.cursor_style {
            CursorStyle::Block => div()
                .w(px(7.))
                .h(px(f32::from(saved.font_size) * 1.1))
                .bg(rgb(colors.accent).alpha(0.85)),
            CursorStyle::Bar => div()
                .w(px(2.))
                .h(px(f32::from(saved.font_size) * 1.1))
                .bg(rgb(colors.accent)),
            CursorStyle::Underline => div()
                .w(px(8.))
                .h(px(2.))
                .mt(px(f32::from(saved.font_size) * 0.9))
                .bg(rgb(colors.accent)),
        };

        let mini_preview = div()
            .px(px(12.))
            .py(px(10.))
            .rounded(px(8.))
            .bg(rgb(pal.background))
            .border_1()
            .border_color(colors.hairline())
            .text_color(rgb(pal.foreground))
            .text_size(preview_size)
            .line_height(preview_lh)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(div().text_color(rgb(pal.ansi[2])).child("deploy@prod-api-01"))
                    .child(":")
                    .child(div().text_color(rgb(pal.ansi[4])).child("/var/www/api"))
                    .child("$ uptime"),
            )
            .child(
                div()
                    .text_color(rgb(pal.ansi[8]))
                    .child("10:15  up 14 days, 6:42, 1 user, load averages: 0.42 0.38 0.31"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(div().text_color(rgb(pal.ansi[2])).child("deploy@prod-api-01"))
                    .child(":")
                    .child(div().text_color(rgb(pal.ansi[4])).child("/var/www/api"))
                    .child("$ ")
                    .child(cursor_el),
            );

        let line_height_seg = segmented_control(colors, [
            seg_item(
                "pref-lh-compact",
                saved.line_height == LineHeightPreset::Compact,
                language.text("Compact", "紧凑"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.line_height = LineHeightPreset::Compact, window, cx);
                }),
            ),
            seg_item(
                "pref-lh-standard",
                saved.line_height == LineHeightPreset::Standard,
                language.text("Standard", "标准"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.line_height = LineHeightPreset::Standard, window, cx);
                }),
            ),
            seg_item(
                "pref-lh-relaxed",
                saved.line_height == LineHeightPreset::Relaxed,
                language.text("Relaxed", "宽松"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.line_height = LineHeightPreset::Relaxed, window, cx);
                }),
            ),
        ]);

        let typography_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(row_text(
                        language.text("Terminal Font", "终端字体"),
                        language.text(
                            "Applies immediately to all open terminal tabs and split panes",
                            "立即应用到所有已打开的终端标签页与分屏",
                        ),
                        colors,
                    ))
                    .child(div().w(px(210.)).child(Select::new(&self.font_select).text_xs())),
            )
            .child(
                inset_col_row(true, colors)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(row_text(
                                language.text("Font Size", "终端字号"),
                                language.text(
                                    "Adjust from 10px to 22px, or zoom temporarily with ⌘+/⌘-",
                                    "支持 10px–22px 自由调节，也可以在终端内用 ⌘+/⌘- 临时缩放",
                                ),
                                colors,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(value_pill(format!("{} px", saved.font_size), colors))
                                    .child(
                                        secondary_button(
                                            "pref-reset-font-size",
                                            None,
                                            language.text("Default", "默认"),
                                            colors,
                                        )
                                        .h(px(23.))
                                        .px(px(8.))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.reset_font_size(window, cx);
                                        })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .w(px(34.))
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child("10 px"),
                            )
                            .child(div().flex_1().child(Slider::new(&self.font_size_slider)))
                            .child(
                                div()
                                    .w(px(34.))
                                    .text_right()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child("22 px"),
                            ),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Line Height", "行高"),
                        language.text("Control vertical spacing between terminal lines", "控制终端字符行间距"),
                        colors,
                    ))
                    .child(line_height_seg),
            )
            .child(inset_col_row(true, colors).child(mini_preview));

        let cursor_seg = segmented_control(colors, [
            seg_item(
                "pref-cur-block",
                saved.cursor_style == CursorStyle::Block,
                language.text("Block", "方块"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.cursor_style = CursorStyle::Block, window, cx);
                }),
            ),
            seg_item(
                "pref-cur-bar",
                saved.cursor_style == CursorStyle::Bar,
                language.text("Bar", "竖线"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.cursor_style = CursorStyle::Bar, window, cx);
                }),
            ),
            seg_item(
                "pref-cur-underline",
                saved.cursor_style == CursorStyle::Underline,
                language.text("Underline", "下划线"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.cursor_style = CursorStyle::Underline, window, cx);
                }),
            ),
        ]);

        let bell_seg = segmented_control(colors, [
            seg_item(
                "pref-bell-off",
                saved.bell == BellMode::Off,
                language.text("Off", "静音"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.bell = BellMode::Off, window, cx);
                }),
            ),
            seg_item(
                "pref-bell-visual",
                saved.bell == BellMode::Visual,
                language.text("Visual", "视觉闪烁"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.bell = BellMode::Visual, window, cx);
                }),
            ),
            seg_item(
                "pref-bell-sound",
                saved.bell == BellMode::Sound,
                language.text("Sound", "系统提示音"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.bell = BellMode::Sound, window, cx);
                }),
            ),
        ]);

        let behavior_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(row_text(
                        language.text("Cursor Style", "光标样式"),
                        language.text("Preview cursor shape in the box above", "在上方预览区实时查看光标形态"),
                        colors,
                    ))
                    .child(cursor_seg),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Cursor Blink", "光标闪烁"),
                        language.text("Enable blinking cursor when terminal is focused", "终端聚焦时启用光标呼吸闪烁"),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-cursor-blink", saved.cursor_blink, colors).on_click(
                            cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.cursor_blink = v, window, cx);
                            }),
                        ),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Copy on Select", "选中文本自动复制"),
                        language.text(
                            "Automatically copy selected terminal text to clipboard",
                            "鼠标拖选终端内容后立即写入系统剪贴板",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-copy-select", saved.copy_on_select, colors).on_click(
                            cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.copy_on_select = v, window, cx);
                            }),
                        ),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Option as Meta (Esc+)", "Option 键作为 Meta (Esc+)"),
                        language.text(
                            "Treat ⌥ as Meta modifier in Emacs, Vim, and zsh",
                            "在 Emacs / Vim / zsh 中将 ⌥ 识别为 Meta 修饰键",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-option-meta", saved.option_as_meta, colors).on_click(
                            cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.option_as_meta = v, window, cx);
                            }),
                        ),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Terminal Bell", "终端响铃 (Bell)"),
                        language.text("Action when receiving BEL character", "收到 BEL 字符时的提示方式"),
                        colors,
                    ))
                    .child(bell_seg),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Scrollback Buffer", "历史回滚缓冲区 (Scrollback)"),
                        language.text(
                            "Maximum lines retained per terminal pane",
                            "单个终端窗格可向上回溯的最大行数",
                        ),
                        colors,
                    ))
                    .child(div().w(px(140.)).child(Select::new(&self.scrollback_select).text_xs())),
            );

        let shell_card = inset_card(colors)
            .child(
                inset_col_row(false, colors)
                    .child(row_text(
                        language.text("Shell Executable", "Shell 可执行文件"),
                        language.text(
                            "Leave blank for system default shell; applies to new terminal tabs",
                            "留空时使用系统默认 Shell，修改对新建的终端标签生效",
                        ),
                        colors,
                    ))
                    .child(input_field(&self.shell_path, Some(Icon::Terminal), colors)),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Login Shell (-l)", "登录 Shell (-l)"),
                        language.text(
                            "Launch local shell as login shell to load ~/.zprofile and PATH",
                            "以登录模式启动本地 Shell 以加载 ~/.zprofile 与 PATH 环境变量",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-login-shell", self.login_shell, colors).on_change(
                            cx.listener(|this, checked, window, cx| {
                                this.login_shell = *checked;
                                this.save_shell(window, cx);
                            }),
                        ),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .justify_end()
                    .child(
                        primary_button(
                            "pref-save-shell",
                            Some(Icon::Check),
                            language.text("Save Shell Settings", "保存 Shell 设置"),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.save_shell(window, cx);
                        })),
                    ),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(group_title(
                language.text("Typography", "字体与排版"),
                colors,
            ))
            .child(typography_card)
            .child(group_title(
                language.text("Cursor & Behavior", "光标与交互行为"),
                colors,
            ))
            .child(behavior_card)
            .child(group_title(
                language.text("Local Shell", "本地 Shell"),
                colors,
            ))
            .child(shell_card)
    }

    fn render_connection(
        &self,
        saved: &Settings,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let host_key_seg = segmented_control(colors, [
            seg_item(
                "pref-hk-ask",
                saved.host_key_policy == HostKeyPolicy::Ask,
                language.text("Ask", "首次询问"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.host_key_policy = HostKeyPolicy::Ask, window, cx);
                }),
            ),
            seg_item(
                "pref-hk-strict",
                saved.host_key_policy == HostKeyPolicy::Strict,
                language.text("Strict", "严格校验"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.host_key_policy = HostKeyPolicy::Strict, window, cx);
                }),
            ),
            seg_item(
                "pref-hk-accept",
                saved.host_key_policy == HostKeyPolicy::Accept,
                language.text("Accept", "自动接受"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.host_key_policy = HostKeyPolicy::Accept, window, cx);
                }),
            ),
        ]);

        let ssh_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(row_text(
                        language.text("KeepAlive Interval", "KeepAlive 心跳间隔"),
                        language.text(
                            "Send periodic keepalive packets to prevent idle NAT/firewall drops",
                            "定期发送空包以防止防火墙或 NAT 断开空闲 SSH 会话",
                        ),
                        colors,
                    ))
                    .child(div().w(px(130.)).child(Select::new(&self.keep_alive_select).text_xs())),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Connection Timeout", "连接超时时间"),
                        language.text(
                            "Maximum wait for TCP handshake and SSH negotiation",
                            "TCP 握手与 SSH 协商阶段的最长等待时间",
                        ),
                        colors,
                    ))
                    .child(div().w(px(130.)).child(Select::new(&self.connect_timeout_select).text_xs())),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Auto Reconnect", "断线自动重连"),
                        language.text(
                            "Automatically restore SSH sessions after network drops or wake from sleep",
                            "网络抖动或休眠唤醒后自动尝试恢复会话，保留屏幕输出",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-auto-reconnect", saved.auto_reconnect, colors)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.auto_reconnect = v, window, cx);
                            })),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Host Key Policy", "Host Key 指纹校验策略"),
                        language.text(
                            "How to handle unknown or changed server fingerprints",
                            "首次连接未知服务器或指纹变化时的安全处理",
                        ),
                        colors,
                    ))
                    .child(host_key_seg),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("SSH Agent Forwarding", "默认启用 SSH Agent 转发"),
                        language.text(
                            "Allow remote hosts to reuse local ssh-agent keys (trusted hosts only)",
                            "允许远程主机复用本机 ssh-agent 密钥（仅建议对可信主机开启）",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-agent-forward", saved.agent_forward, colors).on_click(
                            cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.agent_forward = v, window, cx);
                            }),
                        ),
                    ),
            );

        let conflict_seg = segmented_control(colors, [
            seg_item(
                "pref-sftp-ask",
                saved.sftp_conflict == SftpConflictPolicy::Ask,
                language.text("Ask", "每次询问"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.sftp_conflict = SftpConflictPolicy::Ask, window, cx);
                }),
            ),
            seg_item(
                "pref-sftp-overwrite",
                saved.sftp_conflict == SftpConflictPolicy::Overwrite,
                language.text("Overwrite", "覆盖"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.sftp_conflict = SftpConflictPolicy::Overwrite, window, cx);
                }),
            ),
            seg_item(
                "pref-sftp-skip",
                saved.sftp_conflict == SftpConflictPolicy::Skip,
                language.text("Skip", "跳过"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.sftp_conflict = SftpConflictPolicy::Skip, window, cx);
                }),
            ),
        ]);

        let concurrency_seg = segmented_control(colors, [
            seg_item(
                "pref-conc-2",
                saved.sftp_concurrency == 2,
                language.text("2x", "2 并发"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.sftp_concurrency = 2, window, cx);
                }),
            ),
            seg_item(
                "pref-conc-4",
                saved.sftp_concurrency == 4,
                language.text("4x", "4 并发"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.sftp_concurrency = 4, window, cx);
                }),
            ),
            seg_item(
                "pref-conc-8",
                saved.sftp_concurrency == 8,
                language.text("8x", "8 并发"),
                cx.listener(|this, _, window, cx| {
                    this.save(|s| s.sftp_concurrency = 8, window, cx);
                }),
            ),
        ]);

        let sftp_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(row_text(
                        language.text("Default Download Directory", "默认本地下载目录"),
                        language.text(
                            "Target folder when downloading from SFTP drawer or dual-pane view",
                            "在 SFTP 抽屉或双栏面板直接下载文件时的目标目录",
                        ),
                        colors,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .w(px(170.))
                                    .child(input_field(&self.sftp_download_dir, None, colors)),
                            )
                            .child(
                                secondary_button(
                                    "pref-sftp-browse",
                                    None,
                                    language.text("Browse…", "选择…"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.choose_download_dir(window, cx);
                                })),
                            ),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("File Conflict Policy", "同名文件冲突处理"),
                        language.text(
                            "Default action when destination file already exists",
                            "上传或下载遇到同名目标文件时的默认策略",
                        ),
                        colors,
                    ))
                    .child(conflict_seg),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Max Concurrent Transfers", "最大并发传输任务数"),
                        language.text(
                            "Maximum parallel channels for batch uploads and downloads",
                            "批量上传或下载目录时的并发通道上限",
                        ),
                        colors,
                    ))
                    .child(concurrency_seg),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Show Hidden Files (dotfiles)", "显示隐藏文件 (dotfiles)"),
                        language.text(
                            "Display dotfiles such as .env in SFTP file listings",
                            "在 SFTP 文件列表中展示以 . 开头的配置文件（如 .env）",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-sftp-hidden", saved.sftp_show_hidden, colors)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.sftp_show_hidden = v, window, cx);
                            })),
                    ),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(group_title(
                language.text("SSH Connection & KeepAlive", "SSH 连接与保活"),
                colors,
            ))
            .child(ssh_card)
            .child(group_title(
                language.text("SFTP File Transfer", "SFTP 文件传输"),
                colors,
            ))
            .child(sftp_card)
    }

    fn render_security(
        &self,
        saved: &Settings,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let pwd = self.vault_passphrase.read(cx).value();
        let (score, strength_label, strength_color) = eval_passphrase_strength(&pwd, language, colors);

        let lock_card = inset_card(colors)
            .child(
                inset_row(false, colors)
                    .child(row_text(
                        language.text("Unlock Portus with Touch ID", "使用 Touch ID 解锁 Portus"),
                        language.text(
                            "Verify with macOS Touch ID when waking or viewing plaintext credentials",
                            "唤醒应用或查看密钥库明文凭证时通过 macOS Touch ID 验证",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-touch-id", saved.touch_id_unlock, colors).on_click(
                            cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.touch_id_unlock = v, window, cx);
                            }),
                        ),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Auto-Lock After Idle", "空闲自动锁定"),
                        language.text(
                            "Automatically lock keychain and sensitive config after inactivity",
                            "离开电脑超过设定时间后自动锁定密钥库与敏感配置",
                        ),
                        colors,
                    ))
                    .child(div().w(px(130.)).child(Select::new(&self.auto_lock_select).text_xs())),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Hide Secrets on Blur", "切换后台时隐藏敏感凭证"),
                        language.text(
                            "Mask keys and password fields in Mission Control or when unfocused",
                            "在 Mission Control 预览或窗口失焦时自动遮蔽密钥与密码字段",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-hide-blur", saved.hide_secrets_on_blur, colors)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.hide_secrets_on_blur = v, window, cx);
                            })),
                    ),
            );

        let vault_card = inset_card(colors).child(
            inset_col_row(false, colors)
                .child(row_text(
                    language.text(
                        "Argon2id + AES-256-GCM Offline Vault",
                        "Argon2id + AES-256-GCM 离线加密保险库",
                    ),
                    language.text(
                        "Encrypt and export or restore hosts, keychain, tunnels, and snippets (.portus-vault). Key derivation stays local.",
                        "加密导出或导入全部主机、密钥库、端口转发与命令片段（.portus-vault），主密码仅在本地派生密钥，绝不上传",
                    ),
                    colors,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(input_field(&self.vault_passphrase, Some(Icon::Lock), colors)),
                        )
                        .child(
                            secondary_button(
                                "pref-toggle-pwd",
                                None,
                                if self.show_passphrase {
                                    language.text("Hide", "隐藏")
                                } else {
                                    language.text("Show", "显示")
                                },
                                colors,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_passphrase_visibility(window, cx);
                            })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .gap(px(4.))
                                .children((1..=4).map(|i| {
                                    div()
                                        .flex_1()
                                        .h(px(4.))
                                        .rounded(px(99.))
                                        .bg(if i <= score {
                                            rgb(strength_color).into()
                                        } else {
                                            colors.surface_active_glass()
                                        })
                                })),
                        )
                        .child(
                            div()
                                .min_w(px(64.))
                                .text_right()
                                .text_size(px(10.5))
                                .text_color(rgb(if score > 0 {
                                    strength_color
                                } else {
                                    colors.muted
                                }))
                                .child(format!(
                                    "{}{}",
                                    language.text("Strength: ", "强度："),
                                    strength_label
                                )),
                        ),
                )
                .child(
                    div()
                        .pt(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            primary_button(
                                "pref-export-vault",
                                Some(Icon::Import),
                                language.text("Export Vault…", "导出加密备份…"),
                                colors,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.export_vault_file(window, cx);
                            })),
                        )
                        .child(
                            secondary_button(
                                "pref-import-vault",
                                Some(Icon::Folder),
                                language.text("Import Vault…", "导入加密备份…"),
                                colors,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.import_vault_file(window, cx);
                            })),
                        ),
                ),
        );

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(group_title(
                language.text("App Lock & Biometrics", "应用锁定与生物认证"),
                colors,
            ))
            .child(lock_card)
            .child(group_title(
                language.text("Encrypted Vault Backup", "端到端加密配置备份 (Vault)"),
                colors,
            ))
            .child(vault_card)
    }

    fn render_shortcuts(
        &self,
        saved: &Settings,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let bindings = if saved.shortcuts.is_empty() {
            default_shortcuts()
        } else {
            saved.shortcuts.clone()
        };

        let shortcuts_card = inset_card(colors).children(
            bindings.into_iter().enumerate().map(|(ix, sc)| {
                let rec = self.recording_shortcut_id.as_deref() == Some(&sc.id);
                let (label, desc) = shortcut_meta(&sc.id, language);
                let sc_id = sc.id.clone();
                inset_row(ix > 0, colors)
                    .child(row_text(label, desc, colors))
                    .child(
                        div()
                            .id(("pref-sc-btn", ix))
                            .min_w(px(82.))
                            .h(px(26.))
                            .px(px(9.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .border_1()
                            .border_color(if rec {
                                rgb(colors.accent).into()
                            } else {
                                colors.hairline_strong()
                            })
                            .bg(if rec {
                                colors.accent_soft()
                            } else {
                                colors.content_bg(false, 100)
                            })
                            .hover(|s| s.border_color(colors.accent_border()))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if rec {
                                colors.accent
                            } else {
                                colors.foreground
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.recording_shortcut_id.as_deref() == Some(&sc_id) {
                                    this.recording_shortcut_id = None;
                                } else {
                                    this.recording_shortcut_id = Some(sc_id.clone());
                                    this.focus_handle.focus(window, cx);
                                }
                                cx.notify();
                            }))
                            .child(if rec {
                                language.text("Press keys…", "按下组合键…").to_owned()
                            } else {
                                sc.keys
                            }),
                    )
            }),
        );

        let config_path_display = self
            .store
            .read(cx)
            .settings_path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Portus/settings.json".to_owned());

        let version = concat!("v", env!("CARGO_PKG_VERSION"));

        let about_card = inset_card(colors)
            .child(
                div()
                    .p(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(crate::icons::app_logo(px(46.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(14.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child("Portus"),
                                    )
                                    .child(value_pill(version.to_owned(), colors)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .child("macOS Native SSH / SFTP / Terminal Client · Apple Silicon (GPUI)"),
                            ),
                    )
                    .child(
                        secondary_button(
                            "pref-check-updates",
                            None,
                            language.text("Check for Updates", "检查更新"),
                            colors,
                        )
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let msg = match language {
                                Language::English => format!("Up to date: Portus {version}"),
                                Language::ChineseSimplified => {
                                    format!("检查完成：当前已是最新版本 {version}")
                                }
                            };
                            crate::notify::info(msg, window, cx);
                        })),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(row_text(
                        language.text("Auto Check for Updates", "自动检查更新"),
                        language.text(
                            "Silently check for new releases in the background",
                            "在后台静默检查新版本发布通知",
                        ),
                        colors,
                    ))
                    .child(
                        toggle_switch("pref-auto-updates", saved.auto_check_updates, colors)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                let v = *checked;
                                this.save(|s| s.auto_check_updates = v, window, cx);
                            })),
                    ),
            )
            .child(
                inset_row(true, colors)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(language.text(
                                        "Local Config & Diagnostics",
                                        "本地配置与诊断",
                                    )),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.secondary))
                                    .child(config_path_display),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                secondary_button(
                                    "pref-open-config",
                                    None,
                                    language.text("Open Folder", "打开目录"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_config_folder(window, cx);
                                })),
                            )
                            .child(
                                secondary_button(
                                    "pref-export-diag",
                                    None,
                                    language.text("Export Diagnostics", "导出诊断日志"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_diagnostics(window, cx);
                                })),
                            ),
                    ),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(-8.))
                    .child(group_title(
                        language.text(
                            "Keyboard Shortcuts (click pill to record)",
                            "键盘快捷键（点击徽章可录制新按键）",
                        ),
                        colors,
                    ))
                    .child(
                        secondary_button(
                            "pref-reset-shortcuts",
                            None,
                            language.text("Reset Defaults", "恢复默认"),
                            colors,
                        )
                        .h(px(23.))
                        .px(px(8.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reset_shortcuts(window, cx);
                        })),
                    ),
            )
            .child(shortcuts_card)
            .child(group_title(
                language.text("About & Updates", "关于与更新"),
                colors,
            ))
            .child(about_card)
    }
}

fn shortcut_meta(id: &str, language: Language) -> (&'static str, &'static str) {
    match id {
        "cmdk" => (
            language.text("Quick Connect Palette", "快速连接命令面板"),
            language.text("Fuzzy search hosts, snippets, and actions", "全局模糊搜索主机、片段与操作"),
        ),
        "new_tab" => (
            language.text("New Local Terminal", "新建本地终端"),
            language.text("Open a new local shell tab in the titlebar", "在标题栏创建新的本地 Shell 标签"),
        ),
        "split_v" => (
            language.text("Split Vertically", "左右分屏"),
            language.text("Split the current terminal pane side by side", "在当前会话中横向拆分终端窗格"),
        ),
        "split_h" => (
            language.text("Split Horizontally", "上下分屏"),
            language.text("Split the current terminal pane top and bottom", "在当前会话中纵向拆分终端窗格"),
        ),
        "close_tab" => (
            language.text("Close Current Tab", "关闭当前标签"),
            language.text("Close the focused session or split pane", "关闭当前聚焦的会话或分屏"),
        ),
        "drawer_sftp" => (
            language.text("Toggle SFTP Drawer", "切换 SFTP 文件抽屉"),
            language.text("Expand or collapse remote files on the right", "在终端右侧快速展开/收起远程文件"),
        ),
        "drawer_snippets" => (
            language.text("Toggle Snippets Drawer", "切换命令片段抽屉"),
            language.text("Search and inject saved shell snippets", "快速检索并注入常用命令片段"),
        ),
        "settings" => (
            language.text("Open Settings", "打开设置页"),
            language.text("Jump directly to Portus Settings from anywhere", "从任意界面直接跳转到 Portus 设置"),
        ),
        _ => ("Shortcut", ""),
    }
}

fn format_keystroke(event: &KeyDownEvent) -> String {
    let mut out = String::new();
    let m = event.keystroke.modifiers;
    if m.control {
        out.push('⌃');
    }
    if m.alt {
        out.push('⌥');
    }
    if m.shift {
        out.push('⇧');
    }
    if m.platform {
        out.push('⌘');
    }
    let key = event.keystroke.key.as_str();
    let display = match key {
        "space" => "Space".to_owned(),
        "escape" => "Esc".to_owned(),
        "enter" => "↩".to_owned(),
        "backspace" => "⌫".to_owned(),
        "tab" => "⇥".to_owned(),
        k if k.len() == 1 => k.to_uppercase(),
        k => k.to_owned(),
    };
    out.push_str(&display);
    out
}

fn eval_passphrase_strength(
    pwd: &str,
    language: Language,
    colors: Colors,
) -> (u8, &'static str, u32) {
    if pwd.is_empty() {
        return (0, language.text("Empty", "未输入"), colors.muted);
    }
    let mut s = 0u8;
    if pwd.chars().count() >= 8 {
        s += 1;
    }
    if pwd.chars().count() >= 12 {
        s += 1;
    }
    if pwd.chars().any(|c| c.is_ascii_uppercase()) && pwd.chars().any(|c| c.is_ascii_lowercase()) {
        s += 1;
    }
    if pwd.chars().any(|c| c.is_ascii_digit()) && pwd.chars().any(|c| !c.is_ascii_alphanumeric()) {
        s += 1;
    }
    match s {
        0 | 1 => (1, language.text("Weak", "较弱"), colors.danger),
        2 => (2, language.text("Fair", "中等"), colors.warning()),
        3 => (3, language.text("Good", "良好"), colors.accent),
        _ => (4, language.text("Strong", "极强"), colors.success),
    }
}

fn group_title(text: &'static str, colors: Colors) -> gpui_kit::Div {
    div()
        .pl(px(2.))
        .mb(px(-8.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(colors.muted))
        .child(text.to_uppercase())
}

fn inset_card(colors: Colors) -> gpui_kit::Div {
    div()
        .w_full()
        .rounded(px(10.))
        .bg(colors.surface_glass())
        .border_1()
        .border_color(colors.hairline())
        .overflow_hidden()
        .flex()
        .flex_col()
}

fn inset_row(with_top_border: bool, colors: Colors) -> gpui_kit::Div {
    let mut row = div()
        .px(px(14.))
        .py(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.));
    if with_top_border {
        row = row.border_t_1().border_color(colors.hairline());
    }
    row
}

fn inset_col_row(with_top_border: bool, colors: Colors) -> gpui_kit::Div {
    let mut row = div()
        .px(px(14.))
        .py(px(12.))
        .flex()
        .flex_col()
        .gap(px(10.));
    if with_top_border {
        row = row.border_t_1().border_color(colors.hairline());
    }
    row
}

fn row_text(title: &'static str, desc: &'static str, colors: Colors) -> gpui_kit::Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(colors.foreground))
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(colors.secondary))
                .child(desc),
        )
}

fn value_pill(text: impl Into<SharedString>, colors: Colors) -> gpui_kit::Div {
    div()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(999.))
        .bg(colors.accent_soft())
        .text_color(rgb(colors.accent))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

fn dot(color: u32) -> gpui_kit::Div {
    div().size(px(5.5)).rounded_full().bg(rgb(color))
}

type SegHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

fn seg_item(
    id: &'static str,
    active: bool,
    label: &'static str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> (&'static str, bool, &'static str, SegHandler) {
    (id, active, label, Box::new(handler))
}

fn segmented_control<I>(colors: Colors, items: I) -> gpui_kit::Div
where
    I: IntoIterator<Item = (&'static str, bool, &'static str, SegHandler)>,
{
    div()
        .flex()
        .flex_shrink_0()
        .p(px(2.))
        .rounded(px(7.))
        .bg(colors.surface_glass())
        .border_1()
        .border_color(colors.hairline())
        .gap(px(2.))
        .children(
            items
                .into_iter()
                .map(|(id, active, label, handler)| {
                    div()
                        .id(id)
                        .px(px(10.))
                        .h(px(24.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if active {
                            colors.surface_active_glass()
                        } else {
                            transparent_black().into()
                        })
                        .text_size(px(12.))
                        .font_weight(if active {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(rgb(if active {
                            colors.foreground
                        } else {
                            colors.secondary
                        }))
                        .on_click(handler)
                        .child(label)
                }),
        )
}

fn language_option_row(
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    selected: bool,
    colors: Colors,
    handler: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .px(px(12.))
        .py(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .border_1()
        .border_color(if selected {
            colors.accent_border()
        } else {
            colors.hairline()
        })
        .bg(if selected {
            colors.accent_soft()
        } else {
            colors.content_bg(false, 100)
        })
        .hover(|s| {
            if selected {
                s
            } else {
                s.bg(colors.surface_hover_glass())
            }
        })
        .on_click(handler)
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
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors.foreground))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(colors.secondary))
                        .child(subtitle),
                ),
        )
        .child(
            div()
                .size(px(17.))
                .rounded_full()
                .border_1()
                .border_color(if selected {
                    rgb(colors.accent).into()
                } else {
                    colors.hairline_strong()
                })
                .bg(if selected {
                    rgb(colors.accent)
                } else {
                    rgb(0x000000).alpha(0.0)
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .text_color(rgb(colors.accent_fg))
                .when(selected, |this| this.child("✓")),
        )
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
    let mut input = Input::new(field)
        .h(px(34.))
        .py(px(2.))
        .text_xs()
        .line_height(px(24.));
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
        .xsmall()
        .h(px(28.))
        .px(px(10.))
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
        .xsmall()
        .h(px(28.))
        .px(px(10.))
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
        .xsmall()
        .h(px(28.))
        .px(px(10.))
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
