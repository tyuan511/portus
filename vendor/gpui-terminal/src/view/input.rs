//! Keys, mouse, scroll, copy and paste: each translates a GPUI event into
//! transport input or a grid change, and does nothing else.

use gpui_kit::{
    ClipboardItem, Context, ExternalPaths, Keystroke, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, Window,
};

use super::TerminalView;
use crate::{
    KeyInput, MouseButton, MouseInput, TerminalEvent, Transport, key_to_bytes_with_modes,
    mouse_to_bytes, paste_payload,
};

impl<T: Transport> TerminalView<T> {
    /// Copies the selection to the system clipboard, trailing whitespace
    /// trimmed from each line - the padding a grid row carries is not text
    /// the user selected.
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.terminal.with_grid(|grid| grid.selection_text())
        else {
            return;
        };
        let text = text.lines()
                       .map(str::trim_end)
                       .collect::<Vec<_>>()
                       .join("\n");
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Writes the system clipboard's text to the terminal, bracketed when the
    /// program asked for bracketed paste. If the clipboard holds no text (for
    /// example, a screenshot), emits [`TerminalEvent::ImagePasteRequested`] so
    /// the host can paste a local or SFTP-uploaded image path.
    pub fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.exited() {
            return;
        }
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        let Some(text) = text.filter(|t| !t.is_empty())
        else {
            cx.emit(TerminalEvent::ImagePasteRequested);
            return;
        };
        let bracketed = self.terminal.with_grid(|grid| grid.bracketed_paste_mode());
        if let Some(payload) = paste_payload(&text, bracketed) {
            self.return_to_live_screen(cx);
            self.reset_cursor_blink(cx);
            let written = self.terminal.write(payload.as_bytes());
            self.report(written, cx);
        }
    }

    /// Handles a keystroke typed while the view has focus.
    pub(super) fn handle_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        match clipboard_chord(keystroke, cfg!(target_os = "macos")) {
            Some(Clipboard::Copy) => {
                self.copy_selection(cx);
                return true;
            }
            Some(Clipboard::Paste) => {
                self.paste_clipboard(cx);
                return true;
            }
            None => {}
        }
        if keystroke.modifiers.platform || self.exited() {
            return false;
        }
        let input = KeyInput { key:            &keystroke.key,
                               key_char:       keystroke.key_char.as_deref(),
                               control:        keystroke.modifiers.control,
                               alt:            keystroke.modifiers.alt,
                               shift:          keystroke.modifiers.shift,
                               option_as_meta: self.style.option_as_meta, };
        let modes = self.terminal.with_grid(|grid| grid.key_modes());
        let Some(bytes) = key_to_bytes_with_modes(input, modes)
        else {
            return false;
        };
        self.return_to_live_screen(cx);
        self.reset_cursor_blink(cx);
        let written = self.terminal.write(&bytes);
        self.report(written, cx);
        true
    }

    /// Scrolls back to the live screen before input goes to the program, as
    /// terminals do: what is typed shows where it lands.
    fn return_to_live_screen(&mut self, cx: &mut Context<Self>) {
        if self.terminal.with_grid_mut(|grid| grid.scroll_to_bottom()) {
            cx.notify();
        }
    }

    pub(super) fn on_left_down(&mut self, event: &MouseDownEvent, window: &mut Window,
                               cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.reset_cursor_blink(cx);
        let Some((column, row)) = self.cell_at(event.position)
        else {
            return;
        };
        // Cmd+Click opens OSC 8 hyperlinks or detected URLs.
        if event.modifiers.platform
           && let Some(uri) = self.terminal.with_grid(|grid| grid.url_at(column, row))
        {
            cx.emit(TerminalEvent::OpenUrl(uri));
            return;
        }
        // Holding Shift bypasses SGR mouse reporting so users can always select
        // and copy text inside full-screen mouse-aware AI TUIs / tmux.
        let bypass_mouse = event.modifiers.shift;
        if !bypass_mouse && self.mouse_reporting() {
            self.mouse_button(event.position, MouseButton::Left, true, cx);
            return;
        }
        self.terminal.with_grid_mut(|grid| match event.click_count {
                         2 => grid.start_word_selection(column, row),
                         count if count >= 3 => grid.start_line_selection(column, row),
                         _ => grid.start_selection(column, row),
                     });
        self.selection_dragged = event.click_count >= 2;
        cx.notify();
    }

    pub(super) fn on_left_up(&mut self, event: &MouseUpEvent, _window: &mut Window,
                             cx: &mut Context<Self>) {
        let bypass_mouse = event.modifiers.shift;
        if !bypass_mouse && self.mouse_reporting() && !self.selection_dragged {
            self.mouse_button(event.position, MouseButton::Left, false, cx);
            return;
        }
        if self.selection_dragged {
            self.selection_dragged = false;
            if self.style.copy_on_select {
                self.copy_selection(cx);
            }
        }
    }

    /// Extends the selection while the left button is held, when no
    /// mouse-aware program has claimed the mouse (or Shift bypasses it).
    pub(super) fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window,
                                cx: &mut Context<Self>) {
        if !event.dragging() {
            return;
        }
        let Some((column, row)) = self.cell_at(event.position)
        else {
            return;
        };
        let bypass_mouse = event.modifiers.shift;
        let selected = self.terminal.with_grid_mut(|grid| {
                                        let selecting = bypass_mouse || !grid.sgr_mouse_mode();
                                        if selecting {
                                            grid.update_selection(column, row);
                                        }
                                        selecting
                                    });
        if selected {
            self.selection_dragged = true;
            cx.notify();
        }
    }

    /// Handles files dragged from Finder onto the terminal surface.
    pub(super) fn on_drop_paths(&mut self, paths: &ExternalPaths, _window: &mut Window,
                                cx: &mut Context<Self>) {
        let files = paths.paths().to_vec();
        if !files.is_empty() {
            cx.emit(TerminalEvent::FilesDropped(files));
        }
    }

    /// A mouse-aware program gets one wheel report per scroll event, in the
    /// direction scrolled. On the alternate screen with ALTERNATE_SCROLL enabled
    /// and mouse reporting off, wheel ticks translate into Up/Down arrow keys.
    /// Otherwise the wheel scrolls the view through the scrollback.
    pub(super) fn on_scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window,
                            cx: &mut Context<Self>) {
        let Some(metrics) = self.metrics
        else {
            return;
        };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => metrics.pixels_to_lines(delta.y),
        };
        if lines == 0. {
            return;
        }
        if self.mouse_reporting() {
            let button = if lines > 0. {
                MouseButton::WheelUp
            }
            else {
                MouseButton::WheelDown
            };
            self.mouse_button(event.position, button, true, cx);
            return;
        }

        let rows = self.whole_lines(lines);
        if rows == 0 {
            return;
        }

        let (alt_screen, alt_scroll, app_cursor) = self.terminal.with_grid(|grid| {
                                                       (grid.alt_screen(),
                                                        grid.alternate_scroll(),
                                                        grid.key_modes().app_cursor)
                                                   });
        if alt_screen && alt_scroll && !self.exited() {
            let seq: &[u8] = if rows > 0 {
                if app_cursor { b"\x1bOA" } else { b"\x1b[A" }
            }
            else if app_cursor {
                b"\x1bOB"
            }
            else {
                b"\x1b[B"
            };
            let count = rows.unsigned_abs().min(8) as usize;
            let mut payload = Vec::with_capacity(seq.len() * count);
            for _ in 0..count {
                payload.extend_from_slice(seq);
            }
            let written = self.terminal.write(&payload);
            self.report(written, cx);
            return;
        }

        if self.terminal
               .with_grid_mut(|grid| grid.scroll_display(rows))
        {
            cx.notify();
        }
    }

    fn whole_lines(&mut self, lines: f32) -> i32 {
        if self.scroll_remainder != 0. && self.scroll_remainder.signum() != lines.signum() {
            self.scroll_remainder = 0.;
        }
        self.scroll_remainder += lines;
        let whole = self.scroll_remainder.trunc();
        self.scroll_remainder -= whole;
        whole as i32
    }

    fn mouse_button(&mut self, position: Point<Pixels>, button: MouseButton, pressed: bool,
                    cx: &mut Context<Self>) {
        let Some((column, row)) = self.cell_at(position)
        else {
            return;
        };
        let sgr = self.mouse_reporting();
        if !sgr {
            if button == MouseButton::Left && pressed {
                self.terminal
                    .with_grid_mut(|grid| grid.start_selection(column, row));
                cx.notify();
            }
            return;
        }
        let input = MouseInput { row,
                                 column,
                                 button,
                                 pressed };
        if let Some(bytes) = mouse_to_bytes(input, sgr) {
            let written = self.terminal.write(&bytes);
            self.report(written, cx);
        }
    }

    fn mouse_reporting(&self) -> bool {
        self.terminal.with_grid(|grid| grid.sgr_mouse_mode()) && !self.exited()
    }

    fn exited(&self) -> bool {
        self.terminal.exit_report().is_some()
    }

    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        let metrics = self.metrics?;
        Some(metrics.cell_at(position - self.origin, self.terminal.size()))
    }
}

/// A clipboard chord the view handles itself rather than sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Clipboard {
    Copy,
    Paste,
}

pub(super) fn clipboard_chord(keystroke: &Keystroke, macos: bool) -> Option<Clipboard> {
    let modifiers = &keystroke.modifiers;
    let chord =
        modifiers.platform || (!macos && modifiers.control && modifiers.shift && !modifiers.alt);
    if !chord {
        return None;
    }
    match keystroke.key.to_ascii_lowercase().as_str() {
        "c" => Some(Clipboard::Copy),
        "v" => Some(Clipboard::Paste),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
