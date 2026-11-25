//! Line grouping and text rules for block assembly.

use crate::rows::{is_cjk, Segment};

pub(crate) fn group_rows(mut segments: Vec<Segment>) -> Vec<Vec<Segment>> {
    segments.sort_by(|a, b| b.base.total_cmp(&a.base).then(a.x0.total_cmp(&b.x0)));
    let mut rows: Vec<Vec<Segment>> = Vec::new();
    for segment in segments {
        if let Some(row) = rows.last_mut() {
            let anchor = &row[0];
            let table = anchor.table.is_some() || segment.table.is_some();
            if !table && (anchor.base - segment.base).abs() <= 0.45 * anchor.size.max(segment.size)
            {
                row.push(segment);
                continue;
            }
        }
        rows.push(vec![segment]);
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    rows
}

pub(crate) fn is_equation_number(text: &str) -> bool {
    let t = text.trim();
    t.len() <= 8
        && t.starts_with('(')
        && t.ends_with(')')
        && t[1..t.len() - 1]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
}

pub(crate) fn bullet_body(text: &str) -> Option<&str> {
    for bullet in [
        "•", "◦", "▪", "‣", "●", "○", "■", "□", "–", "—", "-", "*", "·", "\u{F0B7}", "➢", "✓",
    ] {
        if let Some(rest) = text.strip_prefix(bullet) {
            if rest.starts_with(' ')
                || (bullet != "-"
                    && bullet != "*"
                    && bullet != "–"
                    && bullet != "—"
                    && !rest.is_empty())
            {
                let body = rest.trim_start();
                if !body.is_empty() {
                    return Some(body);
                }
            }
        }
    }
    None
}

pub(crate) fn starts_enumerated(text: &str) -> bool {
    let mut chars = text.chars();
    let first = chars.next();
    match first {
        Some('(') => {
            let inner: String = chars.by_ref().take_while(|c| *c != ')').collect();
            !inner.is_empty()
                && inner.len() <= 4
                && inner.chars().all(|c| c.is_ascii_alphanumeric())
        }
        Some(c) if c.is_ascii_digit() || c.is_ascii_lowercase() => {
            let head: String = text
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let rest = &text[head.len()..];
            head.len() <= 3
                && (head.chars().all(|c| c.is_ascii_digit()) || head.len() == 1)
                && (rest.starts_with(". ") || rest.starts_with(") "))
        }
        _ => false,
    }
}

/// Section headings like "3.2 Attention" or "4 Why Self-Attention".
pub(crate) fn numbered_heading_level(text: &str) -> Option<usize> {
    let text = text.trim();
    let (number, rest) = text.split_once(' ')?;
    let number = number.trim_end_matches('.');
    if number.is_empty() || number.len() > 8 {
        return None;
    }
    let parts: Vec<&str> = number.split('.').collect();
    let numeric = parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let lettered =
        parts.len() == 1 && parts[0].len() == 1 && parts[0].chars().all(|c| c.is_ascii_uppercase());
    if !(numeric || lettered) || parts.len() > 4 {
        return None;
    }
    if numeric && parts[0].parse::<u32>().ok()? > 30 {
        return None;
    }
    let rest = rest.trim();
    let words = rest.split_whitespace().count();
    let first = rest.chars().next()?;
    if !first.is_uppercase() || words == 0 || words > 12 || rest.len() > 90 {
        return None;
    }
    if rest.ends_with('.') || rest.ends_with(',') || rest.ends_with(':') && words > 6 {
        return None;
    }
    // A heading does not stop on a word that needs another after it.
    let last = rest.rsplit(' ').next().unwrap_or("").to_lowercase();
    if words > 3
        && [
            "the", "of", "and", "to", "a", "an", "in", "for", "with", "by", "on", "or",
        ]
        .contains(&last.as_str())
    {
        return None;
    }
    // Headings are mostly letters, not numbers or math.
    let letters = rest.chars().filter(|c| c.is_alphabetic()).count();
    if letters * 10 < rest.chars().filter(|c| !c.is_whitespace()).count() * 7 {
        return None;
    }
    Some((parts.len() + 1).min(6))
}

pub(crate) fn named_heading(text: &str) -> bool {
    const NAMES: &[&str] = &[
        "abstract",
        "introduction",
        "background",
        "related work",
        "method",
        "methods",
        "methodology",
        "results",
        "discussion",
        "conclusion",
        "conclusions",
        "references",
        "bibliography",
        "acknowledgments",
        "acknowledgements",
        "appendix",
        "summary",
        "contents",
        "table of contents",
        "preface",
        "foreword",
        "index",
        "glossary",
    ];
    let lower = text.trim().trim_end_matches(':').to_lowercase();
    NAMES.contains(&lower.as_str())
}

pub(crate) fn join_line(paragraph: &mut String, line: &str) {
    if paragraph.is_empty() {
        paragraph.push_str(line);
        return;
    }
    let prev_last = paragraph.chars().last();
    let next_first = line.chars().next();
    if paragraph.ends_with('-') {
        let mut chars = paragraph.chars().rev();
        chars.next();
        let before = chars.next();
        let next_word = line.split_whitespace().next().unwrap_or("");
        let prev_word = paragraph.split_whitespace().next_back().unwrap_or("");
        let compound = prev_word[..prev_word.len() - 1].contains('-') || next_word.contains('-');
        if before.is_some_and(char::is_alphabetic) && next_first.is_some_and(char::is_lowercase) {
            if !compound {
                // A word broken across lines: "transduc-" + "tion".
                paragraph.pop();
            }
            // Compounds keep the hyphen: "left-to-" + "right".
            paragraph.push_str(line);
            return;
        }
    }
    if !(prev_last.is_some_and(is_cjk) && next_first.is_some_and(is_cjk)) {
        paragraph.push(' ');
    }
    paragraph.push_str(line);
}
