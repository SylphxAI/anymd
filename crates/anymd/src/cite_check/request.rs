use std::collections::BTreeSet;

use anymd_core::text_index::cite_check::valid_cite_box;
use anymd_core::text_index::TextBoundingBox;

use crate::schema::{Citation, InspectArgs};

const FULL_PAGE_BOUNDS: TextBoundingBox = TextBoundingBox {
    left: -1.0e9,
    bottom: -1.0e9,
    right: 1.0e9,
    top: 1.0e9,
};

pub(super) fn bounds(citation: &Citation) -> TextBoundingBox {
    match &citation.bounding_box {
        Some(b) => TextBoundingBox {
            left: b.left,
            bottom: b.bottom,
            right: b.right,
            top: b.top,
        },
        None => FULL_PAGE_BOUNDS,
    }
}

/// Normalize a citation so callers can pass output directly from `read`
/// (`<!-- page N -->` markers) or `search` (`- p.N: …**match**…` lines)
/// without hand-computing PDF bounding boxes.
pub fn normalize_citation(citation: &mut Citation) {
    if let Some(start) = citation.quote.find("<!-- page ") {
        let after = &citation.quote[start + "<!-- page ".len()..];
        if let Some(end) = after.find(" -->") {
            if let Ok(parsed_page) = after[..end].trim().parse::<u32>() {
                if citation.page == 0 {
                    citation.page = parsed_page;
                }
                let mut cleaned = String::new();
                cleaned.push_str(&citation.quote[..start]);
                cleaned.push_str(&after[end + 4..]);
                let trimmed = cleaned.trim_matches(['\r', '\n']);
                if !trimmed.is_empty() {
                    citation.quote = trimmed.to_string();
                }
            }
        }
    }
    let candidate_lines: Vec<String> = citation.quote.lines().map(str::to_string).collect();
    for line in candidate_lines {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("- p.")
            .or_else(|| trimmed.strip_prefix("p."));
        if let Some(after_p) = rest {
            if let Some((digits, snippet)) = after_p.split_once(':') {
                if let Ok(parsed_page) = digits.trim().parse::<u32>() {
                    if citation.page == 0 {
                        citation.page = parsed_page;
                    }
                    let cleaned = snippet
                        .trim()
                        .trim_matches('…')
                        .trim_start_matches("...")
                        .trim_end_matches("...")
                        .replace("**", "");
                    let cleaned = cleaned.trim();
                    if !cleaned.is_empty() {
                        citation.quote = cleaned.to_string();
                        break;
                    }
                }
            }
        }
    }
}

pub fn normalize_args(args: &mut InspectArgs) {
    if let Some(citations) = args.citations.as_mut() {
        for citation in citations {
            normalize_citation(citation);
        }
    }
}

pub(super) fn validate(args: &InspectArgs) -> Result<(), String> {
    if args.sources.len() != 1 {
        return Err("cite_check needs exactly one PDF source.".into());
    }
    args.sources[0].validate()?;
    if args.sources[0].pages.is_some() || args.sources[0].regions.is_some() {
        return Err("cite_check page and bounding_box belong in citations, not sources.".into());
    }
    let citations = args
        .citations
        .as_deref()
        .ok_or("cite_check needs citations.")?;
    if !(1..=100).contains(&citations.len()) {
        return Err("citations must contain 1–100 entries.".into());
    }
    let mut total = 0;
    let mut pages = BTreeSet::new();
    for citation in citations {
        let units = citation.quote.encode_utf16().count();
        if units == 0 || units > 4096 {
            return Err("Each quote must contain 1–4096 UTF-16 units.".into());
        }
        total += units;
        if citation.page == 0 || !valid_cite_box(bounds(citation)) {
            return Err(
                "Each citation needs a positive page and a finite, positive-area bounding_box."
                    .into(),
            );
        }
        pages.insert(citation.page);
    }
    if total > 64000 || pages.len() > 20 {
        return Err(
            "cite_check permits at most 64000 quoted UTF-16 units and 20 distinct pages.".into(),
        );
    }
    if args
        .expected_source_sha256
        .as_ref()
        .is_some_and(|hash| hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("expected_source_sha256 must be 64 hexadecimal digits.".into());
    }
    if args
        .timeout_ms
        .is_some_and(|n| !(1000..=300000).contains(&n))
    {
        return Err("timeout_ms must be 1000–300000.".into());
    }
    if args
        .max_output_chars
        .is_some_and(|n| !(1000..=1000000).contains(&n))
    {
        return Err("max_output_chars must be 1000–1000000.".into());
    }
    Ok(())
}
