//! Plain tables and JSON for list commands.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

/// Enough of an id to tell sessions apart in a table.
pub fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// The path with the home directory as `~`.
pub fn tilde(p: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = p.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    p.display().to_string()
}

pub fn json<T: Serialize + ?Sized>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

/// Left-aligned columns, two spaces apart. The last column is not padded.
/// An all-blank header prints no header line.
pub fn table(header: &[&str], rows: &[Vec<String>]) {
    let mut width: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (w, c) in width.iter_mut().zip(r) {
            *w = (*w).max(c.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let last = cells.len().saturating_sub(1);
        let mut out = String::new();
        for (i, c) in cells.iter().enumerate() {
            if i == last {
                out.push_str(c);
            } else {
                out.push_str(&format!("{c:<w$}  ", w = width[i]));
            }
        }
        println!("{}", out.trim_end());
    };
    if header.iter().any(|h| !h.is_empty()) {
        line(header.to_vec());
    }
    for r in rows {
        line(r.iter().map(String::as_str).collect());
    }
}
