//! A line diff for `skiff skipper template diff`, with no external tool.

/// Above this many table cells, the diff shows the whole middle as replaced.
/// An instruction file is small: only a person's very large file reaches it.
const MAX_CELLS: usize = 16_000_000;

/// The lines of context around a change.
const CONTEXT: usize = 3;

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Same,
    Del,
    Add,
}

/// The steps from `a` to `b`, from a longest common subsequence.
fn steps(a: &[&str], b: &[&str]) -> Vec<Op> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (x, y) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let mut out = vec![Op::Same; head];
    if x.len().saturating_mul(y.len()) > MAX_CELLS {
        out.extend(std::iter::repeat_n(Op::Del, x.len()));
        out.extend(std::iter::repeat_n(Op::Add, y.len()));
    } else {
        // table[i][j]: the common length of x[i..] and y[j..].
        let width = y.len() + 1;
        let mut table = vec![0u32; (x.len() + 1) * width];
        for i in (0..x.len()).rev() {
            for j in (0..y.len()).rev() {
                table[i * width + j] = if x[i] == y[j] {
                    table[(i + 1) * width + j + 1] + 1
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < x.len() || j < y.len() {
            if i < x.len() && j < y.len() && x[i] == y[j] {
                out.push(Op::Same);
                i += 1;
                j += 1;
            } else if j == y.len() || (i < x.len() && table[(i + 1) * width + j] >= table[i * width + j + 1]) {
                out.push(Op::Del);
                i += 1;
            } else {
                out.push(Op::Add);
                j += 1;
            }
        }
    }
    out.extend(std::iter::repeat_n(Op::Same, tail));
    out
}

/// `old` to `new` as a unified diff, or an empty string when they are the same.
pub(super) fn unified(old: &str, new: &str, old_name: &str, new_name: &str) -> String {
    use std::fmt::Write;
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let ops = steps(&a, &b);
    let changes: Vec<usize> = ops.iter().enumerate().filter(|(_, o)| **o != Op::Same).map(|(i, _)| i).collect();
    let Some(&first) = changes.first() else { return String::new() };
    let mut out = format!("--- {old_name}\n+++ {new_name}\n");
    // The hunks: runs of changes with at most two contexts between them.
    let mut hunks = vec![(first, first)];
    for &c in &changes[1..] {
        let last = hunks.last_mut().expect("one hunk at least");
        if c - last.1 <= CONTEXT * 2 + 1 {
            last.1 = c;
        } else {
            hunks.push((c, c));
        }
    }
    // The line of each side at each step.
    let (mut ai, mut bi) = (Vec::with_capacity(ops.len() + 1), Vec::with_capacity(ops.len() + 1));
    let (mut i, mut j) = (0, 0);
    for op in &ops {
        ai.push(i);
        bi.push(j);
        i += usize::from(*op != Op::Add);
        j += usize::from(*op != Op::Del);
    }
    ai.push(i);
    bi.push(j);
    for (from, to) in hunks {
        let (start, end) = (from.saturating_sub(CONTEXT), (to + CONTEXT + 1).min(ops.len()));
        let _ = writeln!(out, "@@ -{},{} +{},{} @@", ai[start] + 1, ai[end] - ai[start], bi[start] + 1, bi[end] - bi[start]);
        for s in start..end {
            let (mark, line) = match ops[s] {
                Op::Same => (' ', a[ai[s]]),
                Op::Del => ('-', a[ai[s]]),
                Op::Add => ('+', b[bi[s]]),
            };
            let _ = writeln!(out, "{mark}{}", line.strip_suffix('\n').unwrap_or(line));
        }
    }
    out
}
