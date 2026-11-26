mod file_drawer;
mod file_icons;
mod icons;
mod mcp;
mod notify;
mod platform;
mod preferences;
mod protocols;
mod quick_connect;
mod settings;
mod sftp;
mod sftp_view;
mod ssh;
mod terminal;
mod theme;
mod tunnel;
mod vault;
mod workspace;

use gpui_kit::{
    Action, App, AppContext as _, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions,
    WindowBounds, WindowOptions, actions, application, px, size,
};

use settings::{Language, SettingsStore};
use workspace::Workspace;

actions!(
    portus,
    [
        NewTab,
        CloseTab,
        NextTab,
        PreviousTab,
        ShowHosts,
        ShowSnippets,
        ShowTunnels,
        ShowSettings,
        ShowVault,
        SplitPane,
        ToggleQuickConnect,
        ToggleSnippetsDrawer,
        ToggleFilesDrawer,
        FindInTerminal,
        CloseWindow,
        MinimizeWindow,
        ToggleFullscreen,
        Quit,
    ]
);

#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = portus, no_json)]
struct SelectTab {
    index: usize,
}

fn main() {
    if std::env::args().any(|arg| arg == "--mcp-stdio") {
        mcp::run_stdio_server();
        return;
    }
    application().with_assets(icons::Assets).run(|cx: &mut App| {
        gpui_kit::init(cx);
        platform::set_app_icon();
        let settings = cx.new(|_| SettingsStore::open());
        let saved = &settings.read(cx).current;
        let mut language = saved.language;
        let mut active_theme = saved.theme;
        active_theme.colors().apply(cx);
        set_menus(language, cx);
        cx.observe(&settings, move |settings, cx| {
            let saved = &settings.read(cx).current;
            let (next_language, next_theme) = (saved.language, saved.theme);
            if language != next_language {
                language = next_language;
                set_menus(language, cx);
            }
            if active_theme != next_theme {
                active_theme = next_theme;
                active_theme.colors().apply(cx);
            }
        })
        .detach();

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("secondary-t", NewTab, None),
            KeyBinding::new("secondary-w", CloseTab, None),
            KeyBinding::new("secondary-shift-w", CloseWindow, None),
            KeyBinding::new("secondary-shift-]", NextTab, None),
            KeyBinding::new("secondary-shift-[", PreviousTab, None),
            KeyBinding::new("secondary-k", ToggleQuickConnect, None),
            KeyBinding::new("secondary-d", SplitPane, None),
            KeyBinding::new("secondary-0", ShowVault, None),
            KeyBinding::new("secondary-shift-s", ToggleSnippetsDrawer, None),
            KeyBinding::new("secondary-shift-e", ToggleFilesDrawer, None),
            KeyBinding::new("secondary-f", FindInTerminal, None),
            KeyBinding::new("secondary-,", ShowSettings, None),
            KeyBinding::new("secondary-m", MinimizeWindow, None),
            KeyBinding::new("ctrl-secondary-f", ToggleFullscreen, None),
            KeyBinding::new("secondary-q", Quit, None),
        ]);
        cx.bind_keys((0..9).map(|index| {
            KeyBinding::new(
                &format!("secondary-{}", index + 1),
                SelectTab { index },
                None,
            )
        }));

        let initial_bg = {
            let saved = &settings.read(cx).current;
            platform::window_background_appearance(saved.opacity, saved.vibrancy)
        };
        let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
        let result = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_background: initial_bg,
                app_id: Some(env!("CARGO_PKG_NAME").into()),
                titlebar: Some(TitlebarOptions {
                    title: Some("Portus".into()),
                    appears_transparent: cfg!(target_os = "macos"),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(680.), px(400.))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| Workspace::new(settings, window, cx)),
        );
        match result {
            Ok((main_window, workspace)) => {
                cx.on_action({
                    let workspace = workspace.clone();
                    move |_: &ShowSettings, cx| {
                        let _ = main_window.update(cx, |_, window, cx| {
                            workspace.update(cx, |ws, cx| ws.open_settings(window, cx));
                            window.activate_window();
                        });
                    }
                });
                cx.on_action({
                    let workspace = workspace.clone();
                    move |_: &ShowHosts, cx| {
                        let _ = main_window.update(cx, |_, window, cx| {
                            workspace.update(cx, |ws, cx| ws.open_hosts(None, window, cx));
                            window.activate_window();
                        });
                    }
                });
                cx.on_action({
                    let workspace = workspace.clone();
                    move |_: &ShowSnippets, cx| {
                        let _ = main_window.update(cx, |_, window, cx| {
                            workspace.update(cx, |ws, cx| ws.open_snippets(window, cx));
                            window.activate_window();
                        });
                    }
                });
                cx.on_action({
                    let workspace = workspace.clone();
                    move |_: &ShowTunnels, cx| {
                        let _ = main_window.update(cx, |_, window, cx| {
                            workspace.update(cx, |ws, cx| ws.open_tunnels(window, cx));
                            window.activate_window();
                        });
                    }
                });
                cx.on_window_closed(move |cx, id| {
                    if id == main_window.window_id() {
                        cx.quit();
                    }
                })
                .detach();
                cx.activate(true);
            }
            Err(error) => {
                eprintln!("Could not open Portus. {error:#}");
                cx.quit();
            }
        }
    });
}

fn set_menus(language: Language, cx: &mut App) {
    let text = |english, chinese| language.text(english, chinese);
    cx.set_menus(vec![
        Menu {
            name: "Portus".into(),
            disabled: false,
            items: vec![
                MenuItem::action(text("Settings…", "设置…"), ShowSettings),
                MenuItem::action(text("Quit Portus", "退出 Portus"), Quit),
            ],
        },
        Menu {
            name: text("File", "文件").into(),
            disabled: false,
            items: vec![
                MenuItem::action(text("New Local Tab", "新建本地标签"), NewTab),
                MenuItem::action(text("Quick Connect…", "快速连接…"), ToggleQuickConnect),
                MenuItem::action(text("Find in Terminal…", "终端内搜索…"), FindInTerminal),
                MenuItem::action(text("SSH Hosts", "SSH 主机"), ShowHosts),
                MenuItem::action(text("Snippets", "命令片段"), ShowSnippets),
                MenuItem::action(text("Port Forwarding", "端口转发"), ShowTunnels),
                MenuItem::action(text("Close Tab", "关闭标签"), CloseTab),
                MenuItem::action(text("Close Window", "关闭窗口"), CloseWindow),
            ],
        },
        Menu {
            name: text("Window", "窗口").into(),
            disabled: false,
            items: vec![
                MenuItem::action(text("Minimize", "最小化"), MinimizeWindow),
                MenuItem::action(text("Toggle Full Screen", "切换全屏"), ToggleFullscreen),
                MenuItem::action(text("Previous Tab", "上一个标签"), PreviousTab),
                MenuItem::action(text("Next Tab", "下一个标签"), NextTab),
            ],
        },
    ]);
}
