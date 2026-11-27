use std::borrow::Cow;

use gpui_kit::{
    AssetSource, Div, Img, ParentElement as _, Pixels, Result, SharedString, Styled as _, Svg,
    div, img, px, rgb, svg,
};

pub const APP_LOGO_PATH: &str = "icons/app_logo.png";
pub const APP_LOGO_BYTES: &[u8] = include_bytes!("../assets/logo-ui.png");

pub fn app_logo(size: Pixels) -> Img {
    img(APP_LOGO_PATH).size(size).flex_shrink_0()
}

pub struct Assets;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Shield,
    Terminal,
    Server,
    Settings,
    Plus,
    Close,
    Search,
    Import,
    Duplicate,
    Trash,
    Folder,
    Globe,
    Check,
    Play,
    Edit,
    Refresh,
    Lock,
    Code,
    Network,
    Cloud,
    Database,
    Cpu,
    HardDrive,
    Monitor,
    ArrowUp,
    ArrowDown,
    Upload,
    Key,
    Split,
    LayoutGrid,
    List,
    Palette,
    Keyboard,
    Expand,
    Mcp,
    Copy,
    Home,
}

impl Icon {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Shield => "icons/shield.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::Server => "icons/server.svg",
            Self::Settings => "icons/settings.svg",
            Self::Plus => "icons/plus.svg",
            Self::Close => "icons/close.svg",
            Self::Search => "icons/search.svg",
            Self::Import => "icons/import.svg",
            Self::Duplicate => "icons/duplicate.svg",
            Self::Trash => "icons/trash.svg",
            Self::Folder => "icons/folder.svg",
            Self::Globe => "icons/globe.svg",
            Self::Check => "icons/check.svg",
            Self::Play => "icons/play.svg",
            Self::Edit => "icons/edit.svg",
            Self::Refresh => "icons/refresh.svg",
            Self::Lock => "icons/lock.svg",
            Self::Code => "icons/code.svg",
            Self::Network => "icons/network.svg",
            Self::Cloud => "icons/cloud.svg",
            Self::Database => "icons/database.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::HardDrive => "icons/hard_drive.svg",
            Self::Monitor => "icons/monitor.svg",
            Self::ArrowUp => "icons/arrow_up.svg",
            Self::ArrowDown => "icons/arrow_down.svg",
            Self::Upload => "icons/upload.svg",
            Self::Key => "icons/key.svg",
            Self::Split => "icons/split.svg",
            Self::LayoutGrid => "icons/layout_grid.svg",
            Self::List => "icons/list.svg",
            Self::Palette => "icons/palette.svg",
            Self::Keyboard => "icons/keyboard.svg",
            Self::Expand => "icons/expand.svg",
            Self::Mcp => "icons/mcp.svg",
            Self::Copy => "icons/copy.svg",
            Self::Home => "icons/home.svg",
        }
    }

    pub fn render(self, size: Pixels, color: u32) -> Svg {
        svg()
            .path(self.path())
            .size(size)
            .flex_shrink_0()
            .text_color(rgb(color))
    }

    /// Apple HIG / macOS System Settings squircle tile color for each icon.
    pub const fn macos_color(self) -> u32 {
        match self {
            Self::Server => 0x007aff,     // Apple Blue
            Self::Key => 0xff9500,        // Apple Orange
            Self::Network => 0x34c759,    // Apple Green
            Self::Code => 0xaf52de,       // Apple Purple
            Self::Folder => 0x32ade6,     // Apple Cyan / Finder Blue
            Self::Settings => 0x636366,   // Apple System Gray
            Self::Terminal => 0x2c2c2e,   // Apple Charcoal
            Self::Cloud => 0x30b0c7,      // Apple Teal
            Self::Database => 0x5856d6,   // Apple Indigo
            Self::Cpu => 0xff2d55,        // Apple Pink
            Self::HardDrive => 0x00c7be,  // Apple Mint
            Self::Monitor => 0x0a84ff,    // Apple Sky Blue
            Self::Globe => 0x007aff,      // Apple Blue
            Self::Shield => 0x30d158,     // Apple Green
            Self::Lock => 0xff3b30,       // Apple Red
            Self::Split => 0x32ade6,      // Apple Cyan
            Self::Play => 0x34c759,       // Apple Green
            Self::Trash => 0xff3b30,      // Apple Red
            Self::Search | Self::Import | Self::Upload => 0x007aff,
            Self::LayoutGrid | Self::List => 0x5856d6,
            Self::Palette => 0xaf52de,    // Apple Purple
            Self::Keyboard => 0x636366,   // Apple System Gray
            Self::Mcp => 0x5856d6,        // Apple Indigo
            _ => 0x007aff,
        }
    }

    /// Renders a macOS System Settings style colorful squircle icon tile with a white symbol.
    pub fn badge(self, size: Pixels) -> Div {
        self.badge_with_color(size, self.macos_color())
    }

    pub fn badge_with_color(self, size: Pixels, bg: u32) -> Div {
        let raw: f32 = size.into();
        let radius = px((raw * 0.24).clamp(4.0, 12.0));
        let glyph = px((raw * 0.56).max(10.0));
        div()
            .size(size)
            .flex_shrink_0()
            .rounded(radius)
            .bg(rgb(bg))
            .flex()
            .items_center()
            .justify_center()
            .child(self.render(glyph, 0xffffff))
    }

    fn svg_bytes(path: &str) -> Option<&'static [u8]> {
        match path {
            "icons/shield.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.5 4.5 5.5v6c0 5 3.2 8.7 7.5 10 4.3-1.3 7.5-5 7.5-10v-6L12 2.5z"/><path d="M9.2 10.2 11.4 12l-2.2 1.8"/><path d="M13 13.8h2.4"/></svg>"##,
            ),
            "icons/terminal.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="m7.5 9.5 3 2.5-3 2.5"/><path d="M12.5 14.5h4"/></svg>"##,
            ),
            "icons/server.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3.5" width="18" height="7" rx="2"/><rect x="3" y="13.5" width="18" height="7" rx="2"/><path d="M7 7h.01"/><path d="M7 17h.01"/><path d="M11 7h6"/><path d="M11 17h6"/></svg>"##,
            ),
            "icons/settings.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3.2"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9c.26.604.852.997 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>"##,
            ),
            "icons/plus.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14"/><path d="M5 12h14"/></svg>"##,
            ),
            "icons/close.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>"##,
            ),
            "icons/search.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"##,
            ),
            "icons/import.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"/></svg>"##,
            ),
            "icons/copy.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>"##,
            ),
            "icons/duplicate.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="8" y="8" width="13" height="13" rx="2.5"/><path d="M16 8V5.5A2.5 2.5 0 0 0 13.5 3h-8A2.5 2.5 0 0 0 3 5.5v8A2.5 2.5 0 0 0 5.5 16H8"/><path d="M14.5 11.5v6"/><path d="M11.5 14.5h6"/></svg>"##,
            ),
            "icons/trash.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4.5A1.5 1.5 0 0 1 9.5 3h5A1.5 1.5 0 0 1 16 4.5V6"/><path d="M6 6l1 14a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-14"/></svg>"##,
            ),
            "icons/folder.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4.5l2 2H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/></svg>"##,
            ),
            "icons/palette.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a10 10 0 0 0 0 20c1.4 0 2.5-1.1 2.5-2.5 0-.6-.2-1.2-.6-1.6-.4-.4-.6-.9-.6-1.5 0-1.1.9-2 2-2h2.2c2.5 0 4.5-2 4.5-4.5C22 5.4 17.5 2 12 2z"/><circle cx="7.5" cy="11.5" r="1" fill="currentColor" stroke="none"/><circle cx="10.5" cy="7.5" r="1" fill="currentColor" stroke="none"/><circle cx="15.5" cy="8" r="1" fill="currentColor" stroke="none"/></svg>"##,
            ),
            "icons/globe.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a14 14 0 0 1 0 18"/><path d="M12 3a14 14 0 0 0 0 18"/></svg>"##,
            ),
            "icons/check.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>"##,
            ),
            "icons/play.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><polygon points="6 4 20 12 6 20 6 4"/></svg>"##,
            ),
            "icons/edit.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>"##,
            ),
            "icons/refresh.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-2.64-6.36"/><path d="M21 3v6h-6"/></svg>"##,
            ),
            "icons/lock.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>"##,
            ),
            "icons/code.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>"##,
            ),
            "icons/network.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="5" r="2.5"/><circle cx="5" cy="19" r="2.5"/><circle cx="19" cy="19" r="2.5"/><path d="M12 7.5v4.5"/><path d="M12 12 6.5 17"/><path d="M12 12l5.5 5"/></svg>"##,
            ),
            "icons/cloud.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M17.5 19H9a5 5 0 0 1-.5-9.98A6 6 0 0 1 20 11a4 4 0 0 1-2.5 8Z"/></svg>"##,
            ),
            "icons/database.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 1.66 3.58 3 8 3s8-1.34 8-3V5"/><path d="M4 12c0 1.66 3.58 3 8 3s8-1.34 8-3"/></svg>"##,
            ),
            "icons/cpu.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="6" y="6" width="12" height="12" rx="2"/><rect x="9" y="9" width="6" height="6"/><path d="M9 2v4"/><path d="M15 2v4"/><path d="M9 18v4"/><path d="M15 18v4"/><path d="M2 9h4"/><path d="M2 15h4"/><path d="M18 9h4"/><path d="M18 15h4"/></svg>"##,
            ),
            "icons/hard_drive.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M22 12H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><line x1="6" x2="6.01" y1="16" y2="16"/><line x1="10" x2="10.01" y1="16" y2="16"/></svg>"##,
            ),
            "icons/monitor.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/></svg>"##,
            ),
            "icons/arrow_up.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5"/><path d="m5 12 7-7 7 7"/></svg>"##,
            ),
            "icons/arrow_down.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>"##,
            ),
            "icons/upload.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 15V3"/><path d="m7 8 5-5 5 5"/><path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"/></svg>"##,
            ),
            "icons/chevron_right.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>"##,
            ),
            "icons/chevron_down.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"##,
            ),
            "icons/key.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="15" r="4"/><path d="m11 12 9-9"/><path d="m17 6 2 2"/><path d="m14 9 2 2"/></svg>"##,
            ),
            "icons/split.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M12 4v16"/></svg>"##,
            ),
            "icons/layout_grid.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>"##,
            ),
            "icons/list.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M8 6h13"/><path d="M8 12h13"/><path d="M8 18h13"/><path d="M3 6h.01"/><path d="M3 12h.01"/><path d="M3 18h.01"/></svg>"##,
            ),
            "icons/activity.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M22 12h-4l-3 7-6-14-3 7H2"/></svg>"##,
            ),
            "icons/expand.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="m21 3-7 7"/><path d="m3 21 7-7"/></svg>"##,
            ),
            "icons/keyboard.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01"/><path d="M10 10h.01"/><path d="M14 10h.01"/><path d="M18 10h.01"/><path d="M8 14h8"/></svg>"##,
            ),
            "icons/mcp.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a4 4 0 0 1 4 4v2h2a3 3 0 0 1 0 6h-2v2a4 4 0 0 1-8 0v-2H6a3 3 0 0 1 0-6h2V6a4 4 0 0 1 4-4z"/><circle cx="12" cy="11" r="1.5"/></svg>"##,
            ),
            "icons/home.svg" => Some(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>"##,
            ),
            _ => None,
        }
    }
}

impl gpui_kit::component::IconNamed for Icon {
    fn path(self) -> SharedString {
        Icon::path(self).into()
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_LOGO_PATH {
            return Ok(Some(Cow::Borrowed(APP_LOGO_BYTES)));
        }
        if let Some(bytes) = Icon::svg_bytes(path).or_else(|| crate::file_icons::svg_bytes(path)) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}
