//! Search entry points and Markdown listing of matches with document locators.

use rmcp::model::CallToolResult;

use crate::document::{EvidenceKind, Unit};
use crate::schema::{PdfSource, SearchArgs, SearchPdfArgs};
use crate::source_access::SourceAccessPolicy;

use super::finish;

pub(super) mod matching;
mod sources;

use matching::{literal_hits, ranked_hits, Hit};
use sources::{expand_sources, load_search_docs, SearchDoc};

/// `search`: literal or ranked search across files, directories, and URLs.
pub fn search(
    args: &SearchArgs,
    policy: &SourceAccessPolicy,
) -> Result<CallToolResult, rmcp::ErrorData> {
    search_with_evidence(args, policy).map(|(result, _)| result)
}

pub(crate) fn search_with_evidence(
    args: &SearchArgs,
    policy: &SourceAccessPolicy,
) -> Result<(CallToolResult, Option<EvidenceKind>), rmcp::ErrorData> {
    let mode = args.mode.as_deref().unwrap_or("auto").to_ascii_lowercase();
    if !matches!(mode.as_str(), "auto" | "literal" | "ranked") {
        return Err(rmcp::ErrorData::invalid_params(
            "mode must be auto, literal, or ranked.",
            None,
        ));
    }
    let sources = if args.sources.is_empty() {
        vec![".".to_string()]
    } else {
        args.sources.clone()
    };
    run_search(
        &sources,
        &args.query,
        &mode,
        SearchOptions {
            case_sensitive: args.case_sensitive.unwrap_or(false),
            whole_word: args.whole_word.unwrap_or(false),
            context: args.context_chars.unwrap_or(80) as usize,
            max_results: args.max_results.unwrap_or(20).clamp(1, 500) as usize,
            glob: args.glob.clone(),
        },
        policy,
    )
}

/// Legacy `search_pdf` default route: literal search with page locators.
pub fn search_pdf(
    args: &SearchPdfArgs,
    policy: &SourceAccessPolicy,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let specs: Vec<String> = args.sources.iter().map(PdfSource::label).collect();
    run_search(
        &specs,
        &args.query,
        "literal",
        SearchOptions {
            case_sensitive: args.case_sensitive.unwrap_or(false),
            whole_word: args.whole_word.unwrap_or(false),
            context: args.context_chars.unwrap_or(80) as usize,
            max_results: args.max_matches_per_source.unwrap_or(20) as usize,
            glob: None,
        },
        policy,
    )
    .map(|(result, _)| result)
}

/// True when search needs the geometry/OCR JSON route.
pub fn search_wants_legacy(args: &SearchPdfArgs) -> bool {
    args.include_ocr_text_layer == Some(true) || args.detail == Some(true)
}

struct SearchOptions {
    case_sensitive: bool,
    whole_word: bool,
    context: usize,
    max_results: usize,
    glob: Option<String>,
}

fn run_search(
    specs: &[String],
    query: &str,
    mode: &str,
    options: SearchOptions,
    policy: &SourceAccessPolicy,
) -> Result<(CallToolResult, Option<EvidenceKind>), rmcp::ErrorData> {
    let query = query.trim();
    if query.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "query must not be empty.",
            None,
        ));
    }
    let mut notes = Vec::new();
    let files = expand_sources(specs, policy, options.glob.as_deref(), &mut notes)
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    if files.is_empty() {
        let mut out = format!("No searchable documents found in {}.\n", specs.join(", "));
        for note in notes {
            out.push_str(&format!("- {note}\n"));
        }
        return Ok((finish(out, true), None));
    }
    let docs = load_search_docs(files, policy, &mut notes);
    let evidence = docs.iter().find_map(|doc| doc.evidence);
    let units_searched: usize = docs.iter().map(|d| d.units.len()).sum();
    let scope = format!(
        "{} file{}, {} section{} searched",
        docs.len(),
        if docs.len() == 1 { "" } else { "s" },
        units_searched,
        if units_searched == 1 { "" } else { "s" }
    );
    let single = docs.len() == 1;
    let mut out = String::new();
    let mut literal_total = 0;
    if mode != "ranked" {
        let (per_doc, total) = literal_hits(
            &docs,
            query,
            options.case_sensitive,
            options.whole_word,
            options.context,
            options.max_results,
        );
        literal_total = total;
        if total > 0 || mode == "literal" {
            out.push_str(&format!(
                "{total} match{} for \"{query}\" ({scope})\n",
                if total == 1 { "" } else { "es" }
            ));
            let mut shown = 0;
            for (_, count, hits) in &per_doc {
                if shown >= options.max_results {
                    break;
                }
                let doc = hits[0].doc;
                if !single {
                    out.push_str(&format!("\n### {} ({count})\n", doc.label));
                }
                for hit in hits.iter().take(options.max_results - shown) {
                    let loc = hit_locator(hit);
                    let loc = loc.trim_start();
                    if loc.is_empty() {
                        out.push_str(&format!("- {}\n", hit.snippet));
                    } else {
                        out.push_str(&format!("- {loc}: {}\n", hit.snippet));
                    }
                    shown += 1;
                }
                if *count > hits.len() {
                    out.push_str(&format!("- … {} more in this file\n", count - hits.len()));
                }
            }
        }
    }
    if mode == "ranked" || (mode == "auto" && literal_total == 0) {
        let ranked = ranked_hits(&docs, query, options.context, options.max_results);
        if mode == "auto" {
            out.push_str(&format!("No exact match for \"{query}\" ({scope}). "));
        }
        if ranked.is_empty() {
            out.push_str("No passages contain any of the query words.\n");
        } else {
            out.push_str(&format!(
                "Best matching passages (BM25, top {}):\n",
                ranked.len()
            ));
            for (rank, (score, hit)) in ranked.iter().enumerate() {
                let file = if single {
                    String::new()
                } else {
                    format!("{} ", hit.doc.label)
                };
                let loc = hit_locator(hit);
                out.push_str(&format!(
                    "{}. {}{} (score {:.1}): {}\n",
                    rank + 1,
                    file,
                    loc.trim_start(),
                    score,
                    hit.snippet
                ));
            }
        }
    }
    if !notes.is_empty() {
        out.push_str("\n<!-- skipped: ");
        out.push_str(&notes.join("; "));
        out.push_str(" -->\n");
    }
    Ok((finish(out, docs.is_empty()), evidence))
}

fn hit_locator(hit: &Hit<'_>) -> String {
    let node = hit.doc.outline.node_at(hit.unit.number, hit.offset);
    format!(
        "{} [node {}; {}]",
        locator(hit.doc, hit.unit),
        node.id,
        node.path
    )
}

fn locator(doc: &SearchDoc, unit: &Unit) -> String {
    if doc.total <= 1 && doc.noun != "page" {
        String::new()
    } else if doc.noun == "page" {
        format!(" p.{}", unit.number)
    } else {
        format!(" {}", unit.label)
    }
}
