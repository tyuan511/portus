//! Output-to-cells parsing, backed by `alacritty_terminal`: feed raw bytes
//! into a VT/ANSI parser, read back the cell grid and the events the running
//! program raised (title changes, clipboard requests, bell, OSC notifications,
//! progress bars, shell integration markers, and replies it expects written back).

use std::sync::mpsc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::{Processor, Rgb};

use crate::keys::KeyModes;
use crate::osc::{OscEvent, OscScanner, ShellMark};
use crate::view::TerminalPalette;

const MAX_PROMPT_MARKS: usize = 512;
const SYNC_UPDATE_TIMEOUT: Duration = Duration::from_millis(80);

/// A terminal's visible size, in columns and rows. Scrollback beyond `rows`
/// is tracked by the grid; [`Grid::scroll_display`] moves the visible rows
/// into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridSize {
    pub columns: usize,
    pub rows:    usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// A search match within the grid (including scrollback, where `line < 0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    /// Grid line index (`0..rows` for live screen, `-history..0` for scrollback).
    pub line:      i32,
    pub col_start: usize,
    pub col_end:   usize,
}

/// Visual highlight state for a cell during terminal search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchHighlight {
    None,
    Match,
    Active,
}

/// Forwards `alacritty_terminal` events to a channel the grid drains after
/// each feed. `EventListener::send_event` takes `&self`, so a channel - not a
/// `Vec` - is the simplest way to record them without a lock.
#[derive(Clone)]
pub(crate) struct EventForwarder(mpsc::Sender<Event>);

impl EventListener for EventForwarder {
    fn send_event(&self, event: Event) {
        // The receiver lives in the same `Grid`, so it outlives every send.
        let _ = self.0.send(event);
    }
}

/// One terminal's parsed state.
pub struct Grid {
    term:              Term<EventForwarder>,
    parser:            Processor,
    events:            mpsc::Receiver<Event>,
    osc_scanner:       OscScanner,
    osc_events:        Vec<OscEvent>,
    /// Recorded OSC 133;A prompt start lines (adjusted as scrollback grows).
    prompt_marks:      Vec<i32>,
    /// Active search matches & selected match index for rendering highlights.
    search_matches:    Vec<SearchMatch>,
    search_active:     Option<usize>,
    /// Current theme palette used to answer OSC 10/11/12/4 color queries.
    palette:           TerminalPalette,
    /// Measured cell size in pixels for XTWINOPS text-area size queries.
    cell_size_px:      (u16, u16),
    /// Timestamp when DECSET 2026 synchronized output started, for timeout guard.
    sync_started:      Option<Instant>,
    /// Set whenever what is drawn changes (feed, resize, selection), and
    /// cleared by the view's pump when it asks for a repaint.
    dirty:             bool,
}

impl Grid {
    pub fn new(size: GridSize) -> Self {
        let config = TermConfig { kitty_keyboard: true,
                                  ..TermConfig::default() };
        Self::with_config(size, config)
    }

    pub fn with_scrollback(size: GridSize, scrolling_history: usize) -> Self {
        let config = TermConfig { scrolling_history,
                                  kitty_keyboard: true,
                                  ..TermConfig::default() };
        Self::with_config(size, config)
    }

    fn with_config(size: GridSize, config: TermConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let term = Term::new(config, &size, EventForwarder(tx));
        Self { term,
               parser: Processor::new(),
               events: rx,
               osc_scanner: OscScanner::default(),
               osc_events: Vec::new(),
               prompt_marks: Vec::new(),
               search_matches: Vec::new(),
               search_active: None,
               palette: TerminalPalette::default(),
               cell_size_px: (8, 16),
               sync_started: None,
               dirty: false }
    }

    /// Updates the maximum scrollback history retained by this grid.
    pub fn set_scrollback(&mut self, scrolling_history: usize) {
        let config = TermConfig { scrolling_history,
                                  kitty_keyboard: true,
                                  ..TermConfig::default() };
        self.term.set_options(config);
        self.dirty = true;
    }

    pub fn set_palette(&mut self, palette: TerminalPalette) {
        self.palette = palette;
    }

    pub fn set_cell_size_px(&mut self, width: u16, height: u16) {
        self.cell_size_px = (width.max(1), height.max(1));
    }

    pub(crate) fn color_for_request(&self, index: usize) -> Rgb {
        crate::view::rgb_for_index(index, &self.palette)
    }

    pub(crate) fn window_size(&self) -> WindowSize {
        WindowSize { num_lines:   self.term.screen_lines().min(u16::MAX as usize) as u16,
                     num_cols:    self.term.columns().min(u16::MAX as usize) as u16,
                     cell_width:  self.cell_size_px.0,
                     cell_height: self.cell_size_px.1, }
    }

    /// Parses `bytes` - raw program output - into the grid.
    pub fn feed(&mut self, bytes: &[u8]) {
        let prev_history = self.term.grid().history_size() as i32;
        let prev_sync = self.term.mode().contains(TermMode::SYNC_UPDATE);

        let mut new_osc = Vec::new();
        self.osc_scanner.advance(bytes, &mut new_osc);

        self.parser.advance(&mut self.term, bytes);

        let next_history = self.term.grid().history_size() as i32;
        let hist_delta = next_history - prev_history;
        if hist_delta > 0 {
            let min_line = -next_history;
            for mark in &mut self.prompt_marks {
                *mark -= hist_delta;
            }
            self.prompt_marks.retain(|&m| m >= min_line);
            for m in &mut self.search_matches {
                m.line -= hist_delta;
            }
            self.search_matches.retain(|m| m.line >= min_line);
        }

        for ev in new_osc {
            if matches!(ev, OscEvent::ShellMark(ShellMark::PromptStart)) {
                let line = self.term.grid().cursor.point.line.0;
                if self.prompt_marks.last().copied() != Some(line) {
                    self.prompt_marks.push(line);
                    if self.prompt_marks.len() > MAX_PROMPT_MARKS {
                        let excess = self.prompt_marks.len() - MAX_PROMPT_MARKS;
                        self.prompt_marks.drain(..excess);
                    }
                }
            }
            self.osc_events.push(ev);
        }

        let now_sync = self.term.mode().contains(TermMode::SYNC_UPDATE);
        if now_sync && !prev_sync {
            self.sync_started = Some(Instant::now());
        }
        else if !now_sync {
            self.sync_started = None;
        }

        self.dirty = true;
    }

    /// Whether DEC Mode 2026 synchronized output is currently buffering a frame
    /// (returns `false` if the safety timeout elapsed so a hung app can't freeze UI).
    pub fn sync_update_active(&self) -> bool {
        if !self.term.mode().contains(TermMode::SYNC_UPDATE) {
            return false;
        }
        self.sync_started
            .is_some_and(|started| started.elapsed() < SYNC_UPDATE_TIMEOUT)
    }

    /// Resizes the grid. The transport's own size is the caller's to change;
    /// [`crate::Terminal::resize`] does both.
    pub fn resize(&mut self, size: GridSize) {
        self.term.resize(size);
        self.dirty = true;
    }

    pub fn size(&self) -> GridSize {
        GridSize { columns: self.term.columns(),
                   rows:    self.term.screen_lines(), }
    }

    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn key_modes(&self) -> KeyModes {
        KeyModes::from_term_mode(*self.term.mode())
    }

    pub fn focus_reporting(&self) -> bool {
        self.term.mode().contains(TermMode::FOCUS_IN_OUT)
    }

    pub fn alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn alternate_scroll(&self) -> bool {
        self.term.mode().contains(TermMode::ALTERNATE_SCROLL)
    }

    /// The cursor's column and row within the screen, 0-indexed - where the
    /// program put it, wherever the view is scrolled.
    pub fn cursor(&self) -> (usize, usize) {
        let point = self.term.grid().cursor.point;
        (point.column.0, point.line.0.max(0) as usize)
    }

    /// The cursor's column and visible row, or `None` while the cursor is
    /// hidden by the program (DECTCEM) or scrolled out of view.
    pub fn cursor_in_view(&self) -> Option<(usize, usize)> {
        if !self.term.mode().contains(TermMode::SHOW_CURSOR) {
            return None;
        }
        let (column, row) = self.cursor();
        let row = row + self.display_offset();
        (row < self.size().rows).then_some((column, row))
    }

    /// Moves the visible rows `lines` back into the scrollback (negative:
    /// forward, toward the live screen), stopping at either end. Whether the
    /// view moved. Nothing to scroll on the alternate screen, which keeps no
    /// scrollback.
    pub fn scroll_display(&mut self, lines: i32) -> bool {
        let before = self.display_offset();
        self.term.scroll_display(Scroll::Delta(lines));
        let moved = self.display_offset() != before;
        self.dirty |= moved;
        moved
    }

    /// Returns the view to the live screen. Whether it moved.
    pub fn scroll_to_bottom(&mut self) -> bool {
        let moved = self.display_offset() != 0;
        if moved {
            self.term.scroll_display(Scroll::Bottom);
            self.dirty = true;
        }
        moved
    }

    /// Scrolls the viewport so `line` (negative for scrollback, `0..rows` for live
    /// screen) is centered in view when possible.
    pub fn scroll_to_line(&mut self, line: i32) -> bool {
        let rows = self.size().rows as i32;
        let history = self.history_size() as i32;
        if history == 0 {
            return false;
        }
        let target_top = (line - rows / 3).clamp(-history, 0);
        let target_offset = (-target_top) as usize;
        let current = self.display_offset();
        if target_offset == current {
            return false;
        }
        let delta = target_offset as i32 - current as i32;
        self.scroll_display(delta)
    }

    /// Jumps to the previous or next recorded OSC 133 prompt start.
    pub fn jump_to_prompt(&mut self, previous: bool) -> bool {
        if self.prompt_marks.is_empty() {
            return false;
        }
        let top_line = -(self.display_offset() as i32);
        let target = if previous {
            self.prompt_marks
                .iter()
                .rev()
                .copied()
                .find(|&m| m < top_line)
        }
        else {
            self.prompt_marks
                .iter()
                .copied()
                .find(|&m| m > top_line)
        };
        if let Some(line) = target {
            let target_offset = (-line).clamp(0, self.history_size() as i32) as usize;
            let delta = target_offset as i32 - self.display_offset() as i32;
            self.scroll_display(delta)
        }
        else if !previous {
            self.scroll_to_bottom()
        }
        else {
            false
        }
    }

    /// How many rows back into the scrollback the view is: 0 on the live
    /// screen.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Whether the program has turned on SGR mouse reporting. When it has
    /// not, mouse input drives the grid's own selection instead of being sent
    /// to the program.
    pub fn sgr_mouse_mode(&self) -> bool {
        let mode = self.term.mode();
        mode.contains(TermMode::SGR_MOUSE) && mode.intersects(TermMode::MOUSE_MODE)
    }

    /// Whether the program has turned on bracketed paste, and so wants pasted
    /// text wrapped in markers it can recognise.
    pub fn bracketed_paste_mode(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Searches the entire terminal buffer (scrollback + visible screen)
    /// case-insensitively and returns all matches in chronological order.
    pub fn search_all(&self, query: &str) -> Vec<SearchMatch> {
        let query_lower = query.to_lowercase();
        if query_lower.is_empty() {
            return Vec::new();
        }
        let history = self.history_size() as i32;
        let rows = self.size().rows as i32;
        let col_len = query_lower.chars().count();
        let mut found = Vec::new();
        for line in -history..rows {
            let text = self.line_text(line);
            if text.is_empty() {
                continue;
            }
            let lower = text.to_lowercase();
            let mut start = 0;
            while let Some(pos) = lower[start..].find(&query_lower) {
                let byte_start = start + pos;
                let col_start = lower[..byte_start].chars().count();
                let col_end = col_start + col_len.saturating_sub(1);
                found.push(SearchMatch { line,
                                         col_start,
                                         col_end });
                start = byte_start + query_lower.len().max(1);
            }
        }
        found
    }

    /// Sets the search matches to highlight on the terminal canvas.
    pub fn set_search_highlights(&mut self, matches: Vec<SearchMatch>, active: Option<usize>) {
        self.search_matches = matches;
        self.search_active = active;
        self.dirty = true;
    }

    /// Clears all search highlights.
    pub fn clear_search_highlights(&mut self) {
        if !self.search_matches.is_empty() || self.search_active.is_some() {
            self.search_matches.clear();
            self.search_active = None;
            self.dirty = true;
        }
    }

    /// Returns the search highlight state for a visible cell `(column, row)`.
    pub(crate) fn search_highlight_at(&self, column: usize, row: usize) -> SearchHighlight {
        if self.search_matches.is_empty() {
            return SearchHighlight::None;
        }
        let line = self.point_at(column, row).line.0;
        let mut state = SearchHighlight::None;
        for (idx, m) in self.search_matches.iter().enumerate() {
            if m.line == line && column >= m.col_start && column <= m.col_end {
                if self.search_active == Some(idx) {
                    return SearchHighlight::Active;
                }
                state = SearchHighlight::Match;
            }
        }
        state
    }

    /// Returns an OSC 8 hyperlink URI at visible cell `(column, row)`, if any.
    pub fn hyperlink_at(&self, column: usize, row: usize) -> Option<String> {
        let size = self.size();
        if row >= size.rows || column >= size.columns {
            return None;
        }
        let point = self.point_at(column, row);
        self.term.grid()[point.line][point.column]
            .hyperlink()
            .map(|h| h.uri().to_owned())
    }

    /// Returns either an OSC 8 hyperlink or a detected `http(s)://` URL under `(column, row)`.
    pub fn url_at(&self, column: usize, row: usize) -> Option<String> {
        if let Some(uri) = self.hyperlink_at(column, row) {
            return Some(uri);
        }
        let size = self.size();
        if row >= size.rows || column >= size.columns {
            return None;
        }
        let text = self.row_text(row);
        find_url_at_column(&text, column)
    }

    /// Starts a plain text selection at this visible cell, replacing any
    /// other. Cells are where they show, so a selection made scrolled back
    /// covers the scrollback rows it was made over.
    pub fn start_selection(&mut self, column: usize, row: usize) {
        let point = self.point_at(column, row);
        self.term.selection = Some(Selection::new(SelectionType::Simple, point, Side::Left));
        self.dirty = true;
    }

    /// Starts a word (semantic) selection at this visible cell.
    pub fn start_word_selection(&mut self, column: usize, row: usize) {
        let point = self.point_at(column, row);
        self.term.selection = Some(Selection::new(SelectionType::Semantic, point, Side::Left));
        self.dirty = true;
    }

    /// Starts a whole-line selection at this visible cell.
    pub fn start_line_selection(&mut self, column: usize, row: usize) {
        let point = self.point_at(column, row);
        self.term.selection = Some(Selection::new(SelectionType::Lines, point, Side::Left));
        self.dirty = true;
    }

    /// Extends the selection in progress, if any, to this visible cell.
    pub fn update_selection(&mut self, column: usize, row: usize) {
        let point = self.point_at(column, row);
        if let Some(selection) = &mut self.term.selection {
            selection.update(point, Side::Right);
            self.dirty = true;
        }
    }

    pub fn clear_selection(&mut self) {
        if self.term.selection.take().is_some() {
            self.dirty = true;
        }
    }

    /// The selected text, or `None` for no selection or an empty one.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    /// Whether this cell is inside the selection, for drawing a highlight.
    pub fn is_selected(&self, column: usize, row: usize) -> bool {
        self.term
            .selection
            .as_ref()
            .and_then(|selection| selection.to_range(&self.term))
            .is_some_and(|range| range.contains(self.point_at(column, row)))
    }

    /// A visible row's text, trailing blanks trimmed - scrolled back, a row of
    /// the scrollback.
    pub fn row_text(&self, row: usize) -> String {
        let mut text: String = self.row_cells(row).map(|cell| cell.c).collect();
        text.truncate(text.trim_end_matches(' ').len());
        text
    }

    fn line_text(&self, line: i32) -> String {
        let mut text: String = self.term.grid()[Line(line)]
            .into_iter()
            .map(|cell| cell.c)
            .collect();
        text.truncate(text.trim_end_matches(' ').len());
        text
    }

    /// A visible row's cells, left to right. Panics like [`Self::row_text`].
    pub(crate) fn row_cells(&self, row: usize) -> impl Iterator<Item = &Cell> {
        self.term.grid()[self.point_at(0, row).line].into_iter()
    }

    /// Whether the grid changed since the last call, clearing the flag.
    pub(crate) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Every alacritty event raised since the last call.
    pub(crate) fn drain_events(&mut self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Every sideband OSC event raised since the last call.
    pub(crate) fn drain_osc_events(&mut self) -> Vec<OscEvent> {
        std::mem::take(&mut self.osc_events)
    }
}

impl Grid {
    /// The grid point a visible cell shows: `row` rows down from the top of
    /// the view, which is `display_offset` rows back into the scrollback.
    fn point_at(&self, column: usize, row: usize) -> Point {
        Point::new(Line(row as i32 - self.display_offset() as i32),
                   Column(column))
    }
}

fn find_url_at_column(line: &str, target_col: usize) -> Option<String> {
    for scheme in ["https://", "http://"] {
        let mut search_start = 0;
        while let Some(pos) = line[search_start..].find(scheme) {
            let byte_start = search_start + pos;
            let rest = &line[byte_start..];
            let byte_len = rest
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`' | ')' | ']' | '}')
                })
                .unwrap_or(rest.len());
            let raw = &rest[..byte_len];
            let trimmed = raw.trim_end_matches(['.', ',', ';', ':']);
            let col_start = line[..byte_start].chars().count();
            let col_end = col_start + trimmed.chars().count();
            if target_col >= col_start && target_col < col_end && trimmed.len() > scheme.len() {
                return Some(trimmed.to_owned());
            }
            search_start = byte_start + byte_len.max(1);
        }
    }
    None
}

#[cfg(test)]
mod tests;
