//! A headless terminal per session. It keeps the screen and scrollback so a
//! client that attaches late can draw what it missed.

use std::{
    fmt::Write as _,
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Arc, Mutex},
};

use alacritty_terminal::{
    event::{Event as TermEvent, EventListener},
    grid::Dimensions,
    index::{Column, Line},
    term::{
        cell::{Cell, Flags},
        test::TermSize,
        Config, Term, TermMode,
    },
    vte::ansi::{Color, NamedColor, Processor},
};

/// Lines of history the daemon keeps per session.
pub const SCROLLBACK: usize = 2000;

/// What one `feed` saw besides screen changes.
#[derive(Default, Debug, PartialEq)]
pub struct Signals {
    /// A real BEL, not the terminator of an OSC sequence.
    pub bell: bool,
    /// `Some(None)` when the program reset its title.
    pub title: Option<Option<String>>,
}

#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Signals>>);

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        let mut s = self.0.lock().unwrap();
        match event {
            TermEvent::Bell => s.bell = true,
            TermEvent::Title(t) => s.title = Some(Some(t)),
            TermEvent::ResetTitle => s.title = Some(None),
            _ => {}
        }
    }
}

pub struct Screen {
    term: Term<Listener>,
    parser: Processor,
    signals: Listener,
    /// Hash of the text at the last `text_changed`.
    text: u64,
}

impl Screen {
    pub fn new(cols: u16, rows: u16) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        let signals = Listener::default();
        Self {
            term: Term::new(config, &size(cols, rows), signals.clone()),
            parser: Processor::new(),
            signals,
            text: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Signals {
        self.parser.advance(&mut self.term, bytes);
        std::mem::take(&mut *self.signals.0.lock().unwrap())
    }

    /// True when the characters on screen or the scrollback changed since the
    /// last call. Colors and cursor moves do not count, so an idle program
    /// that animates a logo's colors is not at work.
    pub fn text_changed(&mut self) -> bool {
        let grid = self.term.grid();
        let mut h = DefaultHasher::new();
        grid.history_size().hash(&mut h);
        for line in 0..grid.screen_lines() as i32 {
            let row = &grid[Line(line)];
            for c in 0..grid.columns() {
                row[Column(c)].c.hash(&mut h);
            }
        }
        let text = h.finish();
        let changed = text != self.text;
        self.text = text;
        changed
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(size(cols, rows));
    }

    /// Bytes that redraw scrollback, screen, cursor, and input modes on a
    /// terminal of the same size. Starts with a full reset.
    pub fn snapshot(&self) -> Vec<u8> {
        let grid = self.term.grid();
        let mode = *self.term.mode();
        let cols = grid.columns();
        let mut out = String::with_capacity(64 * 1024);
        out.push_str("\x1bc");
        if mode.contains(TermMode::ALT_SCREEN) {
            out.push_str("\x1b[?1049h\x1b[H");
        }

        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let mut pen = Pen::default();
        for line in top..=bottom {
            let row = &grid[Line(line)];
            let wrapped = row[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
            // A wrapped row must fill every column so the next row wraps onto it.
            let end = if wrapped {
                cols
            } else {
                (0..cols)
                    .rev()
                    .find(|&c| !is_blank(&row[Column(c)]))
                    .map_or(0, |c| c + 1)
            };
            for c in 0..end {
                let cell = &row[Column(c)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                pen.set(&mut out, cell);
                out.push(if cell.flags.contains(Flags::HIDDEN) { ' ' } else { cell.c });
                if let Some(zw) = cell.zerowidth() {
                    out.extend(zw);
                }
            }
            if !wrapped && line != bottom {
                pen.reset(&mut out);
                out.push_str("\r\n");
            }
        }
        pen.reset(&mut out);

        let cursor = grid.cursor.point;
        let _ = write!(out, "\x1b[{};{}H", cursor.line.0 + 1, cursor.column.0 + 1);
        for (flag, set, unset) in [
            (TermMode::APP_CURSOR, "\x1b[?1h", ""),
            (TermMode::APP_KEYPAD, "\x1b=", ""),
            (TermMode::BRACKETED_PASTE, "\x1b[?2004h", ""),
            (TermMode::MOUSE_REPORT_CLICK, "\x1b[?1000h", ""),
            (TermMode::MOUSE_DRAG, "\x1b[?1002h", ""),
            (TermMode::MOUSE_MOTION, "\x1b[?1003h", ""),
            (TermMode::SGR_MOUSE, "\x1b[?1006h", ""),
            (TermMode::UTF8_MOUSE, "\x1b[?1005h", ""),
            (TermMode::FOCUS_IN_OUT, "\x1b[?1004h", ""),
            (TermMode::LINE_WRAP, "", "\x1b[?7l"),
            (TermMode::SHOW_CURSOR, "", "\x1b[?25l"),
        ] {
            out.push_str(if mode.contains(flag) { set } else { unset });
        }
        out.into_bytes()
    }
}

fn size(cols: u16, rows: u16) -> TermSize {
    TermSize::new(cols.max(1) as usize, rows.max(1) as usize)
}

fn is_blank(cell: &Cell) -> bool {
    cell.c == ' '
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell.flags.intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
        && cell.zerowidth().is_none()
}

/// The SGR state last written, so each cell only emits what changed.
/// `None` means the default attributes.
#[derive(Default)]
struct Pen {
    attrs: Option<(Color, Color, Flags)>,
}

const SGR_FLAGS: Flags = Flags::BOLD
    .union(Flags::DIM)
    .union(Flags::ITALIC)
    .union(Flags::ALL_UNDERLINES)
    .union(Flags::INVERSE)
    .union(Flags::STRIKEOUT);

impl Pen {
    fn reset(&mut self, out: &mut String) {
        if self.attrs.take().is_some() {
            out.push_str("\x1b[0m");
        }
    }

    fn set(&mut self, out: &mut String, cell: &Cell) {
        let want = (cell.fg, cell.bg, cell.flags & SGR_FLAGS);
        let default = (
            Color::Named(NamedColor::Foreground),
            Color::Named(NamedColor::Background),
            Flags::empty(),
        );
        if want == default {
            self.reset(out);
            return;
        }
        if self.attrs == Some(want) {
            return;
        }
        self.attrs = Some(want);
        out.push_str("\x1b[0");
        let f = want.2;
        for (flag, code) in [
            (Flags::BOLD, ";1"),
            (Flags::DIM, ";2"),
            (Flags::ITALIC, ";3"),
            (Flags::INVERSE, ";7"),
            (Flags::STRIKEOUT, ";9"),
        ] {
            if f.contains(flag) {
                out.push_str(code);
            }
        }
        if f.contains(Flags::UNDERCURL) {
            out.push_str(";4:3");
        } else if f.contains(Flags::DOUBLE_UNDERLINE) {
            out.push_str(";21");
        } else if f.contains(Flags::DOTTED_UNDERLINE) {
            out.push_str(";4:4");
        } else if f.contains(Flags::DASHED_UNDERLINE) {
            out.push_str(";4:5");
        } else if f.contains(Flags::UNDERLINE) {
            out.push_str(";4");
        }
        color(out, want.0, 30);
        color(out, want.1, 40);
        out.push('m');
    }
}

/// `base` is 30 for foreground, 40 for background.
fn color(out: &mut String, c: Color, base: u8) {
    match c {
        Color::Spec(rgb) => {
            let _ = write!(out, ";{};2;{};{};{}", base + 8, rgb.r, rgb.g, rgb.b);
        }
        Color::Indexed(i) => {
            let _ = write!(out, ";{};5;{}", base + 8, i);
        }
        Color::Named(n) => {
            let n = match n {
                NamedColor::Foreground
                | NamedColor::Background
                | NamedColor::Cursor
                | NamedColor::BrightForeground
                | NamedColor::DimForeground => return,
                // Dim variants map back to their base color; DIM is a flag.
                n if (n as usize) > 15 => n.to_bright() as usize,
                n => n as usize,
            };
            let code = if n < 8 { base as usize + n } else { base as usize + 60 + n - 8 };
            let _ = write!(out, ";{code}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_only_redraw_is_not_a_text_change() {
        let mut s = Screen::new(20, 4);
        s.feed(b"\x1b[1;1H\x1b[38;2;1;2;3mlogo");
        assert!(s.text_changed());
        s.feed(b"\x1b[1;1H\x1b[38;2;9;9;9mlogo");
        assert!(!s.text_changed());
        s.feed(b"\x1b[1;1Hlogs");
        assert!(s.text_changed());
    }

    #[test]
    fn title_bel_is_not_a_bell() {
        let mut s = Screen::new(20, 4);
        let sig = s.feed(b"\x1b]0;fix worktrees\x07$ ");
        assert!(!sig.bell);
        assert_eq!(sig.title, Some(Some("fix worktrees".into())));
        assert!(s.feed(b"\x07").bell);
    }

    #[test]
    fn snapshot_redraws_screen() {
        let mut s = Screen::new(20, 4);
        s.feed(b"one\r\n\x1b[31mred\x1b[0m\r\nthree\x1b[?2004h");
        let snap = String::from_utf8(s.snapshot()).unwrap();
        assert!(snap.starts_with("\x1bc"));
        assert!(snap.contains("one\r\n"));
        assert!(snap.contains("\x1b[0;31mred"));
        assert!(snap.contains("three"));
        assert!(snap.contains("\x1b[3;6H"), "cursor after 'three': {snap:?}");
        assert!(snap.contains("\x1b[?2004h"));
    }
}
