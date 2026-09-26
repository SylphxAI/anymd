//! Running headers, footers and page numbers.

use std::collections::{HashMap, HashSet};

use crate::extract::RawPage;
use crate::rows::{rows_of, segments_of_row, Segment};

pub(crate) fn margin_key(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            if c.is_ascii_digit() {
                '#'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

pub(crate) fn in_margin(segment: &Segment, page: &RawPage) -> bool {
    let height = (page.top - page.bottom).max(1.0);
    segment.bottom > page.top - height * 0.08 || segment.top < page.bottom + height * 0.08
}

/// Lines in the top/bottom margin that repeat on at least half the pages.
pub(crate) fn repeated_margin_lines(pages: &[RawPage]) -> HashSet<String> {
    let usable = pages.iter().filter(|page| page.glyphs.is_ok()).count();
    if usable < 3 {
        return HashSet::new();
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for page in pages {
        let Ok(glyphs) = &page.glyphs else { continue };
        let mut seen = HashSet::new();
        for row in rows_of(glyphs.clone()) {
            for segment in segments_of_row(row) {
                if in_margin(&segment, page) {
                    let key = margin_key(&segment.text);
                    if !key.is_empty() && seen.insert(key.clone()) {
                        *counts.entry(key).or_default() += 1;
                    }
                }
            }
        }
    }
    let needed = usable.div_ceil(2).max(3);
    counts
        .into_iter()
        .filter(|(_, count)| *count >= needed)
        .map(|(key, _)| key)
        .collect()
}

pub(crate) fn is_page_number(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let lower = lower.trim_matches(|c: char| c == '-' || c == '–' || c == '—' || c.is_whitespace());
    let lower = lower.strip_prefix("page").map(str::trim).unwrap_or(lower);
    if lower.is_empty() || lower.len() > 16 {
        return false;
    }
    let mut parts = lower
        .split(|c: char| c == '/' || c.is_whitespace())
        .filter(|p| !p.is_empty() && *p != "of");
    let first = parts.next().unwrap_or("");
    let numeric = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    let roman = |p: &str| !p.is_empty() && p.len() <= 6 && p.chars().all(|c| "ivxlc".contains(c));
    (numeric(first) || roman(first)) && parts.all(numeric)
}
