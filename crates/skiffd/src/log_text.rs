//! Text from a program or a screen, made fit for the daemon's log.
//! Best effort: a secret in a form not listed here still shows.

const HIDDEN: &str = "[hidden]";
/// The words whose value is a secret.
const KEYS: [&str; 6] = ["password", "token", "secret", "api_key", "authorization", "bearer"];
/// How known tokens begin.
const PREFIXES: [&str; 4] = ["sk-", "ghp_", "gho_", "AKIA"];

/// `text` with control characters gone, white space closed up, and secrets
/// replaced by `[hidden]`.
pub(crate) fn clean(text: &str) -> String {
    let plain: String = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).filter(|c| !c.is_control()).collect();
    let mut next_is_secret = false;
    let mut out = Vec::new();
    for word in plain.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        let key_end = KEYS.iter().find_map(|key| lower.find(key).map(|i| i + key.len()));
        if next_is_secret {
            // A key here is a scheme: `Authorization: Bearer abc`.
            next_is_secret = key_end.is_some();
            out.push(HIDDEN.to_string());
            continue;
        }
        let (shown, next) = read_word(word, key_end);
        next_is_secret = next;
        out.push(shown);
    }
    out.join(" ")
}

/// A word as the log shows it, and whether the next word is a secret.
fn read_word(word: &str, key_end: Option<usize>) -> (String, bool) {
    if let Some(end) = key_end {
        match word[end..].find(['=', ':']) {
            Some(sep) if end + sep + 1 < word.len() => return (format!("{}{HIDDEN}", &word[..=end + sep]), false),
            Some(_) => return (word.to_string(), true),
            None if word.starts_with('-') || word[..end].to_ascii_lowercase().ends_with("bearer") => return (word.to_string(), true),
            None => {}
        }
    }
    if let Some(start) = word.find("://").map(|i| i + 3) {
        if let Some(at) = word[start..].rfind('@') {
            return (format!("{}{HIDDEN}{}", &word[..start], &word[start + at..]), false);
        }
    }
    let mut parts = word.split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    let token = parts.any(|part| PREFIXES.iter().any(|prefix| part.starts_with(prefix)));
    (if token { HIDDEN } else { word }.to_string(), false)
}

/// [`clean`], cut to `max` characters.
pub(crate) fn clean_cut(text: &str, max: usize) -> String {
    let text = clean(text);
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    }
}

/// The part of `row` around `needle`, with `each` characters on each side.
/// `needle` is in the form the screen rules read: lower case, no white space.
/// Empty when `row` does not hold it: never the whole row.
pub(crate) fn around(row: &str, needle: &str, each: usize) -> String {
    let chars: Vec<char> = row.chars().collect();
    let folded: Vec<(char, usize)> = chars.iter().enumerate().filter(|(_, c)| !c.is_whitespace()).flat_map(|(i, c)| c.to_lowercase().map(move |l| (l, i))).collect();
    let needle: Vec<char> = needle.chars().collect();
    let Some(at) = folded.windows(needle.len().max(1)).position(|w| w.iter().zip(&needle).all(|(a, b)| a.0 == *b) && !needle.is_empty()) else {
        return String::new();
    };
    let (from, to) = (folded[at].1.saturating_sub(each), (folded[at + needle.len() - 1].1 + 1 + each).min(chars.len()));
    let text: String = chars[from..to].iter().collect();
    format!("{}{}{}", if from > 0 { "…" } else { "" }, text.trim(), if to < chars.len() { "…" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_secrets_are_hidden() {
        for (text, want) in [
            ("password=hunter2", "password=[hidden]"),
            ("Password: hunter", "Password: [hidden]"),
            ("--token abc", "--token [hidden]"),
            ("API_KEY=abc run", "API_KEY=[hidden] run"),
            ("MY_SECRET_VALUE=abc", "MY_SECRET_VALUE=[hidden]"),
            ("'Authorization: Bearer abc' x", "'Authorization: [hidden] [hidden] x"),
            ("Bearer abc", "Bearer [hidden]"),
            ("use sk-abcdefgh1234 now", "use [hidden] now"),
            ("--key=ghp_abcdefgh1234", "[hidden]"),
            ("gho_abc", "[hidden]"),
            ("AKIAABCDEFGH1234", "[hidden]"),
            ("https://me:pw@example.com/x", "https://[hidden]@example.com/x"),
            ("a\x1b]0;b\x07c\n\td", "a]0;bc d"),
        ] {
            assert_eq!(clean(text), want, "{text}");
        }
        for text in ["Esc to cancel · Tab to amend", "Fix the token refresh", "task-list and risk-free", "↑/↓ Navigate · tab Amend"] {
            assert_eq!(clean(text), text);
        }
    }

    #[test]
    fn the_limits_hold() {
        assert_eq!(around("left left left Esc to  cancel right right", "esctocancel", 5), "…left Esc to  cancel righ…");
        assert_eq!(around("nothing here", "esctocancel", 40), "");
        assert_eq!(clean_cut("abcdef", 3), "abc…");
        assert_eq!(clean_cut("abc", 3), "abc");
    }
}
