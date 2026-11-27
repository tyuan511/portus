//! A grid joined to a transport.

use std::sync::Arc;

use alacritty_terminal::event::Event;
use alacritty_terminal::term::ClipboardType;
use parking_lot::Mutex;

use crate::consts::DEFAULT_GRID_SIZE;
use crate::osc::OscEvent;
use crate::sink::{ExitHook, OutputHook};
use crate::{
    ExitReport, Grid, GridSize, Result, SearchMatch, TerminalEvent, TerminalPalette, TerminalSink,
    Transport,
};

/// Configures a [`Terminal`] before its transport starts producing output.
pub struct TerminalBuilder {
    size:       GridSize,
    scrollback: Option<usize>,
    on_output:  Option<OutputHook>,
    on_exit:    Option<ExitHook>,
}

impl Default for TerminalBuilder {
    fn default() -> Self {
        Self { size:       DEFAULT_GRID_SIZE,
               scrollback: None,
               on_output:  None,
               on_exit:    None, }
    }
}

impl TerminalBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The size to start at. The view resizes to fit its pane on first
    /// layout; this is what the far end sees until then.
    pub fn size(mut self, size: GridSize) -> Self {
        self.size = size;
        self
    }

    /// Maximum number of scrollback lines retained above the visible screen.
    pub fn scrollback(mut self, lines: usize) -> Self {
        self.scrollback = Some(lines);
        self
    }

    /// Watches the raw output stream - for activity, or for text a host
    /// wants to find (a URL a narrow grid would break across rows). Runs on
    /// the transport's thread, before the grid parses the chunk.
    pub fn on_output(mut self, hook: impl Fn(&[u8]) + Send + Sync + 'static) -> Self {
        self.on_output = Some(Box::new(hook));
        self
    }

    /// Learns of the exit on the transport's thread, without waiting for a
    /// view to pump it. [`TerminalEvent::Exited`] still follows on the view.
    pub fn on_exit(mut self, hook: impl Fn(ExitReport) + Send + Sync + 'static) -> Self {
        self.on_exit = Some(Box::new(hook));
        self
    }

    /// Builds the transport with `connect`, handing it the sink it reports
    /// through, and joins the two.
    pub fn connect<T, E>(self, connect: impl FnOnce(TerminalSink) -> std::result::Result<T, E>)
                         -> std::result::Result<Terminal<T>, E>
        where T: Transport {
        let grid = match self.scrollback {
            Some(lines) => Grid::with_scrollback(self.size, lines),
            None => Grid::new(self.size),
        };
        let grid = Arc::new(Mutex::new(grid));
        let (wake_tx, wake_rx) = async_channel::bounded(1);
        let sink = TerminalSink::new(grid, self.on_output, self.on_exit, wake_tx);
        let transport = connect(sink.clone())?;
        Ok(Terminal { transport: Arc::new(Mutex::new(transport)),
                      sink,
                      wake: wake_rx })
    }
}

/// A terminal: its grid and the transport behind it.
pub struct Terminal<T> {
    transport: Arc<Mutex<T>>,
    sink:      TerminalSink,
    wake:      async_channel::Receiver<()>,
}

impl<T> Clone for Terminal<T> {
    fn clone(&self) -> Self {
        Self { transport: Arc::clone(&self.transport),
               sink:      self.sink.clone(),
               wake:      self.wake.clone(), }
    }
}

impl<T: Transport> Terminal<T> {
    /// Sends input to the far end.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.transport.lock().write(bytes)
    }

    /// Resizes the grid and tells the far end.
    pub fn resize(&self, size: GridSize) -> Result<()> {
        self.sink.grid().lock().resize(size);
        self.transport.lock().resize(size)
    }

    /// Updates the grid's maximum scrollback history.
    pub fn set_scrollback(&self, lines: usize) {
        self.sink.grid().lock().set_scrollback(lines);
    }

    /// Updates the palette used to answer OSC 10/11/12/4 color queries.
    pub fn set_palette(&self, palette: TerminalPalette) {
        self.sink.grid().lock().set_palette(palette);
    }

    /// Updates measured cell pixel dimensions for XTWINOPS size queries.
    pub fn set_cell_size_px(&self, width: u16, height: u16) {
        self.sink.grid().lock().set_cell_size_px(width, height);
    }

    /// Searches across the entire scrollback + live buffer.
    pub fn search_all(&self, query: &str) -> Vec<SearchMatch> {
        self.sink.grid().lock().search_all(query)
    }

    /// Updates active search highlights on the grid.
    pub fn set_search_highlights(&self, matches: Vec<SearchMatch>, active: Option<usize>) {
        self.sink.grid().lock().set_search_highlights(matches, active);
    }

    /// Clears active search highlights on the grid.
    pub fn clear_search_highlights(&self) {
        self.sink.grid().lock().clear_search_highlights();
    }

    /// Scrolls the viewport to bring `line` into view.
    pub fn scroll_to_line(&self, line: i32) -> bool {
        self.sink.grid().lock().scroll_to_line(line)
    }

    /// Jumps to the previous or next OSC 133 prompt start.
    pub fn jump_to_prompt(&self, previous: bool) -> bool {
        self.sink.grid().lock().jump_to_prompt(previous)
    }

    /// Ends the far end.
    pub fn terminate(&self) -> Result<()> {
        self.transport.lock().terminate()
    }

    /// See [`Transport::process_id`].
    pub fn process_id(&self) -> Option<u32> {
        self.transport.lock().process_id()
    }

    pub fn size(&self) -> GridSize {
        self.sink.size()
    }

    /// Reads the grid under its lock. Keep `read` short: the transport's
    /// reader waits on the same lock to parse output.
    pub fn with_grid<R>(&self, read: impl FnOnce(&Grid) -> R) -> R {
        read(&self.sink.grid().lock())
    }

    /// How the far end ended, once it has.
    pub fn exit_report(&self) -> Option<ExitReport> {
        self.sink.exit_report()
    }

    pub(crate) fn with_grid_mut<R>(&self, change: impl FnOnce(&mut Grid) -> R) -> R {
        change(&mut self.sink.grid().lock())
    }

    pub(crate) fn sync_update_active(&self) -> bool {
        self.sink.grid().lock().sync_update_active()
    }

    pub(crate) fn take_dirty(&self) -> bool {
        self.sink.grid().lock().take_dirty()
    }

    pub(crate) fn wake(&self) -> async_channel::Receiver<()> {
        self.wake.clone()
    }

    /// The program's requests since the last call, as events for the host.
    ///
    /// Replies the program expects on its input (cursor position report,
    /// device attributes, OSC 10/11 color queries, text area size queries) are
    /// written back here immediately so modern AI CLIs never stall waiting.
    pub(crate) fn drain_events(&self) -> Vec<TerminalEvent> {
        let (raised, osc_events, replies) = {
            let mut grid = self.sink.grid().lock();
            let raised = grid.drain_events();
            let osc_events = grid.drain_osc_events();
            let mut replies: Vec<String> = Vec::new();
            let mut forwarded = Vec::new();
            for event in raised {
                match event {
                    Event::ColorRequest(index, formatter) => {
                        let rgb = grid.color_for_request(index);
                        replies.push(formatter(rgb));
                    }
                    Event::TextAreaSizeRequest(formatter) => {
                        let size = grid.window_size();
                        replies.push(formatter(size));
                    }
                    Event::PtyWrite(reply) => {
                        replies.push(reply);
                    }
                    other => forwarded.push(other),
                }
            }
            (forwarded, osc_events, replies)
        };

        let mut events = Vec::new();
        for reply in replies {
            if let Err(error) = self.write(reply.as_bytes()) {
                events.push(TerminalEvent::TransportFailed(Arc::new(error)));
            }
        }
        for event in raised {
            match event {
                Event::Title(title) => events.push(TerminalEvent::Title(title)),
                Event::ResetTitle => events.push(TerminalEvent::ResetTitle),
                Event::Bell => events.push(TerminalEvent::Bell),
                Event::ClipboardStore(ClipboardType::Clipboard, text) => {
                    events.push(TerminalEvent::ClipboardStore(text));
                }
                _ => {}
            }
        }
        for osc in osc_events {
            match osc {
                OscEvent::Cwd(cwd) => events.push(TerminalEvent::CwdChanged(cwd)),
                OscEvent::Progress(progress) => events.push(TerminalEvent::Progress(progress)),
                OscEvent::Notification { title, body } => {
                    events.push(TerminalEvent::Notification { title, body });
                }
                OscEvent::ShellMark(mark) => events.push(TerminalEvent::ShellMark(mark)),
            }
        }
        events
    }
}

#[cfg(test)]
mod tests;
