//! Shell aliases. A command line in `projects.toml` may name an alias from
//! the user's rc files, such as `claudey` for `claude --dangerously-skip-permissions`.
//! The daemon has no shell in front of an agent, so it expands the alias itself.

use std::{collections::HashMap, sync::OnceLock};

static ALIASES: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();

/// Stores the aliases. The first call wins.
pub fn set(text: &str) {
    let _ = ALIASES.set(parse(text));
}

/// The command a first word stands for, as words, if it is an alias.
pub fn expand(word: &str) -> Option<&'static [String]> {
    ALIASES.get()?.get(word).map(|v| v.as_slice()).filter(|v| !v.is_empty())
}

/// Reads the output of the shell's `alias` builtin: `alias name='value'` lines.
pub fn parse(text: &str) -> HashMap<String, Vec<String>> {
    text.lines()
        .filter_map(|l| l.strip_prefix("alias "))
        .filter_map(|l| l.split_once('='))
        .map(|(name, value)| {
            let value = words(value).concat();
            (name.trim().to_string(), words(&value))
        })
        .collect()
}

/// Splits a line into words. Handles single quotes, double quotes and backslashes.
pub fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut live = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                live = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                live = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => cur.extend(chars.next()),
                        _ => cur.push(q),
                    }
                }
            }
            '\\' => {
                live = true;
                cur.extend(chars.next());
            }
            c if c.is_whitespace() => {
                if live {
                    out.push(std::mem::take(&mut cur));
                    live = false;
                }
            }
            c => {
                live = true;
                cur.push(c);
            }
        }
    }
    if live {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_alias_lines() {
        let m = parse("alias claudey='claude --dangerously-skip-permissions'\nalias ll='ls -l'\n");
        assert_eq!(m["claudey"], ["claude", "--dangerously-skip-permissions"]);
        assert_eq!(m["ll"], ["ls", "-l"]);
    }
}
