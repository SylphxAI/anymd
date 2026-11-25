use crate::lean::result_text;
use crate::source_access::SourceAccessPolicy;

use super::args::Parsed;
use super::output::CommandOutput;

pub(super) fn run(parsed: &Parsed, policy: &SourceAccessPolicy) -> Result<CommandOutput, i32> {
    if let Err(required) = crate::pro::require_pro("Cite-check") {
        eprintln!("{required}");
        return Err(1);
    }
    let Some((pdf, quotes)) = parsed.inputs.split_first() else {
        eprintln!(
            "anymd: cite-check needs a PDF path/URL and at least one quote (or read/search output)"
        );
        return Err(2);
    };
    if quotes.is_empty() {
        eprintln!("anymd: cite-check needs at least one quote (or read/search output)");
        return Err(2);
    }
    let page = parsed
        .pages
        .as_deref()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    let citations: Vec<crate::schema::Citation> = quotes
        .iter()
        .map(|q| crate::schema::Citation {
            id: None,
            quote: q.clone(),
            page,
            bounding_box: None,
        })
        .collect();
    let is_url = pdf.starts_with("http://") || pdf.starts_with("https://");
    let inspect_args = crate::schema::InspectArgs {
        citations: Some(citations),
        normalization: Some(crate::schema::CiteNormalization::WhitespaceV1),
        operation: crate::schema::InspectOperation::CiteCheck,
        timeline: None,
        timestamps_ms: None,
        expected_source_sha256: None,
        transcript: None,
        sources: vec![crate::schema::PdfEvidenceSource {
            path: (!is_url).then(|| pdf.clone()),
            url: is_url.then(|| pdf.clone()),
            pages: None,
            regions: None,
        }],
        profile: None,
        sample_pages: None,
        include_metadata: None,
        scale: None,
        max_pages: None,
        max_regions: None,
        max_pixels_per_page: None,
        include_image: None,
        timeout_ms: None,
        max_output_chars: None,
        languages: None,
        ocr: None,
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("anymd: {e}");
            return Err(1);
        }
    };
    match rt.block_on(crate::cite_check::inspect(inspect_args, policy.clone())) {
        Ok(res) => Ok(CommandOutput {
            text: format!("{}\n", result_text(&res)),
            failed: res.is_error == Some(true),
            evidence: None,
        }),
        Err(err) => {
            eprintln!("anymd: {}", err.message);
            Err(2)
        }
    }
}
