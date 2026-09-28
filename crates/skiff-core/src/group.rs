//! Named layouts of sessions. The daemon keeps them in memory so every
//! client sees the same splits.

use serde::{Deserialize, Serialize};

use crate::session::SessionId;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplitDir {
    /// `a` left, `b` right.
    Row,
    /// `a` top, `b` bottom.
    Col,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Layout {
    Pane {
        session: SessionId,
    },
    Split {
        dir: SplitDir,
        /// Share of `a`, 0.1 to 0.9.
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

impl Layout {
    /// Every session, left to right, top to bottom.
    pub fn sessions(&self) -> Vec<SessionId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<SessionId>) {
        match self {
            Layout::Pane { session } => out.push(session.clone()),
            Layout::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    /// The layout without `session`. A split that loses one side becomes
    /// the other side. `None` when nothing is left.
    pub fn without(&self, session: &str) -> Option<Layout> {
        match self {
            Layout::Pane { session: s } if s == session => None,
            Layout::Pane { .. } => Some(self.clone()),
            Layout::Split { dir, ratio, a, b } => match (a.without(session), b.without(session)) {
                (Some(a), Some(b)) => Some(Layout::Split {
                    dir: *dir,
                    ratio: *ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(one), None) | (None, Some(one)) => Some(one),
                (None, None) => None,
            },
        }
    }

    /// Keeps every ratio in 0.1..=0.9.
    pub fn clamp_ratios(&mut self) {
        if let Layout::Split { ratio, a, b, .. } = self {
            *ratio = if ratio.is_finite() { ratio.clamp(0.1, 0.9) } else { 0.5 };
            a.clamp_ratios();
            b.clamp_ratios();
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Group {
    /// Empty on create; the daemon assigns one.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub layout: Layout,
    #[serde(default)]
    pub focus: Option<SessionId>,
    /// Where the group's empty panes start their sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// An empty pane in a layout: `slot:` and a number. It waits for the
/// operator to pick an agent and holds no session.
pub fn is_slot(id: &str) -> bool {
    id.starts_with("slot:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(s: &str) -> Layout {
        Layout::Pane { session: s.into() }
    }

    fn split(dir: SplitDir, a: Layout, b: Layout) -> Layout {
        Layout::Split {
            dir,
            ratio: 0.5,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    #[test]
    fn without_collapses_splits() {
        let l = split(SplitDir::Row, pane("x"), split(SplitDir::Col, pane("y"), pane("z")));
        assert_eq!(l.sessions(), ["x", "y", "z"]);
        assert_eq!(
            l.without("y").unwrap(),
            split(SplitDir::Row, pane("x"), pane("z"))
        );
        assert_eq!(l.without("x").unwrap(), split(SplitDir::Col, pane("y"), pane("z")));
        assert_eq!(pane("x").without("x"), None);
        assert_eq!(l.without("nope").unwrap(), l);
    }

    #[test]
    fn layout_json_matches_ts_type() {
        let l = split(SplitDir::Col, pane("a1"), pane("b2"));
        let v = serde_json::to_value(&l).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "type": "split", "dir": "col", "ratio": 0.5,
                "a": {"type": "pane", "session": "a1"},
                "b": {"type": "pane", "session": "b2"},
            })
        );
        let g: Group = serde_json::from_value(serde_json::json!({
            "id": "", "name": "g", "focus": null,
            "layout": {"type": "pane", "session": "s"},
        }))
        .unwrap();
        assert_eq!(g.layout, pane("s"));
    }
}
