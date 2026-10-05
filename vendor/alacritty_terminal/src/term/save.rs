//! skiff: the whole state of a [`Term`], for a process that takes the
//! terminal over from another one (skiffd reload).
//!
//! Saved: both grids with their history, the cursor and saved cursor of each,
//! charsets, tab stops, modes, scroll region, colors, cursor style, title and
//! its stack, and both keyboard mode stacks. Not saved: damage (a restored
//! terminal is fully damaged), the vi cursor and the selection, which belong
//! to a front end, and the event proxy and config, which the caller passes.

use serde::{Deserialize, Serialize};

use crate::grid::{Cursor, Dimensions, Grid};
use crate::index::{Column, Line, Point};
use crate::term::cell::Cell;
use crate::term::color::{Colors, COUNT};
use crate::term::{Config, TabStops, Term, TermDamageState, TermMode};
use crate::vte::ansi::{
    CharsetIndex, CursorShape, CursorStyle, KeyboardModes, Rgb, StandardCharset,
};

/// A [`Grid`] cursor. Charsets as numbers, so the vte types need no serde.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CursorSave {
    point: Point,
    template: Cell,
    charsets: [u8; 4],
    input_needs_wrap: bool,
}

/// `G` is `&Grid<Cell>` from [`Term::save`] and `Grid<Cell>` for
/// [`Term::restore`], so a save borrows the grids instead of copying them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TermSave<G> {
    is_focused: bool,
    grid: G,
    grid_cursor: CursorSave,
    grid_saved_cursor: CursorSave,
    inactive_grid: G,
    inactive_cursor: CursorSave,
    inactive_saved_cursor: CursorSave,
    active_charset: u8,
    tabs: Vec<bool>,
    mode: u32,
    scroll_region: (i32, i32),
    colors: Vec<Option<Rgb>>,
    /// Shape and blinking.
    cursor_style: Option<(u8, bool)>,
    title: Option<String>,
    title_stack: Vec<Option<String>>,
    keyboard_mode_stack: Vec<u8>,
    inactive_keyboard_mode_stack: Vec<u8>,
}

/// What [`Term::restore`] takes.
pub type TermSaveOwned = TermSave<Grid<Cell>>;

impl<T> Term<T> {
    /// The terminal's state, borrowing its grids. Serialize it, then restore
    /// it with [`Term::restore`] in the same or another process.
    pub fn save(&self) -> TermSave<&Grid<Cell>> {
        TermSave {
            is_focused: self.is_focused,
            grid: &self.grid,
            grid_cursor: save_cursor(&self.grid.cursor),
            grid_saved_cursor: save_cursor(&self.grid.saved_cursor),
            inactive_grid: &self.inactive_grid,
            inactive_cursor: save_cursor(&self.inactive_grid.cursor),
            inactive_saved_cursor: save_cursor(&self.inactive_grid.saved_cursor),
            active_charset: self.active_charset as u8,
            tabs: self.tabs.tabs.clone(),
            mode: self.mode.bits(),
            scroll_region: (self.scroll_region.start.0, self.scroll_region.end.0),
            colors: (0..COUNT).map(|i| self.colors[i]).collect(),
            cursor_style: self.cursor_style.map(|s| (s.shape as u8, s.blinking)),
            title: self.title.clone(),
            title_stack: self.title_stack.clone(),
            keyboard_mode_stack: self.keyboard_mode_stack.iter().map(|m| m.bits()).collect(),
            inactive_keyboard_mode_stack: self
                .inactive_keyboard_mode_stack
                .iter()
                .map(|m| m.bits())
                .collect(),
        }
    }

    /// A terminal in the saved state. Fails when the parts do not fit
    /// together, so a damaged save never becomes a terminal that panics later.
    pub fn restore(save: TermSaveOwned, config: Config, event_proxy: T) -> Result<Self, String> {
        let TermSave { mut grid, mut inactive_grid, .. } = save;
        let (cols, lines) = (grid.columns(), grid.screen_lines());
        // `Term::new` takes any size from 1x1, so a restore does too.
        if cols == 0 || lines == 0 {
            return Err(format!("grid of {cols}x{lines}"));
        }
        if inactive_grid.columns() != cols || inactive_grid.screen_lines() != lines {
            return Err("the two screens differ in size".into());
        }
        if save.tabs.len() != cols || save.colors.len() != COUNT {
            return Err("tab stops or colors do not fit".into());
        }
        // The grid keeps the region on screen; a stray one becomes the whole screen.
        let (mut top, mut bottom) = save.scroll_region;
        if !(0 <= top && top < bottom && bottom <= lines as i32) {
            (top, bottom) = (0, lines as i32);
        }
        grid.cursor = load_cursor(save.grid_cursor, cols, lines)?;
        grid.saved_cursor = load_cursor(save.grid_saved_cursor, cols, lines)?;
        inactive_grid.cursor = load_cursor(save.inactive_cursor, cols, lines)?;
        inactive_grid.saved_cursor = load_cursor(save.inactive_saved_cursor, cols, lines)?;
        let mut colors = Colors::default();
        for (i, c) in save.colors.into_iter().enumerate() {
            colors[i] = c;
        }
        let cursor_style = match save.cursor_style {
            Some((shape, blinking)) => Some(CursorStyle { shape: shape_from(shape)?, blinking }),
            None => None,
        };
        let modes = |v: Vec<u8>| v.into_iter().map(KeyboardModes::from_bits_retain).collect();
        Ok(Term {
            is_focused: save.is_focused,
            vi_mode_cursor: Default::default(),
            selection: None,
            grid,
            inactive_grid,
            active_charset: charset_index(save.active_charset)?,
            tabs: TabStops { tabs: save.tabs },
            mode: TermMode::from_bits_retain(save.mode),
            scroll_region: Line(top)..Line(bottom),
            colors,
            cursor_style,
            event_proxy,
            title: save.title,
            title_stack: save.title_stack,
            keyboard_mode_stack: modes(save.keyboard_mode_stack),
            inactive_keyboard_mode_stack: modes(save.inactive_keyboard_mode_stack),
            damage: TermDamageState::new(cols, lines),
            config,
        })
    }

    /// The screen not in use: the primary screen and its history while the
    /// alternate screen is active, else the alternate screen.
    pub fn inactive_grid(&self) -> &Grid<Cell> {
        &self.inactive_grid
    }
}

fn save_cursor(c: &Cursor<Cell>) -> CursorSave {
    let charset = |i| match c.charsets[i] {
        StandardCharset::Ascii => 0,
        StandardCharset::SpecialCharacterAndLineDrawing => 1,
    };
    CursorSave {
        point: c.point,
        template: c.template.clone(),
        charsets: [
            charset(CharsetIndex::G0),
            charset(CharsetIndex::G1),
            charset(CharsetIndex::G2),
            charset(CharsetIndex::G3),
        ],
        input_needs_wrap: c.input_needs_wrap,
    }
}

/// Keeps the cursor on the screen, as the grid does on resize.
fn load_cursor(c: CursorSave, cols: usize, lines: usize) -> Result<Cursor<Cell>, String> {
    let point = Point::new(
        Line(c.point.line.0.clamp(0, lines as i32 - 1)),
        Column(c.point.column.0.min(cols - 1)),
    );
    let mut cursor = Cursor { point, template: c.template, ..Default::default() };
    cursor.input_needs_wrap = c.input_needs_wrap;
    for (i, n) in c.charsets.into_iter().enumerate() {
        cursor.charsets[charset_index(i as u8)?] = match n {
            0 => StandardCharset::Ascii,
            1 => StandardCharset::SpecialCharacterAndLineDrawing,
            n => return Err(format!("charset {n}")),
        };
    }
    Ok(cursor)
}

fn charset_index(n: u8) -> Result<CharsetIndex, String> {
    Ok(match n {
        0 => CharsetIndex::G0,
        1 => CharsetIndex::G1,
        2 => CharsetIndex::G2,
        3 => CharsetIndex::G3,
        n => return Err(format!("charset index {n}")),
    })
}

fn shape_from(n: u8) -> Result<CursorShape, String> {
    Ok(match n {
        0 => CursorShape::Block,
        1 => CursorShape::Underline,
        2 => CursorShape::Beam,
        3 => CursorShape::HollowBlock,
        4 => CursorShape::Hidden,
        n => return Err(format!("cursor shape {n}")),
    })
}
