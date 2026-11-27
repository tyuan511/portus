use gpui_kit::{
    Anchor, App, Rgba,
    component::{Theme as ComponentTheme, ThemeMode as ComponentThemeMode},
    px, rgb,
};
use gpui_terminal::TerminalPalette;
use serde::{Deserialize, Serialize};

use crate::settings::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    // Dark themes (10)
    #[default]
    Graphite,
    Midnight,
    Nord,
    Dracula,
    Catppuccin,
    Everforest,
    OneDark,
    GruvboxDark,
    RosePine,
    SolarizedDark,

    // Light themes (10)
    Paper,
    Latte,
    GithubLight,
    TokyoLight,
    NordLight,
    OneLight,
    GruvboxLight,
    RosePineDawn,
    EverforestLight,
    SolarizedLight,
}

impl Theme {
    pub const DARK: [Self; 10] = [
        Self::Graphite,
        Self::Midnight,
        Self::Nord,
        Self::Dracula,
        Self::Catppuccin,
        Self::Everforest,
        Self::OneDark,
        Self::GruvboxDark,
        Self::RosePine,
        Self::SolarizedDark,
    ];

    pub const LIGHT: [Self; 10] = [
        Self::Paper,
        Self::Latte,
        Self::GithubLight,
        Self::TokyoLight,
        Self::NordLight,
        Self::OneLight,
        Self::GruvboxLight,
        Self::RosePineDawn,
        Self::EverforestLight,
        Self::SolarizedLight,
    ];

    pub fn for_mode(mode: ThemeMode) -> &'static [Self] {
        match mode {
            ThemeMode::Dark => &Self::DARK,
            ThemeMode::Light => &Self::LIGHT,
        }
    }

    pub fn mode(self) -> ThemeMode {
        match self {
            Self::Graphite
            | Self::Midnight
            | Self::Nord
            | Self::Dracula
            | Self::Catppuccin
            | Self::Everforest
            | Self::OneDark
            | Self::GruvboxDark
            | Self::RosePine
            | Self::SolarizedDark => ThemeMode::Dark,
            Self::Paper
            | Self::Latte
            | Self::GithubLight
            | Self::TokyoLight
            | Self::NordLight
            | Self::OneLight
            | Self::GruvboxLight
            | Self::RosePineDawn
            | Self::EverforestLight
            | Self::SolarizedLight => ThemeMode::Light,
        }
    }

    pub fn label(self, language: Language) -> &'static str {
        match self {
            Self::Graphite => language.text("Graphite", "石墨"),
            Self::Midnight => language.text("Tokyo Night", "午夜"),
            Self::Nord => language.text("Nord", "北欧"),
            Self::Dracula => language.text("Dracula", "德古拉"),
            Self::Catppuccin => language.text("Mocha", "摩卡"),
            Self::Everforest => language.text("Everforest", "森绿"),
            Self::OneDark => "One Dark",
            Self::GruvboxDark => "Gruvbox",
            Self::RosePine => "Rosé Pine",
            Self::SolarizedDark => "Solarized",
            Self::Paper => language.text("Paper", "纸白"),
            Self::Latte => language.text("Latte", "拿铁"),
            Self::GithubLight => "GitHub",
            Self::TokyoLight => language.text("Tokyo Day", "东京日间"),
            Self::NordLight => language.text("Nord Snow", "北欧雪白"),
            Self::OneLight => "One Light",
            Self::GruvboxLight => "Gruvbox",
            Self::RosePineDawn => language.text("Rosé Dawn", "玫瑰晨曦"),
            Self::EverforestLight => language.text("Everforest", "浅森绿"),
            Self::SolarizedLight => "Solarized",
        }
    }

    pub fn colors(self) -> Colors {
        match self {
            // --- Dark Themes ---
            Self::Graphite => Colors {
                background: 0x151619,
                panel: 0x1c1d21,
                chrome: 0x1e1f23,
                surface: 0x24262b,
                surface_hover: 0x2c2e35,
                line: 0x2e3037,
                line_subtle: 0x23252a,
                foreground: 0xebebef,
                secondary: 0x9c9ea6,
                muted: 0x676972,
                accent: 0x0a84ff,
                accent_fg: 0xffffff,
                success: 0x30d158,
                danger: 0xff453a,
                dark: true,
            },
            Self::Midnight => Colors {
                background: 0x1a1b26,
                panel: 0x1f2335,
                chrome: 0x16161e,
                surface: 0x292e42,
                surface_hover: 0x343b54,
                line: 0x2c324a,
                line_subtle: 0x22273a,
                foreground: 0xc0caf5,
                secondary: 0x9aa5ce,
                muted: 0x6c759e,
                accent: 0x7aa2f7,
                accent_fg: 0x16161e,
                success: 0x9ece6a,
                danger: 0xf7768e,
                dark: true,
            },
            Self::Nord => Colors {
                background: 0x2e3440,
                panel: 0x343b49,
                chrome: 0x272c36,
                surface: 0x3b4252,
                surface_hover: 0x434c5e,
                line: 0x434c5e,
                line_subtle: 0x363d4b,
                foreground: 0xeceff4,
                secondary: 0xd8dee9,
                muted: 0x8894aa,
                accent: 0x88c0d0,
                accent_fg: 0x272c36,
                success: 0xa3be8c,
                danger: 0xbf616a,
                dark: true,
            },
            Self::Dracula => Colors {
                background: 0x282a36,
                panel: 0x2f3241,
                chrome: 0x21222c,
                surface: 0x383b4d,
                surface_hover: 0x44475a,
                line: 0x44475a,
                line_subtle: 0x343746,
                foreground: 0xf8f8f2,
                secondary: 0xc8cce0,
                muted: 0x7e85a8,
                accent: 0xbd93f9,
                accent_fg: 0x1d1e26,
                success: 0x50fa7b,
                danger: 0xff5555,
                dark: true,
            },
            Self::Catppuccin => Colors {
                background: 0x1e1e2e,
                panel: 0x242437,
                chrome: 0x181825,
                surface: 0x313244,
                surface_hover: 0x3b3d52,
                line: 0x383a4f,
                line_subtle: 0x2a2b3d,
                foreground: 0xcdd6f4,
                secondary: 0xa6adc8,
                muted: 0x7f849c,
                accent: 0xcba6f7,
                accent_fg: 0x181825,
                success: 0xa6e3a1,
                danger: 0xf38ba8,
                dark: true,
            },
            Self::Everforest => Colors {
                background: 0x272e33,
                panel: 0x2e363c,
                chrome: 0x1f2529,
                surface: 0x374145,
                surface_hover: 0x414d52,
                line: 0x414b50,
                line_subtle: 0x333c41,
                foreground: 0xd3c6aa,
                secondary: 0xa7b0a0,
                muted: 0x7f897d,
                accent: 0xa7c080,
                accent_fg: 0x1f2529,
                success: 0x83c092,
                danger: 0xe67e80,
                dark: true,
            },
            Self::OneDark => Colors {
                background: 0x282c34,
                panel: 0x2c313c,
                chrome: 0x21252b,
                surface: 0x353b45,
                surface_hover: 0x3e4451,
                line: 0x3b4048,
                line_subtle: 0x30353f,
                foreground: 0xabb2bf,
                secondary: 0x9198a6,
                muted: 0x676f7d,
                accent: 0x61afef,
                accent_fg: 0x1b1f26,
                success: 0x98c379,
                danger: 0xe06c75,
                dark: true,
            },
            Self::GruvboxDark => Colors {
                background: 0x282828,
                panel: 0x32302f,
                chrome: 0x1d2021,
                surface: 0x3c3836,
                surface_hover: 0x494440,
                line: 0x4a443f,
                line_subtle: 0x383432,
                foreground: 0xebdbb2,
                secondary: 0xc5b694,
                muted: 0x928374,
                accent: 0xfabd2f,
                accent_fg: 0x1d2021,
                success: 0xb8bb26,
                danger: 0xfb4934,
                dark: true,
            },
            Self::RosePine => Colors {
                background: 0x191724,
                panel: 0x1f1d2e,
                chrome: 0x14121d,
                surface: 0x26233a,
                surface_hover: 0x312d49,
                line: 0x312e45,
                line_subtle: 0x242136,
                foreground: 0xe0def4,
                secondary: 0xb4b0cc,
                muted: 0x7e7a96,
                accent: 0xebbcba,
                accent_fg: 0x191724,
                success: 0x9ccfd8,
                danger: 0xeb6f92,
                dark: true,
            },
            Self::SolarizedDark => Colors {
                background: 0x002b36,
                panel: 0x073642,
                chrome: 0x00212b,
                surface: 0x0e424f,
                surface_hover: 0x164e5c,
                line: 0x184b59,
                line_subtle: 0x0c3c47,
                foreground: 0xdbe6e6,
                secondary: 0x93a1a1,
                muted: 0x657b83,
                accent: 0x2aa198,
                accent_fg: 0x00212b,
                success: 0x859900,
                danger: 0xdc322f,
                dark: true,
            },

            // --- Light Themes ---
            Self::Paper => Colors {
                background: 0xffffff,
                panel: 0xeeeff3,
                chrome: 0xf4f5f7,
                surface: 0xf1f2f6,
                surface_hover: 0xe7e8ee,
                line: 0xdcdde4,
                line_subtle: 0xeaeaef,
                foreground: 0x1d1f24,
                secondary: 0x5a5e69,
                muted: 0x8b909c,
                accent: 0x007aff,
                accent_fg: 0xffffff,
                success: 0x248a3d,
                danger: 0xd70015,
                dark: false,
            },
            Self::Latte => Colors {
                background: 0xeff1f5,
                panel: 0xe6e9ef,
                chrome: 0xdce0e8,
                surface: 0xd5dae4,
                surface_hover: 0xc9d0de,
                line: 0xccd0da,
                line_subtle: 0xdbe0ea,
                foreground: 0x4c4f69,
                secondary: 0x6c6f85,
                muted: 0x8c8fa1,
                accent: 0x7287fd,
                accent_fg: 0xffffff,
                success: 0x40a02b,
                danger: 0xd20f39,
                dark: false,
            },
            Self::GithubLight => Colors {
                background: 0xffffff,
                panel: 0xf6f8fa,
                chrome: 0xf0f3f6,
                surface: 0xeaeef2,
                surface_hover: 0xdce2e8,
                line: 0xd0d7de,
                line_subtle: 0xe5e9ee,
                foreground: 0x1f2328,
                secondary: 0x57606a,
                muted: 0x7a838e,
                accent: 0x0969da,
                accent_fg: 0xffffff,
                success: 0x1a7f37,
                danger: 0xcf222e,
                dark: false,
            },
            Self::TokyoLight => Colors {
                background: 0xe1e2e7,
                panel: 0xd8dae2,
                chrome: 0xcfd2dc,
                surface: 0xc8ccd8,
                surface_hover: 0xbcc1d0,
                line: 0xbec3d1,
                line_subtle: 0xd0d4df,
                foreground: 0x3760bf,
                secondary: 0x525c7c,
                muted: 0x767f9c,
                accent: 0x2e7de9,
                accent_fg: 0xffffff,
                success: 0x587539,
                danger: 0xf52a65,
                dark: false,
            },
            Self::NordLight => Colors {
                background: 0xeceff4,
                panel: 0xe5e9f0,
                chrome: 0xd8dee9,
                surface: 0xd3dae6,
                surface_hover: 0xc7d0e0,
                line: 0xc8d0de,
                line_subtle: 0xdbe1ec,
                foreground: 0x2e3440,
                secondary: 0x4c566a,
                muted: 0x7b88a1,
                accent: 0x5e81ac,
                accent_fg: 0xffffff,
                success: 0x4c8c5c,
                danger: 0xbf616a,
                dark: false,
            },
            Self::OneLight => Colors {
                background: 0xfafafa,
                panel: 0xf0f0f1,
                chrome: 0xe8e8ea,
                surface: 0xe4e5e7,
                surface_hover: 0xd8d9dd,
                line: 0xd4d5d9,
                line_subtle: 0xe6e7ea,
                foreground: 0x383a42,
                secondary: 0x696c77,
                muted: 0x8f929c,
                accent: 0x4078f2,
                accent_fg: 0xffffff,
                success: 0x50a14f,
                danger: 0xe45649,
                dark: false,
            },
            Self::GruvboxLight => Colors {
                background: 0xfbf1c7,
                panel: 0xf2e5bc,
                chrome: 0xebdbb2,
                surface: 0xe3d1a6,
                surface_hover: 0xd8c494,
                line: 0xd5c4a1,
                line_subtle: 0xe5d5ad,
                foreground: 0x3c3836,
                secondary: 0x665c54,
                muted: 0x8b7e72,
                accent: 0xd65d0e,
                accent_fg: 0xffffff,
                success: 0x79740e,
                danger: 0xcc241d,
                dark: false,
            },
            Self::RosePineDawn => Colors {
                background: 0xfaf4ed,
                panel: 0xf2e9e1,
                chrome: 0xebe0d6,
                surface: 0xe6d9ce,
                surface_hover: 0xdccbc0,
                line: 0xdfdad9,
                line_subtle: 0xece3db,
                foreground: 0x575279,
                secondary: 0x797593,
                muted: 0x9893a5,
                accent: 0xd7827e,
                accent_fg: 0xffffff,
                success: 0x286983,
                danger: 0xb4637a,
                dark: false,
            },
            Self::EverforestLight => Colors {
                background: 0xfdf6e3,
                panel: 0xf4ecd7,
                chrome: 0xebe3cd,
                surface: 0xe5dcc4,
                surface_hover: 0xdad0b6,
                line: 0xd8cfb6,
                line_subtle: 0xe8e0c9,
                foreground: 0x5c6a72,
                secondary: 0x748188,
                muted: 0x939f91,
                accent: 0x8da101,
                accent_fg: 0xffffff,
                success: 0x35a77c,
                danger: 0xf85552,
                dark: false,
            },
            Self::SolarizedLight => Colors {
                background: 0xfdf6e3,
                panel: 0xf5eedb,
                chrome: 0xeee8d5,
                surface: 0xe5dec9,
                surface_hover: 0xdad2bc,
                line: 0xd8d0b8,
                line_subtle: 0xe7e0cb,
                foreground: 0x475b62,
                secondary: 0x657b83,
                muted: 0x93a1a1,
                accent: 0x268bd2,
                accent_fg: 0xffffff,
                success: 0x859900,
                danger: 0xdc322f,
                dark: false,
            },
        }
    }

    pub fn palette(self) -> TerminalPalette {
        match self {
            // --- Dark Palettes ---
            Self::Graphite => TerminalPalette {
                background: 0x111319,
                foreground: 0xe8ecf4,
                cursor: Some(0x5e9eff),
                selection: Some(0x283a5c),
                ansi: [
                    0x1e222d, 0xf87171, 0x34d399, 0xfbbf24, 0x5e9eff, 0xc084fc, 0x2dd4bf, 0xcbd5e1,
                    0x5b6475, 0xfca5a5, 0x6ee7b7, 0xfde68a, 0x93c5fd, 0xd8b4fe, 0x5eead4, 0xf8fafc,
                ],
            },
            Self::Midnight => TerminalPalette {
                background: 0x1a1b26,
                foreground: 0xc0caf5,
                cursor: Some(0x7aa2f7),
                selection: Some(0x33467c),
                ansi: [
                    0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
                    0x414868, 0xff899d, 0xb9e88c, 0xf5cb8c, 0x9ab8ff, 0xd2b8ff, 0xa2e0ff, 0xc0caf5,
                ],
            },
            Self::Nord => TerminalPalette {
                background: 0x2e3440,
                foreground: 0xeceff4,
                cursor: Some(0x88c0d0),
                selection: Some(0x434c5e),
                ansi: [
                    0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
                    0x4c566a, 0xd0727b, 0xb4cfa0, 0xf2d79f, 0x94b3d1, 0xc5a1bf, 0x8fbcbb, 0xeceff4,
                ],
            },
            Self::Dracula => TerminalPalette {
                background: 0x282a36,
                foreground: 0xf8f8f2,
                cursor: Some(0xbd93f9),
                selection: Some(0x44475a),
                ansi: [
                    0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
                    0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
                ],
            },
            Self::Catppuccin => TerminalPalette {
                background: 0x1e1e2e,
                foreground: 0xcdd6f4,
                cursor: Some(0xcba6f7),
                selection: Some(0x45475a),
                ansi: [
                    0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xcba6f7, 0x94e2d5, 0xbac2de,
                    0x585b70, 0xf5a1b8, 0xb9ebb5, 0xfbebc5, 0xa3c5fb, 0xd9bcf9, 0xaae9df, 0xe6ecff,
                ],
            },
            Self::Everforest => TerminalPalette {
                background: 0x272e33,
                foreground: 0xd3c6aa,
                cursor: Some(0xa7c080),
                selection: Some(0x414b50),
                ansi: [
                    0x2e383c, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
                    0x5c6a72, 0xef9496, 0xb8cf94, 0xe6cb94, 0x94cac3, 0xe0adc5, 0x98cfa5, 0xe6decb,
                ],
            },
            Self::OneDark => TerminalPalette {
                background: 0x282c34,
                foreground: 0xabb2bf,
                cursor: Some(0x61afef),
                selection: Some(0x3e4451),
                ansi: [
                    0x21252b, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf,
                    0x5c6370, 0xee858d, 0xb0d693, 0xf0cf90, 0x7ec2f7, 0xd592eb, 0x73cad5, 0xdce1eb,
                ],
            },
            Self::GruvboxDark => TerminalPalette {
                background: 0x282828,
                foreground: 0xebdbb2,
                cursor: Some(0xfabd2f),
                selection: Some(0x504945),
                ansi: [
                    0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
                    0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
                ],
            },
            Self::RosePine => TerminalPalette {
                background: 0x191724,
                foreground: 0xe0def4,
                cursor: Some(0xebbcba),
                selection: Some(0x403d52),
                ansi: [
                    0x26233a, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
                    0x6e6a86, 0xf287a5, 0x458ba8, 0xf9d094, 0xb2dce4, 0xd3bbf0, 0xf2ccc9, 0xf2f0ff,
                ],
            },
            Self::SolarizedDark => TerminalPalette {
                background: 0x002b36,
                foreground: 0x93a1a1,
                cursor: Some(0x2aa198),
                selection: Some(0x073642),
                ansi: [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x586e75, 0xcb4b16, 0x93a1a1, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
            },

            // --- Light Palettes ---
            Self::Paper => TerminalPalette {
                background: 0xfaf8f5,
                foreground: 0x1f2228,
                cursor: Some(0x2563eb),
                selection: Some(0xdbeafe),
                ansi: [
                    0x2d3139, 0xc93b3b, 0x2c7a4b, 0x9a6700, 0x2563eb, 0x7c3aed, 0x0e7490, 0xe5e7eb,
                    0x6b7280, 0xdc4c4c, 0x35945b, 0xb47908, 0x3b82f6, 0x8b5cf6, 0x0891b2, 0xffffff,
                ],
            },
            Self::Latte => TerminalPalette {
                background: 0xeff1f5,
                foreground: 0x4c4f69,
                cursor: Some(0x7287fd),
                selection: Some(0xd3daf9),
                ansi: [
                    0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0x8839ef, 0x179299, 0xacb0be,
                    0x6c6f85, 0xe64553, 0x4cb036, 0xe89b2d, 0x3b78f7, 0x9c55f2, 0x24a4ab, 0xffffff,
                ],
            },
            Self::GithubLight => TerminalPalette {
                background: 0xffffff,
                foreground: 0x1f2328,
                cursor: Some(0x0969da),
                selection: Some(0xd4e8ff),
                ansi: [
                    0x24292f, 0xcf222e, 0x1a7f37, 0x9a6700, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781,
                    0x57606a, 0xa40e26, 0x2da44e, 0xbf8700, 0x218bff, 0xa475f9, 0x3192aa, 0x8c959f,
                ],
            },
            Self::TokyoLight => TerminalPalette {
                background: 0xe1e2e7,
                foreground: 0x3760bf,
                cursor: Some(0x2e7de9),
                selection: Some(0xb7c1e3),
                ansi: [
                    0x3760bf, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x6172b0,
                    0x68709a, 0xff4774, 0x698a47, 0xa37e49, 0x428cf2, 0xab6cf7, 0x0084b0, 0x3760bf,
                ],
            },
            Self::NordLight => TerminalPalette {
                background: 0xeceff4,
                foreground: 0x2e3440,
                cursor: Some(0x5e81ac),
                selection: Some(0xcbd5e6),
                ansi: [
                    0x3b4252, 0xbf616a, 0x4c8c5c, 0xb0802e, 0x5e81ac, 0x9a6894, 0x3f889c, 0x4c566a,
                    0x657187, 0xd0727b, 0x5da16e, 0xc4913b, 0x6f94c0, 0xae7ca8, 0x4f9cb0, 0x2e3440,
                ],
            },
            Self::OneLight => TerminalPalette {
                background: 0xfafafa,
                foreground: 0x383a42,
                cursor: Some(0x4078f2),
                selection: Some(0xd7e4ff),
                ansi: [
                    0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xa0a1a7,
                    0x696c77, 0xed6a5e, 0x62b561, 0xd6960c, 0x5b8cf7, 0xb83db6, 0x1097d1, 0x383a42,
                ],
            },
            Self::GruvboxLight => TerminalPalette {
                background: 0xfbf1c7,
                foreground: 0x3c3836,
                cursor: Some(0xd65d0e),
                selection: Some(0xdfd0a7),
                ansi: [
                    0x3c3836, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0x7c6f64,
                    0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0x282828,
                ],
            },
            Self::RosePineDawn => TerminalPalette {
                background: 0xfaf4ed,
                foreground: 0x575279,
                cursor: Some(0xd7827e),
                selection: Some(0xe7dcd3),
                ansi: [
                    0x575279, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x797593,
                    0x9893a5, 0xc9748c, 0x367e9c, 0xf2ad49, 0x69a7b3, 0xa38dbd, 0xe39490, 0x575279,
                ],
            },
            Self::EverforestLight => TerminalPalette {
                background: 0xfdf6e3,
                foreground: 0x5c6a72,
                cursor: Some(0x8da101),
                selection: Some(0xe4dec8),
                ansi: [
                    0x5c6a72, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0x829181,
                    0x939f91, 0xff6b68, 0x9fb509, 0xedb011, 0x4da6d6, 0xeb7dc9, 0x44b88d, 0x5c6a72,
                ],
            },
            Self::SolarizedLight => TerminalPalette {
                background: 0xfdf6e3,
                foreground: 0x475b62,
                cursor: Some(0x268bd2),
                selection: Some(0xe3dcc8),
                ansi: [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0x657b83,
                    0x586e75, 0xcb4b16, 0x93a1a1, 0x839496, 0x657b83, 0x6c71c4, 0x586e75, 0x002b36,
                ],
            },
        }
    }

    pub fn colors_for(self, opacity: u8) -> Colors {
        self.colors().for_opacity(opacity)
    }

    pub fn palette_for_opacity(self, opacity: u8) -> TerminalPalette {
        let mut pal = self.palette();
        if !self.colors().dark || opacity >= 100 {
            return pal;
        }
        let t = translucency_factor(opacity);
        pal.foreground = lift_dark_text(pal.foreground, t, 0.76);
        pal.ansi[8] = lift_dark_text(pal.ansi[8], t, 0.36);
        for color in &mut pal.ansi[1..=7] {
            if relative_luminance(*color) < 0.16 {
                *color = lift_dark_text(*color, t, 0.24);
            }
        }
        pal
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(hex: u32) -> f32 {
    let r = srgb_to_linear(((hex >> 16) & 0xff) as f32 / 255.0);
    let g = srgb_to_linear(((hex >> 8) & 0xff) as f32 / 255.0);
    let b = srgb_to_linear((hex & 0xff) as f32 / 255.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn mix_rgb(base: u32, target: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let blend = |shift: u32| -> u32 {
        let a = ((base >> shift) & 0xff) as f32;
        let b = ((target >> shift) & 0xff) as f32;
        (a + (b - a) * t).round().clamp(0.0, 255.0) as u32
    };
    (blend(16) << 16) | (blend(8) << 8) | blend(0)
}

fn translucency_factor(opacity: u8) -> f32 {
    f32::from(100u8.saturating_sub(opacity.clamp(50, 100))) / 50.0
}

fn lift_dark_text(hex: u32, translucency: f32, target_luminance: f32) -> u32 {
    if translucency <= 0.0 {
        return hex;
    }
    const CRISP_WHITE: u32 = 0xf8fafc;
    let cur = relative_luminance(hex);
    let amount = if cur >= target_luminance {
        0.10 * translucency
    } else {
        let needed = ((target_luminance - cur) / (0.94 - cur).max(0.01)).clamp(0.0, 1.0);
        (needed.powf(0.75) * translucency)
            .max(0.14 * translucency)
            .min(0.72)
    };
    mix_rgb(hex, CRISP_WHITE, amount)
}

#[derive(Clone, Copy)]
pub struct Colors {
    pub background: u32,
    pub panel: u32,
    pub chrome: u32,
    pub surface: u32,
    pub surface_hover: u32,
    pub line: u32,
    pub line_subtle: u32,
    pub foreground: u32,
    pub secondary: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_fg: u32,
    pub success: u32,
    pub danger: u32,
    pub dark: bool,
}

impl Colors {
    pub fn hairline(self) -> Rgba {
        if self.dark {
            rgb(0xffffff).alpha(0.075)
        } else {
            rgb(0x000000).alpha(0.085)
        }
    }

    pub fn hairline_strong(self) -> Rgba {
        if self.dark {
            rgb(0xffffff).alpha(0.13)
        } else {
            rgb(0x000000).alpha(0.14)
        }
    }

    pub fn hover_bg(self) -> Rgba {
        if self.dark {
            rgb(0xffffff).alpha(0.05)
        } else {
            rgb(0x000000).alpha(0.045)
        }
    }

    pub fn accent_subtle(self) -> Rgba {
        rgb(self.accent).alpha(if self.dark { 0.11 } else { 0.08 })
    }

    pub fn accent_soft(self) -> Rgba {
        rgb(self.accent).alpha(if self.dark { 0.16 } else { 0.12 })
    }

    pub fn accent_border(self) -> Rgba {
        rgb(self.accent).alpha(if self.dark { 0.42 } else { 0.34 })
    }

    pub fn success_soft(self) -> Rgba {
        rgb(self.success).alpha(if self.dark { 0.16 } else { 0.12 })
    }

    pub fn success_border(self) -> Rgba {
        rgb(self.success).alpha(if self.dark { 0.36 } else { 0.28 })
    }

    pub fn danger_soft(self) -> Rgba {
        rgb(self.danger).alpha(if self.dark { 0.15 } else { 0.10 })
    }

    pub fn danger_border(self) -> Rgba {
        rgb(self.danger).alpha(if self.dark { 0.36 } else { 0.28 })
    }

    pub fn overlay_backdrop(self) -> Rgba {
        rgb(0x000000).alpha(if self.dark { 0.56 } else { 0.28 })
    }

    /// Adjusts text tokens when dark themes are rendered over translucent
    /// blurred backdrops so primary, secondary, and muted labels remain crisp
    /// even over bright windows behind Portus. Unchanged at 100% or in light mode.
    pub fn for_opacity(mut self, opacity: u8) -> Self {
        if !self.dark || opacity >= 100 {
            return self;
        }
        let t = translucency_factor(opacity);
        self.foreground = lift_dark_text(self.foreground, t, 0.80);
        self.secondary = lift_dark_text(self.secondary, t, 0.56);
        self.muted = lift_dark_text(self.muted, t, 0.38);
        self
    }

    /// Root window backdrop: transparent when window opacity < 100% so child
    /// regions each paint a single clean alpha layer over the native Gaussian
    /// blur without double-stacking.
    pub fn window_bg(self, opacity: u8) -> Rgba {
        if opacity < 100 {
            rgb(0x000000).alpha(0.0)
        } else {
            rgb(self.chrome)
        }
    }

    fn surface_alpha(self, opacity: u8, factor: f32, floor: f32) -> f32 {
        if opacity >= 100 {
            return 1.0;
        }
        let clamped = opacity.clamp(50, 100);
        if self.dark {
            // Because sRGB luminance is gamma-encoded, mixing a low-alpha dark
            // surface over a bright backdrop washes out contrast far faster than
            // in light mode. Map 50%..100% smoothly onto a dark-safe alpha band
            // that preserves frosted translucency without sacrificing legibility.
            let t = f32::from(clamped - 50) / 50.0;
            let dark_floor = (floor + 0.22).min(0.95);
            let dark_ceil = (0.94 + 0.05 * factor).min(0.99);
            dark_floor + t * (dark_ceil - dark_floor)
        } else {
            let scale = f32::from(clamped) / 100.0;
            (scale * factor).max(floor)
        }
    }

    pub fn sidebar_bg(self, opacity: u8) -> Rgba {
        rgb(self.panel).alpha(self.surface_alpha(opacity, 0.92, 0.44))
    }

    pub fn panel_bg(self, opacity: u8) -> Rgba {
        rgb(self.panel).alpha(self.surface_alpha(opacity, 0.96, 0.48))
    }

    pub fn titlebar_bg(self, opacity: u8) -> Rgba {
        rgb(self.chrome).alpha(self.surface_alpha(opacity, 0.95, 0.46))
    }

    pub fn content_bg(self, opacity: u8) -> Rgba {
        rgb(self.background).alpha(self.surface_alpha(opacity, 1.0, 0.50))
    }

    pub fn term_bg(self, opacity: u8) -> Rgba {
        self.term_bg_for(self.background, opacity)
    }

    pub fn term_bg_for(self, bg_hex: u32, opacity: u8) -> Rgba {
        rgb(bg_hex).alpha(self.surface_alpha(opacity, 1.0, 0.50))
    }

    pub fn popover_bg(self) -> Rgba {
        rgb(self.panel).alpha(if self.dark { 0.94 } else { 0.96 })
    }

    pub fn surface_glass(self) -> Rgba {
        if self.dark {
            rgb(0xffffff).alpha(0.045)
        } else {
            rgb(0x000000).alpha(0.032)
        }
    }

    pub fn surface_hover_glass(self) -> Rgba {
        if self.dark {
            rgb(0xffffff).alpha(0.08)
        } else {
            rgb(0x000000).alpha(0.058)
        }
    }

    pub fn surface_active_glass(self) -> Rgba {
        if self.dark {
            rgb(0xffffff).alpha(0.11)
        } else {
            rgb(0x000000).alpha(0.085)
        }
    }

    pub fn warning(self) -> u32 {
        if self.dark { 0xff9f0a } else { 0xb25000 }
    }

    pub fn warning_soft(self) -> Rgba {
        rgb(self.warning()).alpha(if self.dark { 0.16 } else { 0.12 })
    }

    pub fn warning_border(self) -> Rgba {
        rgb(self.warning()).alpha(if self.dark { 0.36 } else { 0.30 })
    }

    pub fn apply(self, cx: &mut App) {
        let mode = if self.dark {
            ComponentThemeMode::Dark
        } else {
            ComponentThemeMode::Light
        };
        ComponentTheme::change(mode, None, cx);
        ComponentTheme::update(cx, |theme| {
            // Keep gpui-component's 16px rem base so all rem-sized controls
            // (Input, Button, Icon, TabBar, Switch, Tag) render at their
            // designed dimensions without shrinking or clipping text.
            theme.font_size = px(16.);
            theme.mono_font_size = px(13.);
            theme.focus_ring = false;
            theme.notification.placement = Anchor::BottomRight;
            theme.notification.width = px(300.);
            theme.notification.max_items = 2;
            theme.notification.margins.top = px(46.);
            theme.notification.margins.right = px(14.);
            theme.notification.margins.bottom = px(36.);
            theme.notification.margins.left = px(14.);
            let c = &mut theme.colors;
            c.background = rgb(self.background).into();
            c.foreground = rgb(self.foreground).into();
            c.muted = rgb(self.surface).into();
            c.muted_foreground = rgb(self.muted).into();
            c.border = rgb(self.line).into();
            c.input = rgb(self.line).into();
            c.ring = rgb(self.accent).into();
            c.primary = rgb(self.accent).into();
            c.primary_hover = rgb(self.accent).alpha(0.88).into();
            c.primary_active = rgb(self.accent).alpha(0.78).into();
            c.primary_foreground = rgb(self.accent_fg).into();
            c.secondary = rgb(self.surface).into();
            c.secondary_hover = rgb(self.surface_hover).into();
            c.secondary_active = rgb(self.surface_hover).into();
            c.secondary_foreground = rgb(self.foreground).into();
            c.accent = rgb(self.surface_hover).into();
            c.accent_foreground = rgb(self.foreground).into();
            c.popover = rgb(self.panel).into();
            c.popover_foreground = rgb(self.foreground).into();
            c.group_box = rgb(self.panel).into();
            c.group_box_foreground = rgb(self.foreground).into();
            c.danger = rgb(self.danger).into();
            c.danger_hover = rgb(self.danger).alpha(0.88).into();
            c.danger_active = rgb(self.danger).alpha(0.78).into();
            c.danger_foreground = rgb(self.accent_fg).into();
            c.success = rgb(self.success).into();
            c.success_foreground = rgb(self.accent_fg).into();
            c.tab_bar = rgb(self.chrome).into();
            c.tab = rgb(self.chrome).into();
            c.tab_foreground = rgb(self.secondary).into();
            c.tab_active = rgb(self.background).into();
            c.tab_active_foreground = rgb(self.foreground).into();
            c.list = rgb(self.panel).into();
            c.list_hover = rgb(self.surface).into();
            c.list_active = self.accent_soft().into();
            c.list_active_border = self.accent_border().into();
            c.switch = rgb(self.surface_hover).into();
            c.switch_thumb = rgb(self.accent_fg).into();
            c.title_bar = rgb(self.chrome).into();
            c.title_bar_border = rgb(self.line).into();
            c.caret = rgb(self.accent).into();
            c.selection = self.accent_soft().into();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast_ratio(a: u32, b: u32) -> f32 {
        let la = relative_luminance(a);
        let lb = relative_luminance(b);
        let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn opaque_and_light_themes_preserve_exact_colors() {
        for &theme in Theme::DARK.iter().chain(Theme::LIGHT.iter()) {
            let base = theme.colors();
            let at_100 = theme.colors_for(100);
            assert_eq!(base.foreground, at_100.foreground);
            assert_eq!(base.secondary, at_100.secondary);
            assert_eq!(base.muted, at_100.muted);

            let base_pal = theme.palette();
            let pal_100 = theme.palette_for_opacity(100);
            assert_eq!(base_pal.foreground, pal_100.foreground);
            assert_eq!(base_pal.ansi, pal_100.ansi);
        }

        for &theme in &Theme::LIGHT {
            let base = theme.colors();
            let at_50 = theme.colors_for(50);
            assert_eq!(base.foreground, at_50.foreground);
            assert_eq!(base.secondary, at_50.secondary);
            assert_eq!(base.muted, at_50.muted);
        }
    }

    #[test]
    fn dark_themes_remain_legible_at_minimum_opacity_over_bright_backdrop() {
        // DarkAqua HUDWindow (40% #282828 tint) over pure white #ffffff -> #a9a9a9.
        let bright_hud_backdrop = mix_rgb(0xffffff, 0x282828, 0.40);

        for &theme in &Theme::DARK {
            let colors = theme.colors_for(50);
            let content_alpha = colors.content_bg(50).a;
            let sidebar_alpha = colors.sidebar_bg(50).a;
            assert!(content_alpha > 0.68 && content_alpha < 0.85);
            assert!(sidebar_alpha < content_alpha);
            assert!(colors.content_bg(75).a > content_alpha);
            assert!((colors.content_bg(100).a - 1.0).abs() < f32::EPSILON);

            let eff_content = mix_rgb(bright_hud_backdrop, colors.background, content_alpha);
            let eff_sidebar = mix_rgb(bright_hud_backdrop, colors.panel, sidebar_alpha);

            assert!(
                contrast_ratio(colors.foreground, eff_content) >= 6.0,
                "{theme:?} foreground contrast too low"
            );
            assert!(
                contrast_ratio(colors.secondary, eff_content) >= 4.5,
                "{theme:?} secondary contrast too low"
            );
            assert!(
                contrast_ratio(colors.muted, eff_content) >= 3.0,
                "{theme:?} muted contrast too low"
            );
            assert!(
                contrast_ratio(colors.secondary, eff_sidebar) >= 4.0,
                "{theme:?} sidebar secondary contrast too low"
            );

            let pal = theme.palette_for_opacity(50);
            let eff_term = mix_rgb(
                bright_hud_backdrop,
                pal.background,
                colors.term_bg_for(pal.background, 50).a,
            );
            assert!(
                contrast_ratio(pal.foreground, eff_term) >= 6.0,
                "{theme:?} terminal foreground contrast too low"
            );
        }
    }
}
