//! How a terminal view draws its text: the font, and the colours.

use gpui_kit::{Pixels, Rgba, SharedString};

use crate::consts::{ANSI_16, DEFAULT_BACKGROUND, DEFAULT_FOREGROUND};

/// Visual shape of the terminal cursor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerminalCursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

/// The font, colours, cursor, and interaction style a [`crate::TerminalView`]
/// uses.
///
/// `font_family` must be one GPUI's text system can resolve - registered or
/// installed. An unresolvable name does not fail: GPUI falls back to the
/// app's UI font, which is usually proportional, and a terminal grid in a
/// proportional face is unreadable. Resolve the user's choice against
/// `cx.text_system().all_font_names()` before handing it over, and do that
/// once per change rather than per frame - enumerating installed fonts is
/// slow.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalStyle {
    pub font_family:      SharedString,
    pub font_size:        Pixels,
    pub line_height:      Option<f32>,
    pub palette:          TerminalPalette,
    pub background_alpha: f32,
    pub cursor_style:     TerminalCursorStyle,
    pub cursor_blink:     bool,
    pub copy_on_select:   bool,
    pub option_as_meta:   bool,
}

impl TerminalStyle {
    /// `font_family` at `font_size`, in the [default
    /// palette](TerminalPalette::default).
    pub fn new(font_family: impl Into<SharedString>, font_size: Pixels) -> Self {
        Self { font_family: font_family.into(),
               font_size,
               line_height: None,
               palette: TerminalPalette::default(),
               background_alpha: 1.0,
               cursor_style: TerminalCursorStyle::Block,
               cursor_blink: false,
               copy_on_select: false,
               option_as_meta: true }
    }

    /// Line height multiplier relative to `font_size` (e.g. `1.4`, `1.6`, `1.8`).
    pub fn line_height(mut self, factor: f32) -> Self {
        self.line_height = Some(factor);
        self
    }

    /// This style in `palette` instead - a host's theme, say.
    pub fn palette(mut self, palette: TerminalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Opacity of the default pane background.
    pub fn background_alpha(mut self, alpha: f32) -> Self {
        self.background_alpha = alpha.clamp(0.0, 1.0);
        self
    }

    /// Visual shape of the cursor.
    pub fn cursor_style(mut self, cursor_style: TerminalCursorStyle) -> Self {
        self.cursor_style = cursor_style;
        self
    }

    /// Whether the cursor blinks while the terminal view has focus.
    pub fn cursor_blink(mut self, cursor_blink: bool) -> Self {
        self.cursor_blink = cursor_blink;
        self
    }

    /// Automatically copy selected terminal text to the clipboard on mouse up.
    pub fn copy_on_select(mut self, copy_on_select: bool) -> Self {
        self.copy_on_select = copy_on_select;
        self
    }

    /// Treat Option/Alt as Meta (ESC prefix) instead of platform character composition.
    pub fn option_as_meta(mut self, option_as_meta: bool) -> Self {
        self.option_as_meta = option_as_meta;
        self
    }
}

/// The colours a terminal draws with, each `0xRRGGBB` ([`Self::rgb`] turns a
/// GPUI colour into one). The default is xterm's, on a dark grey pane.
///
/// `ansi` is the sixteen a program addresses by name (SGR 30-37, 90-97 and
/// their backgrounds), which are also the first sixteen of the 256-colour
/// palette; the rest of the 256 and true colour are fixed. `cursor` and
/// `selection` are the cursor cell's and selected cells' backgrounds; `None`
/// draws them by swapping the cell's own colours instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalPalette {
    pub foreground: u32,
    pub background: u32,
    pub ansi:       [u32; 16],
    pub cursor:     Option<u32>,
    pub selection:  Option<u32>,
}

impl TerminalPalette {
    /// `color` as `0xRRGGBB`, its alpha dropped: a theme's `Hsla` or `Rgba`
    /// becomes a palette entry.
    pub fn rgb(color: impl Into<Rgba>) -> u32 {
        let Rgba { r, g, b, .. } = color.into();
        let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
        (channel(r) << 16) | (channel(g) << 8) | channel(b)
    }
}

impl Default for TerminalPalette {
    fn default() -> Self {
        Self { foreground: DEFAULT_FOREGROUND,
               background: DEFAULT_BACKGROUND,
               ansi:       ANSI_16,
               cursor:     None,
               selection:  None, }
    }
}

#[cfg(test)]
mod tests;
