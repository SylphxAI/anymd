use super::{geometry::Geometry, CiteDecision, CiteNormalization, CiteSupport};

pub(super) fn decide(
    text: &str,
    quote: &str,
    normalization: CiteNormalization,
    complete: bool,
    geometry: &Geometry<'_>,
) -> CiteDecision {
    for normalized in [false, true] {
        if normalized && normalization != CiteNormalization::WhitespaceV1 {
            break;
        }
        let (haystack, map) = if normalized {
            whitespace_map(text)
        } else {
            (text.to_owned(), Vec::new())
        };
        let needle = if normalized {
            whitespace_map(quote).0
        } else {
            quote.into()
        };
        if needle.is_empty() {
            continue;
        }
        let mut locations = Vec::new();
        let mut from = 0;
        while let Some(relative) = haystack[from..].find(&needle) {
            let s = from + relative;
            let e = s + needle.len();
            let (original_start, original_end) = if normalized {
                (map[s].0, map[e - 1].1)
            } else {
                (s, e)
            };
            let start = text[..original_start].encode_utf16().count() as u32;
            let end = text[..original_end].encode_utf16().count() as u32;
            if let Some((boxes, geometry_levels)) = geometry.support(start, end) {
                let geometry_level = if geometry_levels
                    .iter()
                    .all(|level| level == &geometry_levels[0])
                {
                    geometry_levels[0].clone()
                } else {
                    "mixed".into()
                };
                locations.push(CiteSupport {
                    observed_text: text[original_start..original_end].into(),
                    start,
                    end,
                    boxes,
                    geometry_level,
                    geometry_levels,
                });
                // Presence, not uniqueness: output bounded locations.
                if locations.len() == 8 {
                    break;
                }
            }
            from = s + haystack[s..].chars().next().unwrap().len_utf8();
        }
        if !locations.is_empty() {
            return CiteDecision {
                verdict: if normalized {
                    "verified_normalized"
                } else {
                    "verified_exact"
                }
                .into(),
                locations,
            };
        }
    }
    let geometry_complete = geometry.is_complete();
    CiteDecision {
        verdict: if complete && !text.trim().is_empty() && geometry_complete {
            "unmatched"
        } else {
            "insufficient_evidence"
        }
        .into(),
        locations: Vec::new(),
    }
}

// Each normalized UTF-8 byte maps to the complete original character/run.
fn whitespace_map(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut output = String::new();
    let mut map = Vec::new();
    let mut pending = None;
    for (start, ch) in text.char_indices() {
        let end = start + ch.len_utf8();
        if ch.is_whitespace() {
            pending = Some((pending.map_or(start, |(s, _)| s), end));
        } else {
            if !output.is_empty() {
                if let Some(span) = pending.take() {
                    output.push(' ');
                    map.push(span);
                }
            }
            pending = None;
            output.push(ch);
            map.extend(std::iter::repeat_n((start, end), ch.len_utf8()));
        }
    }
    (output, map)
}
