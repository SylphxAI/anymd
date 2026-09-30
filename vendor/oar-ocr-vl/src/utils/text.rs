use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

// Shared regex patterns
pub static UNDERSCORE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"_{4,}").expect("static regex"));
pub static DOTS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\.{4,}").expect("static regex"));
pub static LATEX_BRACKETS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\\(big|Big|bigg|Bigg|bigl|bigr|Bigl|Bigr|biggr|biggl|Biggl|Biggr)\{(\\?[{{}}\[\]()|])\}",
    )
    .expect("static regex")
});
pub static TABLE_TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"</?(table|tr|th|td|thead|tbody|tfoot)[^>]*>").expect("static regex"));
pub static TAG_NEWLINES_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r">\s*\n+\s*").expect("static regex"));

/// Clean special tokens from text.
pub fn clean_special_tokens(text: &str) -> String {
    text.replace("-<|sn|>", "")
        .replace("<|sn|>", " ")
        .replace("<|unk|>", "")
        .replace('\u{FFFF}', "")
}

/// Process text for common OCR artifacts (underscores, dots).
pub fn process_text(text: &str) -> String {
    let mut result = text.to_string();
    result = UNDERSCORE_RE.replace_all(&result, "___").to_string();
    result = DOTS_RE.replace_all(&result, "...").to_string();
    result.trim().to_string()
}

/// Format formula text with LaTeX delimiters.
pub fn format_formula(text: &str) -> String {
    let mut result = text.to_string();
    // Clean special tokens
    result = clean_special_tokens(&result);
    // Standardize mu
    result = result.replace(r"\upmu", r"\mu");
    // Remove existing delimiters to avoid double wrapping
    result = result.replace("\\[", "");
    result = result.replace("\\]", "");
    result = result.replace("\\(", "");
    result = result.replace("\\)", "");
    result = result.trim().trim_matches('$').to_string();

    result = result.replace('\n', "\\\\\n");
    result = fix_latex_brackets(&result);
    // Wrap in display math delimiters
    format!("$${}$$", result.trim())
}

/// Format table HTML content.
pub fn format_table(text: &str) -> String {
    let mut result = text.to_string();
    // Fix common OCR attribute errors
    result = result.replace("<tdcolspan=", "<td colspan=");
    result = result.replace("<tdrowspan=", "<td rowspan=");
    result = result.replace("\"colspan=", "\" colspan=");

    result = clean_special_tokens(&result);

    // Fix LaTeX delimiters
    result = result.replace("\\(", "$").replace("\\)", "$");
    result = result.replace("\\[", "$$").replace("\\]", "$$");

    // Collapse newlines between tags
    result = TAG_NEWLINES_RE.replace_all(&result, ">").to_string();

    result
}

/// Format regular text output with LaTeX normalization.
pub fn format_text(text: &str) -> String {
    let mut result = clean_special_tokens(text);
    // Convert inline LaTeX delimiters
    if result.contains("\\(") && result.contains("\\)") {
        result = result.replace("\\(", " $ ").replace("\\)", " $ ");
    }
    if result.contains("\\[") && result.contains("\\]") {
        result = result.replace("\\[", " $$ ").replace("\\]", " $$ ");
    }
    result = result.replace(r"$\bullet$", "•");
    // Strip HTML table tags if present in text mode
    if result.contains("<table>") {
        result = TABLE_TAG_RE.replace_all(&result, "").to_string();
    }

    // Normalize inline math spacing
    result = tighten_inline_dollar_math(&result);
    result = collapse_consecutive_spaces(&result);
    result = remove_space_before_punctuation(&result);

    process_text(&result)
}

pub fn fix_latex_brackets(text: &str) -> String {
    LATEX_BRACKETS_RE.replace_all(text, r"\$1$2").to_string()
}

pub fn strip_math_wrappers(input: &str) -> &str {
    let mut trimmed = input.trim();
    trimmed = trimmed
        .strip_prefix("$$")
        .and_then(|s| s.strip_suffix("$$"))
        .unwrap_or(trimmed);
    trimmed = trimmed
        .strip_prefix('$')
        .and_then(|s| s.strip_suffix('$'))
        .unwrap_or(trimmed);
    trimmed.trim()
}

pub fn collapse_consecutive_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for ch in text.chars() {
        if ch == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
            out.push(' ');
        } else {
            prev_space = false;
            out.push(ch);
        }
    }
    out
}

pub fn tighten_inline_dollar_math(text: &str) -> String {
    // Trim whitespace inside *single* `$...$` blocks, while leaving `$$...$$` untouched.
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '$' {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // Check if this '$' is part of '$$' (display math)
        let prev_is_dollar = i > 0 && chars[i - 1] == '$';
        let next_is_dollar = i + 1 < chars.len() && chars[i + 1] == '$';

        if prev_is_dollar || next_is_dollar {
            result.push('$');
            i += 1;
            continue;
        }

        // Find the closing single '$'
        let mut close_idx = None;
        let mut j = i + 1;
        while j < chars.len() {
            if chars[j] == '$' {
                let prev_d = j > 0 && chars[j - 1] == '$';
                let next_d = j + 1 < chars.len() && chars[j + 1] == '$';
                if prev_d || next_d {
                    j += 1;
                    continue;
                }
                close_idx = Some(j);
                break;
            }
            j += 1;
        }

        if let Some(end_idx) = close_idx {
            let inner: String = chars[i + 1..end_idx].iter().collect();
            result.push('$');
            result.push_str(inner.trim());
            result.push('$');
            i = end_idx + 1;
        } else {
            // Unmatched '$' (e.g., currency); keep it.
            result.push('$');
            i += 1;
        }
    }

    result
}

pub fn remove_space_before_punctuation(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ' '
            && let Some(&next) = chars.peek()
            && matches!(next, ',' | '.' | ';' | ':' | '!' | '?' | ')')
        {
            continue;
        }
        out.push(ch);
    }
    out
}

fn find_shortest_repeating_substring(chars: &[char]) -> Option<&[char]> {
    let n = chars.len();
    for i in 1..=n / 2 {
        if n.is_multiple_of(i) {
            let unit = &chars[..i];
            let mut matches = true;
            for start in (0..n).step_by(i) {
                if &chars[start..start + i] != unit {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Some(unit);
            }
        }
    }
    None
}

fn find_repeating_suffix(
    chars: &[char],
    min_len: usize,
    min_repeats: usize,
) -> Option<(&[char], &[char], usize)> {
    let n = chars.len();
    for i in (min_len..=n / min_repeats).rev() {
        let total = i * min_repeats;
        if n < total {
            continue;
        }
        let unit = &chars[n - i..n];
        let mut matches = true;
        let start = n - total;
        for offset in 0..min_repeats {
            let chunk_start = start + offset * i;
            if &chars[chunk_start..chunk_start + i] != unit {
                matches = false;
                break;
            }
        }
        if matches {
            let mut count = 0;
            let mut end = n;
            while end >= i && &chars[end - i..end] == unit {
                count += 1;
                end -= i;
            }
            let prefix = &chars[..end];
            return Some((prefix, unit, count));
        }
    }
    None
}

/// Detect and truncate repetitive content.
pub fn truncate_repetitive_content(
    content: &str,
    line_threshold: usize,
    char_threshold: usize,
    min_len: usize,
) -> String {
    let stripped = content.trim();
    if stripped.is_empty() {
        return content.to_string();
    }
    let chars: Vec<char> = stripped.chars().collect();
    let stripped_chars = chars.len();

    if !stripped.contains('\n')
        && stripped_chars > 100
        && let Some((prefix, unit, count)) = find_repeating_suffix(&chars, 8, 5)
        && unit.len() * count > stripped_chars / 2
    {
        return prefix.iter().collect();
    }

    if !stripped.contains('\n')
        && stripped_chars > min_len
        && let Some(unit) = find_shortest_repeating_substring(&chars)
    {
        let count = stripped_chars / unit.len();
        if count >= char_threshold {
            return unit.iter().collect();
        }
    }

    let lines: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return content.to_string();
    }

    let total_lines = lines.len();
    if total_lines < line_threshold {
        return content.to_string();
    }

    let mut counts = HashMap::new();
    for line in &lines {
        *counts.entry(*line).or_insert(0usize) += 1;
    }

    if let Some((most_common, count)) = counts.into_iter().max_by_key(|(_, c)| *c)
        && count >= line_threshold
        && (count as f32 / total_lines as f32) >= 0.8
    {
        return most_common.to_string();
    }

    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tighten_inline_dollar_math_basic() {
        assert_eq!(tighten_inline_dollar_math("$ x $"), "$x$");
        assert_eq!(tighten_inline_dollar_math("$  y  $"), "$y$");
        assert_eq!(tighten_inline_dollar_math("$x$"), "$x$");
    }

    #[test]
    fn test_tighten_inline_dollar_math_display_math_untouched() {
        assert_eq!(tighten_inline_dollar_math("$$ x $$"), "$$ x $$");
        assert_eq!(tighten_inline_dollar_math("$$  y  $$"), "$$  y  $$");
    }

    #[test]
    fn test_tighten_inline_dollar_math_unmatched() {
        assert_eq!(tighten_inline_dollar_math("$100"), "$100");
        assert_eq!(tighten_inline_dollar_math("price is $50"), "price is $50");
    }

    #[test]
    fn test_tighten_inline_dollar_math_utf8_safety() {
        // Multi-byte characters inside inline math should not cause panic
        assert_eq!(tighten_inline_dollar_math("$€$"), "$€$");
        assert_eq!(tighten_inline_dollar_math("$ €100 $"), "$€100$");
        assert_eq!(tighten_inline_dollar_math("$ α + β $"), "$α + β$");
        assert_eq!(tighten_inline_dollar_math("$中文$"), "$中文$");
        assert_eq!(tighten_inline_dollar_math("$ 数学 $"), "$数学$");
        // Mixed content
        assert_eq!(
            tighten_inline_dollar_math("price $100€$ and $ α $"),
            "price $100€$ and $α$"
        );
    }

    #[test]
    fn test_tighten_inline_dollar_math_mixed() {
        assert_eq!(
            tighten_inline_dollar_math("text $ x $ more $$ y $$ end"),
            "text $x$ more $$ y $$ end"
        );
    }

    #[test]
    fn test_format_formula() {
        assert_eq!(format_formula("x + y = z"), "$$x + y = z$$");
        assert_eq!(format_formula("\\[x^2\\]"), "$$x^2$$");
    }

    #[test]
    fn test_clean_special_tokens() {
        assert_eq!(clean_special_tokens("hello<|sn|>world"), "hello world");
        assert_eq!(clean_special_tokens("test<|unk|>"), "test");
    }

    #[test]
    fn test_truncate_repetitive_content() {
        let text = "hello\nhello\nhello\nhello\nhello\nhello\nhello\nhello\nhello\nhello\nhello";
        let result = truncate_repetitive_content(text, 10, 10, 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_find_shortest_repeating() {
        let s1: Vec<char> = "abcabcabc".chars().collect();
        let u1: Vec<char> = "abc".chars().collect();
        assert_eq!(find_shortest_repeating_substring(&s1), Some(u1.as_slice()));

        let s2: Vec<char> = "綠洲綠洲綠洲".chars().collect();
        let u2: Vec<char> = "綠洲".chars().collect();
        assert_eq!(find_shortest_repeating_substring(&s2), Some(u2.as_slice()));

        let s3: Vec<char> = "hello".chars().collect();
        assert_eq!(find_shortest_repeating_substring(&s3), None);
    }
}
