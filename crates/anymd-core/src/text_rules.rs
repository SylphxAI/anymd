//! The text rules the CLI and the browser build share, stated once: how many
//! tokens a Markdown result costs and how a front-matter scalar is written.
//! Both live in the pure rules crate, so the `anymd` binary and `anymd-wasm`
//! answer with the same numbers and the same YAML.

/// Cheap, slightly conservative estimate of LLM tokens (calibrated on o200k).
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_alphabetic() {
            let mut run: usize = 1;
            while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                chars.next();
                run += 1;
            }
            tokens += run.div_ceil(5);
        } else if ch.is_ascii_digit() {
            let mut run: usize = 1;
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
                run += 1;
            }
            tokens += run.div_ceil(3);
        } else if ch.is_whitespace() {
            let mut run: usize = 1;
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
                run += 1;
            }
            tokens += usize::from(run >= 3);
        } else {
            tokens += 1;
        }
    }
    tokens
}

/// A front-matter scalar: one line, quoted when YAML would otherwise read it
/// as a nested key, a flow collection, an anchor or a comment.
pub fn yaml_value(value: &str) -> String {
    let single_line = value.replace(['\r', '\n'], " ");
    let needs_quotes = single_line.contains(": ")
        || single_line.starts_with([
            '"', '\'', '[', '{', '#', '&', '*', '!', '|', '>', '%', '@', '`', '-', '?',
        ])
        || single_line.ends_with(':');
    if needs_quotes {
        serde_json::to_string(&single_line).unwrap_or(single_line)
    } else {
        single_line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_estimate_counts_letter_runs_digit_runs_and_wide_spacing() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(
            estimate_tokens("abcdefghij"),
            2,
            "a ten-letter run is two tokens"
        );
        assert_eq!(
            estimate_tokens("123456"),
            2,
            "a six-digit run is two tokens"
        );
        assert_eq!(estimate_tokens("a   b"), 3, "a wide gap is one token");
        assert_eq!(estimate_tokens("a b"), 2, "a single space is one token");
        assert_eq!(estimate_tokens("!!!"), 3);
    }

    #[test]
    fn a_front_matter_scalar_is_one_line_and_quoted_only_when_yaml_needs_it() {
        assert_eq!(yaml_value("plain"), "plain");
        assert_eq!(yaml_value("has: colon"), "\"has: colon\"");
        assert_eq!(yaml_value("- dash"), "\"- dash\"");
        assert_eq!(yaml_value("trailing:"), "\"trailing:\"");
        assert_eq!(yaml_value("two\nlines"), "two lines");
    }
}
