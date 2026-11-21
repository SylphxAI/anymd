//! Literal matches, BM25 ranking, and highlighted excerpts.

use std::collections::HashMap;

use crate::document::Unit;

use super::sources::SearchDoc;

pub(super) struct Hit<'a> {
    pub(super) doc: &'a SearchDoc,
    pub(super) unit: &'a Unit,
    pub(super) snippet: String,
    pub(super) offset: usize,
}

pub(super) fn literal_hits<'a>(
    docs: &'a [SearchDoc],
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    context: usize,
    max_per_doc: usize,
) -> (Vec<(usize, usize, Vec<Hit<'a>>)>, usize) {
    // Per document: (doc index, total count, shown hits).
    let (needle, _) = fold(query, case_sensitive);
    let mut out = Vec::new();
    let mut grand_total = 0;
    for (doc_index, doc) in docs.iter().enumerate() {
        let mut count = 0;
        let mut hits = Vec::new();
        for unit in doc.units.iter() {
            let (hay, map) = fold(&unit.markdown, case_sensitive);
            let mut from = 0;
            while let Some(found) = hay[from..].find(&needle) {
                let start = from + found;
                let end = start + needle.len();
                from = end;
                if whole_word
                    && (is_word_char(hay[..start].chars().next_back())
                        || is_word_char(hay[end..].chars().next()))
                {
                    continue;
                }
                count += 1;
                if hits.len() < max_per_doc {
                    let (s, e) = (map[start], map[end.min(map.len() - 1)]);
                    let e = if e <= s { s + 1 } else { e };
                    let mut e = e.min(unit.markdown.len());
                    while !unit.markdown.is_char_boundary(e) {
                        e += 1;
                    }
                    hits.push(Hit {
                        doc,
                        unit,
                        snippet: snippet(&unit.markdown, s, e, context, &[(s, e)]),
                        offset: s,
                    });
                }
            }
        }
        grand_total += count;
        if count > 0 {
            out.push((doc_index, count, hits));
        }
    }
    (out, grand_total)
}

pub(super) fn ranked_hits<'a>(
    docs: &'a [SearchDoc],
    query: &str,
    context: usize,
    limit: usize,
) -> Vec<(f64, Hit<'a>)> {
    let mut query_terms = terms_of(query);
    query_terms.sort();
    query_terms.dedup();
    if query_terms.is_empty() {
        return Vec::new();
    }
    let mut units: Vec<(&SearchDoc, &Unit, HashMap<String, usize>, usize)> = Vec::new();
    for doc in docs {
        for unit in doc.units.iter() {
            let terms = terms_of(&unit.markdown);
            let len = terms.len();
            let mut tf: HashMap<String, usize> = HashMap::new();
            for term in terms {
                if query_terms.binary_search(&term).is_ok() {
                    *tf.entry(term).or_default() += 1;
                }
            }
            units.push((doc, unit, tf, len));
        }
    }
    let n = units.len() as f64;
    let avg = units.iter().map(|u| u.3 as f64).sum::<f64>() / n.max(1.0);
    let idf: HashMap<&String, f64> = query_terms
        .iter()
        .map(|term| {
            let df = units.iter().filter(|u| u.2.contains_key(term)).count() as f64;
            (term, ((n - df + 0.5) / (df + 0.5) + 1.0).ln())
        })
        .collect();
    let (k1, b) = (1.2, 0.75);
    let mut scored: Vec<(f64, usize)> = units
        .iter()
        .enumerate()
        .filter_map(|(index, (_, _, tf, len))| {
            let score: f64 = tf
                .iter()
                .map(|(term, f)| {
                    let f = *f as f64;
                    idf[term] * f * (k1 + 1.0)
                        / (f + k1 * (1.0 - b + b * *len as f64 / avg.max(1.0)))
                })
                .sum();
            (score > 0.0).then_some((score, index))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(limit);
    scored
        .into_iter()
        .map(|(score, index)| {
            let (doc, unit, _, _) = &units[index];
            // Highlight query terms; center on the rarest one present.
            let (lower, offsets) = fold(&unit.markdown, false);
            let mut highlights = Vec::new();
            let mut anchor: Option<(f64, usize, usize)> = None;
            for term in &query_terms {
                let mut from = 0;
                while let Some(found) = lower[from..].find(term.as_str()) {
                    let start = from + found;
                    let end = start + term.len();
                    from = end;
                    let bounded = is_cjk(term.chars().next().unwrap_or(' '))
                        || (!is_word_char(lower[..start].chars().next_back())
                            && !is_word_char(lower[end..].chars().next()));
                    if bounded {
                        let start = offsets[start];
                        let mut end = offsets[end];
                        if end <= start {
                            end = start
                                + unit.markdown[start..]
                                    .chars()
                                    .next()
                                    .map(char::len_utf8)
                                    .unwrap_or(0);
                        }
                        highlights.push((start, end));
                        let weight = idf[term];
                        if anchor.is_none_or(|(w, _, _)| weight > w) {
                            anchor = Some((weight, start, end));
                        }
                    }
                }
            }
            let (start, end) = anchor.map(|(_, s, e)| (s, e)).unwrap_or((0, 0));
            (
                score,
                Hit {
                    doc,
                    unit,
                    snippet: snippet(&unit.markdown, start, end, context, &highlights),
                    offset: start,
                },
            )
        })
        .collect()
}

/// Case-folded, whitespace-collapsed text with a map back to source byte offsets.
pub(in crate::lean) fn fold(text: &str, case_sensitive: bool) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len());
    let mut last_space = true;
    for (offset, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                map.push(offset);
                last_space = true;
            }
            continue;
        }
        last_space = false;
        let start = out.len();
        if case_sensitive {
            out.push(ch);
        } else {
            out.extend(ch.to_lowercase());
        }
        map.extend(std::iter::repeat_n(offset, out.len() - start));
    }
    map.push(text.len());
    (out, map)
}

fn is_word_char(ch: Option<char>) -> bool {
    ch.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// A snippet of `text` around [start, end) with every `highlights` range in bold.
pub(in crate::lean) fn snippet(
    text: &str,
    start: usize,
    end: usize,
    context: usize,
    highlights: &[(usize, usize)],
) -> String {
    let mut from = start.saturating_sub(context);
    while !text.is_char_boundary(from) {
        from -= 1;
    }
    let mut to = (end + context).min(text.len());
    while !text.is_char_boundary(to) {
        to += 1;
    }
    // Snap to word boundaries so snippets do not start mid-word.
    if from > 0 {
        if let Some(space) = text[from..start].find(char::is_whitespace) {
            from += space + 1;
        }
    }
    if to < text.len() {
        if let Some(space) = text[end..to].rfind(char::is_whitespace) {
            to = end + space;
        }
    }
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let mut cursor = from;
    let mut ranges: Vec<(usize, usize)> = highlights
        .iter()
        .copied()
        .filter(|(s, e)| *s >= from && *e <= to && s < e)
        .collect();
    ranges.sort_unstable();
    for (s, e) in ranges {
        if s < cursor {
            continue;
        }
        out.push_str(&text[cursor..s]);
        out.push_str("**");
        out.push_str(&text[s..e]);
        out.push_str("**");
        cursor = e;
    }
    out.push_str(&text[cursor..to]);
    if to < text.len() {
        out.push('…');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(in crate::lean) fn terms_of(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() && !is_cjk(ch) {
            word.extend(ch.to_lowercase());
        } else {
            if !word.is_empty() {
                terms.push(std::mem::take(&mut word));
            }
            if is_cjk(ch) {
                terms.push(ch.to_string());
            }
        }
    }
    if !word.is_empty() {
        terms.push(word);
    }
    terms
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}
