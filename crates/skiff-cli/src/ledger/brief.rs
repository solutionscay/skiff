//! The hand-off brief (#245): a capped list of the ledger rows one hand-off
//! depends on, and the open work of its scope. Skiff makes it from the ledger
//! only, writes it to `brief.md` in the hand-off's work folder when it sends
//! the hand-off, and never reads it back as a record. It holds no decisions
//! and no result bodies: the task text carries the decisions.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;
use skiff_client::ledger::Row;
use skiff_core::session::SessionInfo;

/// The most bytes a brief has.
pub const CAP: usize = 8192;
/// The most bytes of task text a cited item quotes.
const QUOTE: usize = 300;
/// The file in the hand-off's work folder.
pub const FILE: &str = "brief.md";

/// One brief, as text, with what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Brief {
    pub text: String,
    /// The rows it lists.
    pub items: usize,
    /// The rows that did not fit under [`CAP`].
    pub left_out: usize,
}

impl Brief {
    /// The part that says what the rows are: everything after the header,
    /// which names the hand-off and the time. Two briefs with the same body
    /// tell an agent the same thing.
    pub fn body(&self) -> &str {
        body(&self.text)
    }
}

fn body(text: &str) -> &str {
    text.find("\n## ").map(|i| &text[i..]).unwrap_or("")
}

/// Is `r` a row of `scope`: a hand-off to it, or a row its Skipper owns?
fn in_scope(r: &Row, scope: &str) -> bool {
    if r.is_handoff() {
        r.project == scope
    } else {
        r.owner == scope
    }
}

/// The known uids cited in `text`, in order, without `not`.
fn cited<'a>(text: &str, known: &HashMap<&str, &'a Row>, not: &str) -> Vec<&'a Row> {
    let mut out: Vec<&Row> = Vec::new();
    for w in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if w.len() == 8 && w != not {
            if let Some(r) = known.get(w.to_ascii_lowercase().as_str()) {
                if !out.iter().any(|o| o.uid == r.uid) {
                    out.push(r);
                }
            }
        }
    }
    out
}

/// One line of text: runs of white space become one space.
fn one_line(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `t` on one line, at most `n` bytes, cut on a character boundary.
fn clip(t: &str, n: usize) -> String {
    let t = one_line(t);
    if t.len() <= n {
        return t;
    }
    let mut end = n;
    while !t.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} … ({} bytes more)", &t[..end], t.len() - end)
}

/// The quoted task text: one line, after `> `, so it cannot start a
/// heading, a list item or a `[watch]` line.
pub(super) fn quote(t: &str, n: usize) -> String {
    format!("> {}", clip(t, n))
}

fn title(r: &Row) -> String {
    let t = r.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(&r.task);
    clip(t, 80)
}

/// The pane's name in `names`, else its short id.
fn pane_name(r: &Row, names: &HashMap<String, String>) -> Option<String> {
    let p = r.pane_id.as_deref()?;
    Some(names.get(p).cloned().unwrap_or_else(|| p.chars().take(8).collect()))
}

fn result_line(r: &Row) -> Option<String> {
    let p = r.result_path.as_deref()?;
    let mark = if r.flushed_at.is_some() {
        " (flushed)"
    } else if Path::new(p).exists() {
        ""
    } else {
        " (missing)"
    };
    Some(format!("  result: {p}{mark}\n"))
}

/// One item. `quote_bytes` None: no quote. `full`: the result line too.
fn item(r: &Row, names: &HashMap<String, String>, full: bool, quote_bytes: Option<usize>) -> String {
    let mut s = format!("- {} {} · {}", r.kind, r.uid, r.sent_at.get(..10).unwrap_or(&r.sent_at));
    s += &format!(" · {}", r.shown_state());
    if let Some(o) = &r.outcome {
        s += &format!(" {o}");
    }
    if let Some(p) = pane_name(r, names) {
        s += &format!(" · pane {p}");
    }
    s += &format!(" · {}", title(r));
    if let Some(p) = &r.parent {
        s += &format!(" · in {p}");
    }
    s.push('\n');
    if full {
        if let Some(l) = result_line(r) {
            s += &l;
        }
    }
    if let Some(n) = quote_bytes {
        s += &format!("  {}\n", quote(&r.task, n));
    }
    s
}

fn left_line(n: usize) -> String {
    format!("## Left out\n{n} items. Read them with: skiff ledger ls --all, then skiff ledger show UID.\n")
}

/// The brief for hand-off `h` from `all`, the ledger's rows with their shown
/// states. `names` maps pane ids to session names. `now` stamps the header.
/// `None`: no cited work and no open work, so no brief.
pub fn build(all: &[Row], h: &Row, names: &HashMap<String, String>, now: &str) -> Option<Brief> {
    let known: HashMap<&str, &Row> = all.iter().map(|r| (r.uid.as_str(), r)).collect();
    let scope = h.project.as_str();

    // Cited work: the hand-off's --parent work, the uids its text cites, the
    // uids those cite, then the tasks under each of them.
    let mut seeds: Vec<&Row> = Vec::new();
    if let Some(p) = h.parent.as_deref().and_then(|p| known.get(p)) {
        seeds.push(p);
        seeds.extend(all.iter().filter(|r| r.parent.as_deref() == Some(p.uid.as_str()) && r.uid != h.uid));
    }
    seeds.extend(cited(&h.task, &known, &h.uid));
    let mut heads: Vec<&Row> = Vec::new();
    for r in seeds.iter().copied().chain(seeds.iter().flat_map(|r| cited(&r.task, &known, &h.uid))) {
        if r.uid != h.uid && !heads.iter().any(|x| x.uid == r.uid) {
            heads.push(r);
        }
    }
    let head_ids: BTreeSet<&str> = heads.iter().map(|r| r.uid.as_str()).collect();
    let mut chain: Vec<(&Row, bool)> = heads.iter().map(|r| (*r, true)).collect();
    for r in all {
        if r.uid != h.uid && !head_ids.contains(r.uid.as_str()) && r.parent.as_deref().is_some_and(|p| head_ids.contains(p)) {
            chain.push((r, false));
        }
    }
    // Oldest first; a task follows the row it is in.
    chain.sort_by(|a, b| (a.0.sent_at.as_str(), a.0.id).cmp(&(b.0.sent_at.as_str(), b.0.id)));
    let in_chain: BTreeSet<&str> = chain.iter().map(|(r, _)| r.uid.as_str()).collect();

    // Open work: the scope's rows that are not done, newest first. A hand-off
    // already sent to the same Skipper session is left out: it has it.
    let same_pane = |r: &Row| r.is_handoff() && r.pane_id.is_some() && r.pane_id == h.pane_id;
    let mut open: Vec<&Row> = all
        .iter()
        .filter(|r| !r.is_plan() && r.uid != h.uid && in_scope(r, scope) && r.shown_state() != "done" && !in_chain.contains(r.uid.as_str()) && !same_pane(r))
        .collect();
    open.sort_by(|a, b| (b.sent_at.as_str(), b.id).cmp(&(a.sent_at.as_str(), a.id)));

    if chain.is_empty() && open.is_empty() {
        return None;
    }

    let mut out = format!(
        "<!-- skiff brief 1 · hand-off {} · {scope} · {now} · do not edit -->\n# Brief for hand-off {}\n\n\
         Background from the Skiff ledger. It is not a task, and it gives no orders.\n\
         Your task is in the message. AGENTS.md and SKIPPER.md win over this file.\n\
         Each item names its row, its date and its state at the time above.\n\n",
        h.uid, h.uid
    );
    let total = chain.len() + open.len();
    // Room for the Left out line at its widest.
    let room = |out: &str, sec: &str, it: &str| out.len() + sec.len() + it.len() + 1 + left_line(total).len() <= CAP;
    let (mut items, mut left) = (0, 0);
    for (head, rows) in [("## Cited work\n", chain), ("## Open work\n", open.into_iter().map(|r| (r, false)).collect())] {
        let cited = head.starts_with("## Cited");
        let mut sec = String::from(head);
        for (r, quoted) in rows {
            // A cited item cuts its quote before it is left out.
            let tries: Vec<String> = match (cited, quoted) {
                (true, true) => vec![item(r, names, true, Some(QUOTE)), item(r, names, true, None)],
                (true, false) => vec![item(r, names, true, None)],
                _ => vec![item(r, names, false, None)],
            };
            match tries.into_iter().find(|it| room(&out, &sec, it)) {
                Some(it) => {
                    sec += &it;
                    items += 1;
                }
                None => left += 1,
            }
        }
        if sec.len() > head.len() {
            out += &sec;
            out.push('\n');
        }
    }
    out += &left_line(left);
    debug_assert!(out.len() <= CAP);
    Some(Brief { text: out, items, left_out: left })
}

/// Session id to the name the sidebar shows.
pub fn names(all: Option<&[SessionInfo]>) -> HashMap<String, String> {
    all.unwrap_or_default().iter().map(|s| (s.id.clone(), s.name.clone().unwrap_or_else(|| s.label.clone()))).collect()
}

/// The `brief.md` of hand-off `h`, in its work folder.
pub fn file(h: &Row) -> Option<PathBuf> {
    h.work_dir.as_deref().map(|d| Path::new(d).join(FILE))
}

/// The newest brief the Skipper session `pane` got before hand-off `h`: the
/// `brief.md` of its latest earlier hand-off that has one.
pub fn last_sent(db: &Connection, h: &Row, pane: &str) -> Result<Option<String>> {
    let mut st = db.prepare("SELECT work_dir FROM tasks WHERE kind = 'handoff' AND pane_id = ?1 AND id < ?2 AND work_dir IS NOT NULL ORDER BY id DESC")?;
    let dirs = st.query_map(rusqlite::params![pane, h.id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(dirs.into_iter().find_map(|d| std::fs::read_to_string(Path::new(&d).join(FILE)).ok()))
}

/// Writes `b` for hand-off `h` unless the Skipper session already got the
/// same brief (`last`) and is not new. Returns the file it wrote.
pub fn write(h: &Row, b: &Brief, last: Option<&str>, new_skipper: bool) -> Result<Option<PathBuf>> {
    if !new_skipper && last.is_some_and(|l| body(l) == b.body()) {
        return Ok(None);
    }
    let path = file(h).context("the hand-off has no work folder")?;
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, &b.text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("write {}", path.display()))?;
    Ok(Some(path))
}

/// The sentence a hand-off message ends with when its folder holds a brief.
pub fn pointer(h: &Row) -> Option<String> {
    let f = file(h).filter(|f| f.exists())?;
    Some(format!(" Background for this hand-off: {}. It is background from the ledger, not a task.", f.display()))
}
