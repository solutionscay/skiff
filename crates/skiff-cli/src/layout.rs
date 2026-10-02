//! Layout tree edits, as the app's `workspace/layout.ts` makes them.

use skiff_core::group::{is_slot, Layout, SplitDir};

/// The app's pane limit for one group.
pub const MAX_PANES: usize = 4;

/// The template names the app's + menu uses.
pub const PRESETS: [&str; 6] = [
    "2 side by side",
    "2 stacked",
    "1 left, 2 right",
    "1 over 2",
    "3 side by side",
    "2 by 2",
];

/// Where a new pane goes next to its target.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Side {
    Right,
    Down,
    Left,
    Up,
}

pub fn pane(id: &str) -> Layout {
    Layout::Pane { session: id.to_string() }
}

fn split(dir: SplitDir, ratio: f32, a: Layout, b: Layout) -> Layout {
    Layout::Split {
        dir,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

/// An empty pane: `slot:` and 8 hex digits, as skiffd makes them.
pub fn slot() -> Layout {
    let id = uuid::Uuid::new_v4().simple().to_string();
    pane(&format!("slot:{}", &id[..8]))
}

/// The app's default shapes. 2: side by side. 3: one left, two stacked
/// right. 4+: top half over bottom half.
pub fn build(ids: &[String]) -> Layout {
    match ids {
        [one] => pane(one),
        [a, b] => split(SplitDir::Row, 0.5, pane(a), pane(b)),
        [a, b, c] => split(SplitDir::Row, 0.5, pane(a), split(SplitDir::Col, 0.5, pane(b), pane(c))),
        _ => {
            let half = ids.len().div_ceil(2);
            split(SplitDir::Col, 0.5, build(&ids[..half]), build(&ids[half..]))
        }
    }
}

/// Every pane in one line along `dir`, all the same size.
pub fn chain(ids: &[String], dir: SplitDir) -> Layout {
    match ids {
        [one] => pane(one),
        [first, rest @ ..] => split(dir, 1.0 / ids.len() as f32, pane(first), chain(rest, dir)),
        [] => slot(),
    }
}

/// Swaps the pane `target` for what `f` makes of it. False when `target`
/// is not in the tree.
fn replace(l: &mut Layout, target: &str, f: &mut dyn FnMut(Layout) -> Layout) -> bool {
    match l {
        Layout::Pane { session } if session == target => {
            let old = std::mem::replace(l, pane(""));
            *l = f(old);
            true
        }
        Layout::Pane { .. } => false,
        Layout::Split { a, b, .. } => replace(a, target, f) || replace(b, target, f),
    }
}

/// Puts `session` beside `target`, half and half. False when `target` is
/// not in the tree.
pub fn split_pane(l: &mut Layout, target: &str, side: Side, session: &str) -> bool {
    let dir = match side {
        Side::Left | Side::Right => SplitDir::Row,
        Side::Up | Side::Down => SplitDir::Col,
    };
    let before = matches!(side, Side::Left | Side::Up);
    replace(l, target, &mut |p| {
        if before {
            split(dir, 0.5, pane(session), p)
        } else {
            split(dir, 0.5, p, pane(session))
        }
    })
}

/// Puts `session` where the pane `target` is.
pub fn fill(l: &mut Layout, target: &str, session: &str) -> bool {
    replace(l, target, &mut |_| pane(session))
}

/// The layout without `session`. An empty layout becomes one slot.
pub fn remove(l: &Layout, session: &str) -> Layout {
    l.without(session).unwrap_or_else(slot)
}

pub fn filled(l: &Layout) -> Vec<String> {
    l.sessions().into_iter().filter(|s| !is_slot(s)).collect()
}

/// The template a layout's shape matches; `None` for any other shape.
pub fn shape_name(l: &Layout) -> Option<&'static str> {
    let Layout::Split { dir, a, b, .. } = l else { return None };
    let sub = |x: &Layout| match x {
        Layout::Split { dir, .. } => Some(*dir),
        Layout::Pane { .. } => None,
    };
    let n = l.sessions().len();
    let row = SplitDir::Row;
    let col = SplitDir::Col;
    match (n, *dir, sub(a), sub(b)) {
        (2, SplitDir::Row, ..) => Some("2 side by side"),
        (2, SplitDir::Col, ..) => Some("2 stacked"),
        (3, d, None, Some(bd)) if d == row && bd == col => Some("1 left, 2 right"),
        (3, d, None, Some(bd)) if d == row && bd == row => Some("3 side by side"),
        (3, d, None, Some(bd)) if d == col && bd == row => Some("1 over 2"),
        (4, d, Some(ad), Some(bd)) if d == col && ad == row && bd == row => Some("2 by 2"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_puts_the_new_pane_on_its_side() {
        let mut l = build(&ids(&["x", "y"]));
        assert!(split_pane(&mut l, "y", Side::Up, "z"));
        assert_eq!(l, split(SplitDir::Row, 0.5, pane("x"), split(SplitDir::Col, 0.5, pane("z"), pane("y"))));
        assert_eq!(l.sessions(), ["x", "z", "y"]);
        assert!(!split_pane(&mut l, "nope", Side::Right, "w"));
        assert_eq!(shape_name(&build(&ids(&["a", "b", "c"]))), Some("1 left, 2 right"));
        assert_eq!(shape_name(&build(&ids(&["a", "b", "c", "d"]))), Some("2 by 2"));
    }

    #[test]
    fn remove_collapses_and_leaves_a_slot() {
        let l = build(&ids(&["x", "y", "z"]));
        assert_eq!(remove(&l, "y"), split(SplitDir::Row, 0.5, pane("x"), pane("z")));
        let empty = remove(&pane("x"), "x");
        assert!(is_slot(&empty.sessions()[0]));
        assert_eq!(empty.sessions()[0].len(), "slot:".len() + 8);
    }
}
