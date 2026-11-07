//! A headless terminal per session. It keeps the screen and scrollback so a
//! client that attaches late can draw what it missed.

/// Lines of history the daemon keeps per session.
pub const SCROLLBACK: usize = 2000;

mod inspect;
mod signals;
mod snapshot;
mod terminal;

pub use skiff_core::screen::{has_busy_line, title_state, TitleState};
pub use inspect::PromptMatch;
pub use signals::{Signals, StatusSignal};
pub use terminal::Screen;
