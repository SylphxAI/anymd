//! Bounded inspect cite-check orchestration over the existing text/OCR owners.
use std::collections::BTreeSet;
use std::io::Read;
use std::time::{Duration, Instant};

use anymd_core::text_index::cite_check::{
    check_cite_items, check_cite_items_with_separator, valid_cite_box, CiteNormalization,
};
use anymd_core::text_index::{PositionedTextItem, TextBoundingBox};
use rmcp::{model::CallToolResult, ErrorData};
use serde_json::{json, Value};

use crate::command_provider::{self, CommandInvocation};
use crate::schema::{Citation, InspectArgs};
use crate::source_access::SourceAccessPolicy;

#[derive(serde::Serialize, serde::Deserialize)]
struct WorkerRequest {
    args: InspectArgs,
    ocr_admitted: bool,
}

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const SCOPE: &str = "Quote/location support in extracted evidence; not semantic truth or independent confirmation of OCR accuracy.";

fn bounds(citation: &Citation) -> TextBoundingBox {
    let b = &citation.bounding_box;
    TextBoundingBox {
        left: b.left,
        bottom: b.bottom,
        right: b.right,
        top: b.top,
    }
}

fn validate(args: &InspectArgs) -> Result<(), String> {
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

pub(crate) async fn inspect(
    mut args: InspectArgs,
    policy: SourceAccessPolicy,
) -> Result<CallToolResult, ErrorData> {
    let started = Instant::now();
    validate(&args).map_err(|e| ErrorData::invalid_params(e, None))?;
    policy
        .admit_evidence_sources(&mut args.sources)
        .map_err(|e| ErrorData::invalid_params(e, None))?;
    // The parent process holds the existing OCR permit across the supervised
    // worker, so independent cite-check workers cannot evade server admission.
    // A busy OCR owner still permits native positives to be checked.
    let permit = if args.ocr.is_some() {
        crate::ocr_evidence::OcrRequestPermit::acquire().ok()
    } else {
        None
    };
    let ocr_admitted = permit.is_some();
    let timeout = u64::from(args.timeout_ms.unwrap_or(60000));
    let deadline = started + Duration::from_millis(timeout);
    // Parent owns all temporary artifacts, even if the supervised worker is killed.
    let directory =
        tempfile::tempdir().map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    let request = directory.path().join("request.json");
    std::fs::write(
        &request,
        serde_json::to_vec(&WorkerRequest {
            args: args.clone(),
            ocr_admitted,
        })
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    )
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    let maximum = args.max_output_chars.unwrap_or(200000) as usize;
    let executable =
        std::env::current_exe().map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    let result = tokio::task::spawn_blocking(move || {
        let _directory = directory;
        let _permit = permit;
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if remaining == 0 {
            return Err("cite_check deadline exhausted.".into());
        }
        command_provider::run_supervised(CommandInvocation {
            command: executable.to_string_lossy().into(),
            args: vec![
                "__cite-check-worker".into(),
                request.to_string_lossy().into(),
            ],
            timeout_ms: remaining,
            max_stdout_bytes: maximum * 4,
            failure_message: "cite_check worker failed (invalid PDF, extraction or output limit)."
                .into(),
            timeout_message: "cite_check deadline exhausted.".into(),
        })
        .map_err(|e| e.message)
    })
    .await
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    match result {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text).map_err(|e| {
                ErrorData::internal_error(format!("Invalid cite-check worker output: {e}"), None)
            })?;
            Ok(CallToolResult::structured(value))
        }
        Err(reason) if reason.contains("deadline") => {
            let value = insufficient(&args, &reason, None);
            if value.to_string().encode_utf16().count() > maximum {
                Ok(CallToolResult::error(vec![
                    rmcp::model::ContentBlock::text(
                        "Citation result envelope exceeds max_output_chars.",
                    ),
                ]))
            } else {
                Ok(CallToolResult::structured(value))
            }
        }
        Err(reason) => Ok(CallToolResult::error(vec![
            rmcp::model::ContentBlock::text(reason),
        ])),
    }
}

fn insufficient(args: &InspectArgs, reason: &str, hash: Option<&str>) -> Value {
    json!({"scope": SCOPE, "source_hash": hash, "results": args.citations.as_deref().unwrap_or_default().iter().map(|c| json!({"id": c.id, "page": c.page, "verdict": "insufficient_evidence", "reason": reason, "locations": []})).collect::<Vec<_>>()})
}

/// Internal entry point only. Supervision is installed before PDF parsing,
/// network fetching, rendering or model execution; native work is cancellable
/// by terminating this existing command-provider process boundary.
pub fn worker(arguments: &[String]) -> Result<(), String> {
    let request = arguments.first().ok_or("Missing cite-check request.")?;
    let timeout = arguments
        .iter()
        .find_map(|a| a.strip_prefix("--supervised="))
        .ok_or("cite-check worker needs supervision.")?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    command_provider::supervise_parent(timeout)?;
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let request_options: WorkerRequest =
        serde_json::from_slice(&std::fs::read(request).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let args = request_options.args;
    validate(&args)?;
    // Snapshot once: the hash, native extraction and OCR consume identical bytes,
    // even when a caller edits its local file while this request is in flight.
    // URL bytes come from the existing SSRF-safe owner without an independent
    // temporary file that could survive worker termination.
    let bytes = if let Some(path) = &args.sources[0].path {
        if !std::fs::metadata(path)
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("PDF source must be a regular file.".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        bytes
    } else {
        anymd_core::url_fetch::fetch_url(
            args.sources[0]
                .url
                .as_deref()
                .ok_or("Missing PDF source.")?,
        )?
        .bytes
    };
    if bytes.len() as u64 > MAX_BYTES {
        return Err("PDF exceeds 256 MiB.".into());
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err("cite_check accepts PDF sources only.".into());
    }
    let snapshot = std::path::Path::new(request).with_file_name("source.pdf");
    std::fs::write(&snapshot, bytes).map_err(|e| e.to_string())?;
    let hash = anymd_core::hash_file(&snapshot, MAX_BYTES)
        .map_err(|e| e.message)?
        .source_hash;
    let mut output = if args
        .expected_source_sha256
        .as_ref()
        .is_some_and(|expected| !expected.eq_ignore_ascii_case(&hash))
    {
        insufficient(
            &args,
            "Expected source SHA-256 does not match admitted PDF.",
            Some(&hash),
        )
    } else {
        evaluate(
            &args,
            &snapshot,
            &hash,
            deadline,
            request_options.ocr_admitted,
        )?
    };
    let maximum = args.max_output_chars.unwrap_or(200000) as usize;
    // Never truncate text/geometry and leave a verified label attached.
    if output.to_string().encode_utf16().count() > maximum {
        output = insufficient(
            &args,
            "Citation output exceeds max_output_chars; supporting geometry was not returned.",
            Some(&hash),
        );
    }
    let serialized = output.to_string();
    if serialized.encode_utf16().count() > maximum {
        return Err("Citation result envelope exceeds max_output_chars.".into());
    }
    println!("{serialized}");
    Ok(())
}

fn evaluate(
    args: &InspectArgs,
    snapshot: &std::path::Path,
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
    let mut ocr_pages = Vec::new();
    let mut ocr_failed = args.ocr.is_some();
    if let Some(engine) = args.ocr.filter(|_| ocr_admitted) {
        let pdf_source = crate::schema::PdfSource {
            path: Some(snapshot.to_string_lossy().into()),
            url: None,
            pages: None,
        };
        if let Ok(source) = crate::visual_evidence::materialize_read_source(0, &pdf_source) {
            let outcomes = crate::ocr_evidence::run_read_ocr(
                &[(&source, requested_pages)],
                crate::ocr_evidence::ReadOcrOptions {
                    engine: Some(engine),
                    deadline: Some(deadline),
                    exact_geometry: true,
                    max_pages: 20,
                    timeout_ms: Some(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis() as u64,
                    ),
                    max_output_chars: args.max_output_chars.unwrap_or(200000) as usize,
                    ..Default::default()
                },
            );
            for outcome in outcomes {
                ocr_failed = outcome.error.is_some();
                ocr_pages.extend(outcome.pages);
            }
        }
    }
    let results = citations.iter().map(|citation| {
        let native = extracted.get(&citation.page).and_then(|p| p.as_ref().ok());
        let native_items = native.map_or(&[][..], |p| p.positioned_items.as_slice());
        let mut decision = check_cite_items(native_items, &citation.quote, bounds(citation), normalization, !ocr_failed, "text_item");
        let mut layer = json!({"kind": "native_text"});
        if decision.verdict != "verified_exact" {
            if let Some(page) = ocr_pages.iter().find(|p| p.page == citation.page) {
                let items = ocr_items(page);
                let level = if page.words.as_ref().is_some_and(|words| words.iter().any(|w| w.region_type.is_some())) { "ocr_region" } else { "ocr_word" };
                let complete = !ocr_failed && page.warnings.as_ref().is_none_or(Vec::is_empty) && items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join("") == page.text;
                let levels = page.words.as_deref().unwrap_or_default().iter().filter(|word| !word.text.is_empty()).take(items.len()).map(|word| if word.region_type.is_some() { "ocr_region" } else { "ocr_word" }).collect::<Vec<_>>();
                let ocr = check_cite_items_with_separator(&items, &citation.quote, bounds(citation), normalization, complete, level, "", Some(&levels));
                let native_verified = decision.verdict.starts_with("verified");
                if ocr.verdict == "verified_exact" || (!native_verified && (ocr.verdict.starts_with("verified") || native.is_some_and(|page| page.text.trim().is_empty()) || ocr.verdict == "insufficient_evidence")) { decision = ocr; layer = json!({"kind": "ocr", "provider": page.provider, "provenance": page.provenance, "render_evidence_id": page.source_render_evidence_id}); }
            } else if args.ocr.is_some() && !decision.verdict.starts_with("verified") {
                decision.verdict = "insufficient_evidence".into();
            }
        }
        json!({"id": citation.id, "page": citation.page, "source_hash": hash, "verdict": decision.verdict, "evidence_layer": layer, "locations": decision.locations})
    }).collect::<Vec<_>>();
    Ok(
        json!({"scope": SCOPE, "source_hash": hash, "normalization": args.normalization.unwrap_or_default(), "results": results}),
    )
}

fn ocr_items(page: &anymd_core::OcrPage) -> Vec<PositionedTextItem> {
    // Map the ordered provider words/regions to its actual original page text.
    // Only whitespace gaps are admissible. A disconnected region stops coverage;
    // an earlier, fully supported contiguous positive still stands.
    let mut items = Vec::new();
    let mut cursor = 0;
    for word in page.words.as_deref().unwrap_or_default() {
        if word.text.is_empty() {
            continue;
        }
        let Some(relative) = page.text[cursor..].find(&word.text) else {
            break;
        };
        let start = cursor + relative;
        if !page.text[cursor..start].chars().all(char::is_whitespace) {
            break;
        }
        let end = start + word.text.len();
        items.push(PositionedTextItem {
            text: page.text[cursor..end].into(),
            bounding_box: word
                .bounding_box
                .clone()
                .and_then(|v| serde_json::from_value(v).ok()),
            chars: Vec::new(),
            runs: Vec::new(),
        });
        cursor = end;
    }
    if page.text[cursor..].chars().all(char::is_whitespace) {
        if let Some(last) = items.last_mut() {
            last.text.push_str(&page.text[cursor..]);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args() -> InspectArgs {
        serde_json::from_value(json!({"operation":"cite_check", "sources":[{"path":"fixture.pdf"}], "citations":[{"quote":"quote", "page":1,"bounding_box":{"left":0,"bottom":0,"right":10,"top":10}}]})).unwrap()
    }
    #[test]
    fn cite_check_ocr_fixture_never_assembles_disconnected_words() {
        let page: anymd_core::OcrPage = serde_json::from_value(json!({
            "page":1,"text":"first missing second", "provider":"fixture",
            "source_render_evidence_id":"fixture", "provenance":{},
            "words":[{"text":"first", "bounding_box":{"left":0,"bottom":0,"right":10,"top":10}},
                     {"text":"second", "bounding_box":{"left":0,"bottom":0,"right":10,"top":10}}]
        }))
        .unwrap();
        let items = ocr_items(&page);
        assert_eq!(items.len(), 1);
        let bbox = bounds(&args().citations.unwrap()[0]);
        assert_eq!(
            check_cite_items_with_separator(
                &items,
                "first",
                bbox,
                CiteNormalization::None,
                false,
                "ocr_word",
                "",
                None
            )
            .verdict,
            "verified_exact"
        );
        assert_eq!(
            check_cite_items_with_separator(
                &items,
                "first second",
                bbox,
                CiteNormalization::WhitespaceV1,
                false,
                "ocr_word",
                "",
                None
            )
            .verdict,
            "insufficient_evidence"
        );
    }

    #[test]
    fn cite_check_bounds_rejected() {
        let mut value = args();
        assert!(validate(&value).is_ok());
        value.citations = Some(vec![args().citations.unwrap().remove(0); 101]);
        assert!(validate(&value).is_err());
        value = args();
        value.citations.as_mut().unwrap()[0].quote.clear();
        assert!(validate(&value).is_err());
        value = args();
        value.citations = Some(vec![
            {
                let mut c = args().citations.unwrap().remove(0);
                c.quote = "x".repeat(4096);
                c
            };
            16
        ]);
        assert!(validate(&value).is_err());
        value = args();
        value.citations.as_mut().unwrap()[0].quote = "😀".repeat(2049);
        assert!(validate(&value).is_err());
        value = args();
        value.citations.as_mut().unwrap()[0].bounding_box.right = 0.;
        assert!(validate(&value).is_err());
        value = args();
        value.citations.as_mut().unwrap()[0].page = 0;
        assert!(validate(&value).is_err());
        value = args();
        value.expected_source_sha256 = Some("bad".into());
        assert!(validate(&value).is_err());
        value = args();
        value.citations = Some(
            (1..=21)
                .map(|page| {
                    let mut c = args().citations.unwrap().remove(0);
                    c.page = page;
                    c
                })
                .collect(),
        );
        assert!(validate(&value).is_err());
    }
}
