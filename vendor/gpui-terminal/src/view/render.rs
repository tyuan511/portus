//! Drawing the grid directly onto a single GPUI canvas with a per-row
//! [`ShapedLine`] cache, avoiding per-frame `Div` tree construction and
//! flexbox layout across terminal cells.

use std::sync::Arc;

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use gpui_kit::{
    BorderStyle, Bounds, Context, Font, FontStyle, FontWeight, Hsla, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Pixels, Render, ShapedLine, SharedString,
    StrikethroughStyle, Styled, TextAlign, TextRun, UnderlineStyle, Window, canvas, div, fill,
    font, outline, point, px, rgb, rgba, size,
};

use super::metrics::CellMetrics;
use super::{TerminalCursorStyle, TerminalPalette, TerminalView, palette};
use crate::grid::SearchHighlight;
use crate::{Grid, Transport};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CursorRender {
    Hidden,
    SolidBlock,
    HollowBlock,
    Bar,
    Underline,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CursorOverlay {
    HollowBlock,
    Bar,
    Underline,
}

#[derive(Clone)]
struct CachedRow {
    hash: u64,
    line: Option<Arc<ShapedLine>>,
}

/// Per-row shaped line cache owned by [`TerminalView`].
///
/// Unchanged rows reuse their previously shaped [`ShapedLine`] across frames
/// (cursor blinks, typing on a single line, partial redraws).
#[derive(Default)]
pub(super) struct RowRenderCache {
    rows: Vec<CachedRow>,
}

impl RowRenderCache {
    pub(super) fn clear(&mut self) {
        self.rows.clear();
    }
}

struct CursorOverlayPaint {
    kind:  CursorOverlay,
    color: u32,
    row:   usize,
    x:     Pixels,
    width: Pixels,
}

struct PreparedGrid {
    lines:       Vec<Option<Arc<ShapedLine>>>,
    line_height: Pixels,
    cursor:      Option<CursorOverlayPaint>,
}

impl<T: Transport> Render for TerminalView<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_focus(window, cx);
        let metrics = self.cell_metrics(cx);
        let colors = self.style.palette;
        let cursor_render = if !self.focused {
            match self.style.cursor_style {
                TerminalCursorStyle::Block => CursorRender::HollowBlock,
                TerminalCursorStyle::Bar => CursorRender::Bar,
                TerminalCursorStyle::Underline => CursorRender::Underline,
            }
        }
        else if self.style.cursor_blink && !self.cursor_blink_on {
            CursorRender::Hidden
        }
        else {
            match self.style.cursor_style {
                TerminalCursorStyle::Block => CursorRender::SolidBlock,
                TerminalCursorStyle::Bar => CursorRender::Bar,
                TerminalCursorStyle::Underline => CursorRender::Underline,
            }
        };

        let view = cx.weak_entity();
        let grid_canvas = canvas(
            move |bounds, window, cx| {
                let mut prepared = None;
                let _ = view.update(cx, |view, cx| {
                    view.fit(bounds, window, cx);
                    prepared = Some(view.prepare_grid(metrics, &colors, cursor_render, window));
                });
                prepared
            },
            move |bounds, prepared, window, cx| {
                let Some(prepared) = prepared else {
                    return;
                };
                let origin = bounds.origin;
                let line_height = prepared.line_height;
                for (row_idx, line) in prepared.lines.iter().enumerate() {
                    let Some(line) = line else {
                        continue;
                    };
                    let row_origin = point(origin.x, origin.y + line_height * row_idx as f32);
                    let _ = line.paint_background(
                        row_origin,
                        line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                    let _ = line.paint(
                        row_origin,
                        line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
                if let Some(cursor) = prepared.cursor {
                    let y = origin.y + line_height * cursor.row as f32;
                    let x = origin.x + cursor.x;
                    let quad = match cursor.kind {
                        CursorOverlay::Bar => fill(
                            Bounds::new(point(x, y), size(px(2.), line_height)),
                            rgb(cursor.color),
                        ),
                        CursorOverlay::Underline => fill(
                            Bounds::new(
                                point(x, y + line_height - px(2.)),
                                size(cursor.width, px(2.)),
                            ),
                            rgb(cursor.color),
                        ),
                        CursorOverlay::HollowBlock => outline(
                            Bounds::new(point(x, y), size(cursor.width, line_height)),
                            rgb(cursor.color),
                            BorderStyle::Solid,
                        ),
                    };
                    window.paint_quad(quad);
                }
            },
        )
        .size_full();

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .bg(rgba(
                (colors.background << 8) | (self.style.background_alpha * 255.).round() as u32,
            ))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_left_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_left_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_drop(cx.listener(Self::on_drop_paths))
            .child(grid_canvas)
    }
}

impl<T: Transport> TerminalView<T> {
    fn prepare_grid(
        &mut self,
        metrics: CellMetrics,
        colors: &TerminalPalette,
        cursor_render: CursorRender,
        window: &mut Window,
    ) -> PreparedGrid {
        let base_font = font(self.style.font_family.clone());
        let font_size = self.style.font_size;
        let line_height = px(metrics.height);
        let cell_width = metrics.width;
        let cache = &mut self.row_cache;

        self.terminal.with_grid(|grid| {
            let row_count = grid.size().rows;
            let cursor = if cursor_render == CursorRender::Hidden {
                None
            }
            else {
                grid.cursor_in_view()
            };

            if cache.rows.len() > row_count {
                cache.rows.truncate(row_count);
            }

            let mut lines = Vec::with_capacity(row_count);
            let mut cursor_paint = None;

            for row in 0..row_count {
                let row_cursor_col = cursor.and_then(|(c, r)| (r == row).then_some(c));
                let hash = hash_row(grid, row, row_cursor_col, cursor_render, colors);

                let line = if let Some(cached) = cache.rows.get(row)
                    && cached.hash == hash
                {
                    cached.line.clone()
                }
                else {
                    let shaped = shape_row(
                        grid,
                        row,
                        row_cursor_col,
                        cursor_render,
                        colors,
                        &base_font,
                        font_size,
                        window,
                    );
                    if row < cache.rows.len() {
                        cache.rows[row] = CachedRow { hash,
                                                      line: shaped.clone() };
                    }
                    else {
                        cache.rows.push(CachedRow { hash,
                                                    line: shaped.clone() });
                    }
                    shaped
                };

                if let Some(col) = row_cursor_col {
                    let overlay = match cursor_render {
                        CursorRender::HollowBlock => Some(CursorOverlay::HollowBlock),
                        CursorRender::Bar => Some(CursorOverlay::Bar),
                        CursorRender::Underline => Some(CursorOverlay::Underline),
                        CursorRender::SolidBlock | CursorRender::Hidden => None,
                    };
                    if let Some(kind) = overlay {
                        let cursor_color = colors.cursor.unwrap_or(colors.foreground);
                        let is_wide = grid
                            .row_cells(row)
                            .nth(col)
                            .is_some_and(|c| c.flags.contains(Flags::WIDE_CHAR));
                        let width = px(if is_wide { cell_width * 2. } else { cell_width });
                        let x = cursor_x_for_column(grid, row, col, line.as_deref(), cell_width);
                        cursor_paint = Some(CursorOverlayPaint { kind,
                                                                 color: cursor_color,
                                                                 row,
                                                                 x,
                                                                 width });
                    }
                }

                lines.push(line);
            }

            PreparedGrid { lines,
                           line_height,
                           cursor: cursor_paint }
        })
    }
}

/// Computes the x coordinate for a cursor overlay at `(col, row)`, preferring
/// the shaped line's exact glyph position when available.
fn cursor_x_for_column(
    grid: &Grid,
    row: usize,
    target_col: usize,
    line: Option<&ShapedLine>,
    cell_width: f32,
) -> Pixels {
    if let Some(shaped) = line {
        let mut byte_idx = 0usize;
        for (col, cell) in grid.row_cells(row).enumerate() {
            if col >= target_col {
                break;
            }
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            byte_idx += cell.c.len_utf8();
        }
        if byte_idx <= shaped.len() {
            return shaped.x_for_index(byte_idx);
        }
    }
    px(target_col as f32 * cell_width)
}

#[inline]
fn fnv_mix(mut hash: u64, val: u64) -> u64 {
    hash ^= val;
    hash.wrapping_mul(0x100000001b3)
}

#[inline]
fn style_flags_bits(flags: Flags) -> u16 {
    (flags
        & (Flags::BOLD
            | Flags::ITALIC
            | Flags::UNDERLINE
            | Flags::DOUBLE_UNDERLINE
            | Flags::STRIKEOUT))
        .bits()
}

fn hash_row(
    grid: &Grid,
    row: usize,
    row_cursor_col: Option<usize>,
    cursor_render: CursorRender,
    colors: &TerminalPalette,
) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let solid_cursor = cursor_render == CursorRender::SolidBlock;
    for (column, cell) in grid.row_cells(row).enumerate() {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let inverse = cell.flags.contains(Flags::INVERSE);
        let is_solid_cursor = solid_cursor && row_cursor_col == Some(column);
        let selected = grid.is_selected(column, row);
        let search_hl = grid.search_highlight_at(column, row);
        let has_hyperlink = cell.hyperlink().is_some();
        let (mut foreground, mut background) = cell_colors(
            palette::resolve(cell.fg, true, colors),
            palette::resolve(cell.bg, false, colors),
            inverse,
            is_solid_cursor,
            selected,
            colors,
        );
        if !selected && !is_solid_cursor {
            match search_hl {
                SearchHighlight::Active => {
                    foreground = 0x111111;
                    background = 0xffd60a;
                }
                SearchHighlight::Match => {
                    foreground = 0xffffff;
                    background = 0x5e4a18;
                }
                SearchHighlight::None => {}
            }
        }
        let draw_background = inverse
            || is_solid_cursor
            || selected
            || search_hl != SearchHighlight::None
            || cell.bg != Color::Named(NamedColor::Background);
        let bg_tag: u64 = if draw_background {
            0x1_0000_0000 | u64::from(background)
        }
        else {
            0
        };
        let mut style_bits = u64::from(style_flags_bits(cell.flags));
        if has_hyperlink {
            style_bits |= u64::from(Flags::UNDERLINE.bits());
        }
        hash = fnv_mix(hash, u64::from(cell.c as u32) | (style_bits << 32));
        hash = fnv_mix(hash, u64::from(foreground) | (bg_tag << 24));
    }
    hash
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RunStyle {
    foreground: u32,
    background: Option<u32>,
    style_bits: u16,
}

fn shape_row(
    grid: &Grid,
    row: usize,
    row_cursor_col: Option<usize>,
    cursor_render: CursorRender,
    colors: &TerminalPalette,
    base_font: &Font,
    font_size: Pixels,
    window: &Window,
) -> Option<Arc<ShapedLine>> {
    let solid_cursor = cursor_render == CursorRender::SolidBlock;
    let mut text = String::new();
    let mut runs: Vec<(RunStyle, usize)> = Vec::new();
    let mut last_visible_byte = 0usize;

    for (column, cell) in grid.row_cells(row).enumerate() {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let inverse = cell.flags.contains(Flags::INVERSE);
        let is_solid_cursor = solid_cursor && row_cursor_col == Some(column);
        let selected = grid.is_selected(column, row);
        let search_hl = grid.search_highlight_at(column, row);
        let has_hyperlink = cell.hyperlink().is_some();
        let (mut foreground, mut background) = cell_colors(
            palette::resolve(cell.fg, true, colors),
            palette::resolve(cell.bg, false, colors),
            inverse,
            is_solid_cursor,
            selected,
            colors,
        );
        if !selected && !is_solid_cursor {
            match search_hl {
                SearchHighlight::Active => {
                    foreground = 0x111111;
                    background = 0xffd60a;
                }
                SearchHighlight::Match => {
                    foreground = 0xffffff;
                    background = 0x5e4a18;
                }
                SearchHighlight::None => {}
            }
        }
        let draw_background = inverse
            || is_solid_cursor
            || selected
            || search_hl != SearchHighlight::None
            || cell.bg != Color::Named(NamedColor::Background);
        let background = draw_background.then_some(background);
        let mut style_bits = style_flags_bits(cell.flags);
        if has_hyperlink {
            style_bits |= Flags::UNDERLINE.bits();
        }
        let style = RunStyle { foreground,
                               background,
                               style_bits };

        let ch = if cell.c == '\0' { ' ' } else { cell.c };
        let ch_len = ch.len_utf8();
        text.push(ch);

        let is_blank_cell = ch == ' '
            && background.is_none()
            && !cell
                .flags
                .intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE | Flags::STRIKEOUT);
        if !is_blank_cell {
            last_visible_byte = text.len();
        }

        match runs.last_mut() {
            Some((last_style, len)) if *last_style == style => *len += ch_len,
            _ => runs.push((style, ch_len)),
        }
    }

    if last_visible_byte == 0 {
        return None;
    }

    if last_visible_byte < text.len() {
        text.truncate(last_visible_byte);
        let mut remaining = last_visible_byte;
        let mut kept = 0usize;
        for (_, len) in &mut runs {
            if remaining == 0 {
                break;
            }
            if *len > remaining {
                *len = remaining;
            }
            remaining -= *len;
            kept += 1;
        }
        runs.truncate(kept);
    }

    let text_runs: Vec<TextRun> = runs
        .into_iter()
        .map(|(style, len)| {
            let flags = Flags::from_bits_truncate(style.style_bits);
            let mut run_font = base_font.clone();
            if flags.contains(Flags::BOLD) {
                run_font.weight = FontWeight::BOLD;
            }
            if flags.contains(Flags::ITALIC) {
                run_font.style = FontStyle::Italic;
            }
            let fg: Hsla = rgb(style.foreground).into();
            let underline = flags
                .intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE)
                .then(|| UnderlineStyle { thickness: px(1.),
                                          color:     Some(fg),
                                          wavy:      false });
            let strikethrough = flags.contains(Flags::STRIKEOUT).then(|| StrikethroughStyle {
                thickness: px(1.),
                color:     Some(fg),
            });
            TextRun { len,
                      font: run_font,
                      color: fg,
                      background_color: style.background.map(|bg| rgb(bg).into()),
                      underline,
                      strikethrough }
        })
        .collect();

    let shaped = window
        .text_system()
        .shape_line(SharedString::from(text), font_size, &text_runs, None);
    Some(Arc::new(shaped))
}

/// A cell's foreground and background: its own, inverted when it asks to
/// be; then the cursor's and the selection's - the palette's colour behind
/// the cell when it sets one, else the cell's colours swapped.
pub(super) fn cell_colors(
    foreground: u32,
    background: u32,
    inverse: bool,
    cursor: bool,
    selected: bool,
    colors: &TerminalPalette,
) -> (u32, u32) {
    let (mut foreground, mut background) = if inverse {
        (background, foreground)
    }
    else {
        (foreground, background)
    };
    let mut mark = |active: bool, color: Option<u32>| {
        if !active {
            return;
        }
        match color {
            Some(color) => background = color,
            None => std::mem::swap(&mut foreground, &mut background),
        }
    };
    mark(selected, colors.selection);
    mark(cursor, colors.cursor);
    (foreground, background)
}

#[cfg(test)]
mod tests;
