//! Text from a program or a screen, made fit for the daemon's log: no control
//! characters, and no secrets as far as their form shows. Best effort: it
//! hides too much before it hides too little.

use crate::program_status::hidden;

const HIDDEN: &str = "[hidden]";
/// The words whose value is a secret. A longer word such as `tokens` is none.
const KEYS: [&str; 9] = ["password", "passwd", "token", "secret", "api_key", "api-key", "apikey", "authorization", "bearer"];
/// A scheme word between `Authorization:` and the secret.
const SCHEMES: [&str; 3] = ["bearer", "basic", "token"];
/// How known tokens begin, at the start of a word.
const PREFIXES: [&str; 11] = ["sk-", "ghp_", "gho_", "ghs_", "ghu_", "xoxa-", "xoxb-", "xoxp-", "xoxr-", "xoxs-", "AKIA"];
/// Characters after a prefix that make it a token, not a word like `sk-learn`.
const MIN_AFTER_PREFIX: usize = 8;
/// An unbroken run this long, of letters and digits both, is taken for a secret.
const LONG_RUN: usize = 24;

/// `text` with control and hidden characters gone, white space closed up,
/// and secrets replaced by `[hidden]`.
pub(crate) fn clean(text: &str) -> String {
    let plain: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !hidden(*c))
        .collect();
    let mut out = Vec::new();
    let mut after = After::Nothing;
    for word in plain.split_whitespace() {
        let secret = match after {
            // The value already shows as hidden: the rest of it goes.
            After::Quote(q) => {
                if word.contains(q) {
                    after = After::Nothing;
                }
                continue;
            }
            After::Nothing => false,
            After::Secret => true,
            After::SecretUnlessWord => !plain_word(word),
        };
        if secret {
            out.push(HIDDEN.to_string());
            after = after_value(word);
        } else {
            let (shown, next) = read_word(word);
            out.push(shown);
            after = next;
        }
    }
    out.join(" ")
}

/// [`clean`], cut to `max` characters.
pub(crate) fn clean_cut(text: &str, max: usize) -> String {
    let text = clean(text);
    if text.chars().count() <= max {
        return text;
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}

/// The part of `row` around `needle`, with `each` characters on each side.
/// `needle` is in the form the screen rules read: lower case, no white space.
/// Empty when `row` does not hold it: never the whole row.
pub(crate) fn around(row: &str, needle: &str, each: usize) -> String {
    let chars: Vec<char> = row.chars().collect();
    let mut folded = Vec::with_capacity(chars.len());
    for (i, c) in chars.iter().enumerate() {
        if !c.is_whitespace() {
            folded.extend(c.to_lowercase().map(|l| (l, i)));
        }
    }
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return String::new();
    }
    let Some(at) = folded.windows(needle.len()).position(|w| w.iter().zip(&needle).all(|(a, b)| a.0 == *b)) else {
        return String::new();
    };
    let (first, last) = (folded[at].1, folded[at + needle.len() - 1].1);
    let (from, to) = (first.saturating_sub(each), (last + 1 + each).min(chars.len()));
    let text: String = chars[from..to].iter().collect();
    let (before, after) = (if from > 0 { "…" } else { "" }, if to < chars.len() { "…" } else { "" });
    format!("{before}{}{after}", text.trim())
}

/// What a word says about the words after it.
#[derive(Clone, Copy)]
enum After {
    Nothing,
    /// The next word is a secret.
    Secret,
    /// The next word is a secret, unless it is a plain word: `token refresh`.
    SecretUnlessWord,
    /// A quoted secret goes on until this quote closes it.
    Quote(char),
}

/// A word that may hold a key, a token or a long run.
fn read_word(word: &str) -> (String, After) {
    let lower = word.to_ascii_lowercase();
    let Some(end) = key_end(&lower) else {
        return (hide_runs(&hide_url_login(word)), After::Nothing);
    };
    if let Some(sep) = word[end..].find(['=', ':']) {
        let (head, value) = word.split_at(end + sep + 1);
        let bare = value.trim_matches(|c: char| !c.is_alphanumeric()).is_empty();
        return if bare {
            (word.to_string(), After::Secret)
        } else {
            (format!("{head}{HIDDEN}"), after_value(value))
        };
    }
    let next = if word.starts_with('-') || lower[..end].ends_with("bearer") { After::Secret } else { After::SecretUnlessWord };
    (hide_runs(word), next)
}

/// Where the first key in `lower` ends. A letter or digit after it makes it
/// another word.
fn key_end(lower: &str) -> Option<usize> {
    KEYS.iter()
        .flat_map(|key| lower.match_indices(key).map(move |(i, _)| i + key.len()))
        .filter(|&end| !lower[end..].starts_with(|c: char| c.is_alphanumeric()))
        .min()
}

/// What follows a hidden value: more of it, when a quote stays open or the
/// value was only the scheme word before the secret.
fn after_value(value: &str) -> After {
    let bare = value.trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase();
    if SCHEMES.contains(&bare.as_str()) {
        return After::Secret;
    }
    match value.chars().next() {
        Some(q @ ('"' | '\'')) if !value[1..].contains(q) => After::Quote(q),
        _ => After::Nothing,
    }
}

/// Letters only, once the punctuation around it is gone.
fn plain_word(word: &str) -> bool {
    word.trim_matches(|c: char| !c.is_alphanumeric()).chars().all(char::is_alphabetic)
}

/// `scheme://user:password@host` without the part before the host.
fn hide_url_login(word: &str) -> String {
    if let Some(start) = word.find("://").map(|i| i + 3) {
        if let Some(at) = word[start..].rfind('@') {
            return format!("{}{HIDDEN}{}", &word[..start], &word[start + at..]);
        }
    }
    word.to_string()
}

/// `word` with each run that looks like a token replaced.
fn hide_runs(word: &str) -> String {
    let in_run = |c: char| c.is_ascii_alphanumeric() || "_-+/=".contains(c);
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    while !rest.is_empty() {
        let inside = rest.starts_with(in_run);
        let end = rest.find(|c: char| in_run(c) != inside).unwrap_or(rest.len());
        let (part, tail) = rest.split_at(end);
        out.push_str(if inside && secret_run(part) { HIDDEN } else { part });
        rest = tail;
    }
    out
}

fn secret_run(run: &str) -> bool {
    let long = run.len() >= LONG_RUN && run.contains(|c: char| c.is_ascii_digit()) && run.contains(|c: char| c.is_ascii_alphabetic());
    long || PREFIXES.iter().any(|prefix| {
        run.match_indices(prefix).any(|(i, _)| {
            let start = !run[..i].ends_with(|c: char| c.is_ascii_alphanumeric());
            start && run.len() - i - prefix.len() >= MIN_AFTER_PREFIX
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_hidden() {
        let cases = [
            ("password=hunter2", "password=[hidden]"),
            ("Password: hunter", "Password: [hidden]"),
            ("--token abc", "--token [hidden]"),
            ("--api-key=abc", "--api-key=[hidden]"),
            ("API_KEY=abc run", "API_KEY=[hidden] run"),
            ("MY_SECRET_VALUE=abc", "MY_SECRET_VALUE=[hidden]"),
            (r#"{"token":"abc"}"#, r#"{"token":[hidden]"#),
            (r#"secret="two words" after"#, "secret=[hidden] after"),
            ("curl -H 'Authorization: Bearer abc' x", "curl -H 'Authorization: [hidden] [hidden] x"),
            ("Bearer abc", "Bearer [hidden]"),
            ("token hunter2", "token [hidden]"),
            ("use sk-abcdefgh1234 now", "use [hidden] now"),
            ("--key=ghp_abcdefgh1234", "[hidden]"),
            ("xoxb-12345678-abc", "[hidden]"),
            ("AKIAABCDEFGH1234", "[hidden]"),
            ("a c0ffee0123456789abcdef0123456789 b", "a [hidden] b"),
            ("https://me:pw@example.com/x", "https://[hidden]@example.com/x"),
        ];
        for (text, want) in cases {
            assert_eq!(clean(text), want, "{text}");
        }
    }

    #[test]
    fn plain_text_stays() {
        for text in [
            "Esc to cancel · Tab to amend",
            "Fix the token refresh",
            "400 tokens: fine",
            "Enter your password.",
            "task-list and risk-assessment",
            "sk-learn",
            "↑/↓ Navigate · tab Amend",
        ] {
            assert_eq!(clean(text), text);
        }
    }

    #[test]
    fn control_and_hidden_characters_go() {
        assert_eq!(clean("a\x1b]0;x\x07b\u{202e}c\u{200b}d\n\te"), "a]0;xbcd e");
    }

    #[test]
    fn around_shows_only_the_match_and_its_sides() {
        let row = "left left left Esc to  cancel right right right";
        assert_eq!(around(row, "esctocancel", 5), "…left Esc to  cancel righ…");
        assert_eq!(around("Esc to cancel", "esctocancel", 40), "Esc to cancel");
        assert_eq!(around("nothing here", "esctocancel", 40), "");
    }

    #[test]
    fn clean_cut_marks_the_cut() {
        assert_eq!(clean_cut("abcdef", 3), "abc…");
        assert_eq!(clean_cut("abc", 3), "abc");
    }
}
