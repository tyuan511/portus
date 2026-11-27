//! A terminal emulator for GPUI over any byte transport.

mod consts;
mod error;
mod event;
mod grid;
mod keys;
mod mouse;
mod osc;
mod paste;
#[cfg(feature = "pty")]
mod pty;
mod sink;
mod terminal;
mod transport;
mod view;

pub use consts::DEFAULT_GRID_SIZE;
pub use error::{Error, Result};
pub use event::TerminalEvent;
pub use grid::{Grid, GridSize, SearchMatch};
pub use keys::{KeyInput, KeyModes, key_to_bytes, key_to_bytes_with_modes};
pub use mouse::{MouseButton, MouseInput, mouse_to_bytes};
pub use osc::{OscEvent, OscScanner, ShellMark, TerminalProgress};
pub use paste::paste_payload;
#[cfg(feature = "pty")]
pub use pty::{PtyCommand, PtyTransport};
pub use sink::{ExitHook, OutputHook, TerminalSink};
pub use terminal::{Terminal, TerminalBuilder};
pub use transport::{ExitReport, Transport};
pub use view::{TerminalCursorStyle, TerminalPalette, TerminalStyle, TerminalView};
