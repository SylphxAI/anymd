use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

use anymd_core::text_index::cite_check::{check_cite_items, CiteNormalization};
use serde_json::Value;

use crate::schema::InspectArgs;

use super::{evidence, request::bounds, response, source::MAX_BYTES};

pub(super) fn evaluate(
    args: &InspectArgs,
    snapshot: &Path,
    hash: &str,
    deadline: Instant,
    ocr_admitted: bool,
) -> Result<Value, String> {
    let citations = args.citations.as_deref().unwrap_or_default();
    let requested_pages = citations
        .iter()
        .map(|c| c.page)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let extracted =
        match anymd_core::text_index::extract_cite_pages(snapshot, MAX_BYTES, &requested_pages) {
            Ok(text) => text,
            Err(error) => return Err(error.message),
        };
    let normalization = match args.normalization.unwrap_or_default() {
        crate::schema::CiteNormalization::None => CiteNormalization::None,
        crate::schema::CiteNormalization::WhitespaceV1 => CiteNormalization::WhitespaceV1,
    };
    let (ocr_pages, ocr_failed) =
        evidence::ocr_pages(args, snapshot, requested_pages, deadline, ocr_admitted);
    let results = citations
        .iter()
        .map(|citation| {
            let native = extracted.get(&citation.page).and_then(|p| p.as_ref().ok());
            let native_items = native.map_or(&[][..], |p| p.positioned_items.as_slice());
            let mut decision = check_cite_items(
                native_items,
                &citation.quote,
                bounds(citation),
                normalization,
                !ocr_failed,
                "text_item",
            );
            let mut layer = response::native_layer();
            if decision.verdict != "verified_exact" {
                if let Some(page) = ocr_pages.iter().find(|p| p.page == citation.page) {
                    let ocr = evidence::check_ocr(page, citation, normalization, ocr_failed);
                    let native_verified = decision.verdict.starts_with("verified");
                    if ocr.verdict == "verified_exact"
                        || (!native_verified
                            && (ocr.verdict.starts_with("verified")
                                || native.is_some_and(|page| page.text.trim().is_empty())
                                || ocr.verdict == "insufficient_evidence"))
                    {
                        decision = ocr;
                        layer = response::ocr_layer(page);
                    }
                } else if args.ocr.is_some() && !decision.verdict.starts_with("verified") {
                    decision.verdict = "insufficient_evidence".into();
                }
            }
            response::citation_result(citation, hash, decision, layer)
        })
        .collect();
    Ok(response::evaluated(args, hash, results))
}
