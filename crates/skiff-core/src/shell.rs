pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// One word as a POSIX shell reads it back: bare when every character is in
/// `A-Za-z0-9_./~+@%:,=-`, else in single quotes. A typed path and a typed
/// flag both go through it.
pub fn quote_word(s: &str) -> String {
    let bare = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_./~+@%:,=-".contains(c));
    if bare { s.to_string() } else { shell_quote(s) }
}
