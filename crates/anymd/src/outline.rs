//! Deterministic navigation over the same Markdown units used by read and search.
use crate::document::{OpenOptions, Opened};
use crate::schema::OutlineArgs;
use crate::source_access::SourceAccessPolicy;
pub use anymd_core::outline::{Node, Outline, UnitRange};
use rmcp::model::{CallToolResult, ContentBlock};

pub fn render(args: &OutlineArgs, policy: &SourceAccessPolicy) -> Result<String, String> {
    render_with_evidence(args, policy).map(|(text, _)| text)
}

pub(crate) fn render_with_evidence(
    args: &OutlineArgs,
    policy: &SourceAccessPolicy,
) -> Result<(String, Option<crate::document::EvidenceKind>), String> {
    if !matches!(args.format.as_deref(), None | Some("json" | "tree")) {
        return Err("format must be json or tree".into());
    }
    if !matches!(args.images.as_deref(), None | Some("none" | "refs")) {
        return Err("images must be none or refs".into());
    }
    let revisions = match args.revisions.as_deref() {
        Some(value) => anymd_formats::Revisions::parse(value)
            .ok_or("revisions must be markup, accept or reject")?,
        None => anymd_formats::Revisions::default(),
    };
    let environment = if matches!(args.ocr, Some(crate::ocr_vlm::OcrSelection::Enabled(true))) {
        match std::env::var("ANYMD_OCR") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err("Invalid ANYMD_OCR".into()),
        }
    } else {
        None
    };
    let ocr_engine = crate::ocr_vlm::resolve_engine(args.ocr, environment.as_deref())?;
    let options = OpenOptions {
        ocr: Some(args.ocr.is_some_and(|selection| selection.enabled())),
        ocr_engine: Some(ocr_engine),
        images: if args.images.as_deref() == Some("refs") {
            anymd_formats::images::ImageStore::default_location()
        } else {
            None
        },
        revisions,
        ..Default::default()
    };
    let opened = if let Some(selection) = &args.timeline {
        let mut metadata_only = selection.clone();
        metadata_only.caption = false;
        let evidence =
            crate::video_request::document(&args.source, &metadata_only, false, None, policy)?;
        Opened::from_video(&args.source, &evidence)
    } else {
        Opened::open(&args.source, policy, &options)?
    };
    let outline = opened.build_outline()?;
    let text = if args.format.as_deref() == Some("tree") {
        outline.tree()
    } else {
        serde_json::to_string_pretty(&outline).map_err(|e| e.to_string())?
    };
    Ok((text, opened.evidence_kind(opened.total as usize)))
}

pub fn tool(
    args: &OutlineArgs,
    policy: &SourceAccessPolicy,
) -> Result<CallToolResult, rmcp::ErrorData> {
    render(args, policy)
        .map(|text| CallToolResult::success(vec![ContentBlock::text(text)]))
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_accepts_legacy_booleans_and_named_ocr() {
        use crate::ocr_vlm::{OcrEngine, OcrSelection};
        for (value, expected) in [
            (serde_json::json!(false), OcrSelection::Enabled(false)),
            (serde_json::json!(true), OcrSelection::Enabled(true)),
            (
                serde_json::json!("vlm"),
                OcrSelection::Engine(OcrEngine::Vlm),
            ),
            (
                serde_json::json!("tesseract"),
                OcrSelection::Engine(OcrEngine::Tesseract),
            ),
        ] {
            let args: OutlineArgs = serde_json::from_value(serde_json::json!({
                "source": "sample.pdf", "ocr": value
            }))
            .unwrap();
            assert_eq!(args.ocr, Some(expected));
        }
        let args: OutlineArgs = serde_json::from_value(serde_json::json!({
            "source": "sample.pdf"
        }))
        .unwrap();
        assert!(args.ocr.is_none());
    }

    #[test]
    fn existing_format_fixtures_have_readable_stable_nodes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures");
        for file in [
            "sample.pdf",
            "differential/v3014-behavior-v1.pdf",
            "alt-text.docx",
            "slides.pptx",
            "field-notes.epub",
        ] {
            let opened = Opened::open(
                root.join(file).to_str().unwrap(),
                &SourceAccessPolicy::default(),
                &OpenOptions {
                    ocr: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
            let outline = opened.build_outline().unwrap();
            // Build the canonical body only in the test; the runtime index stores
            // lengths and offsets, not a second copy of the whole document.
            let mut canonical = String::new();
            for unit in opened.all_units().unwrap().iter() {
                if opened.total > 1 || opened.is_paged() {
                    canonical.push_str(&format!("<!-- {} -->\n\n", unit.label));
                }
                canonical.push_str(unit.markdown.trim());
                canonical.push_str("\n\n");
            }
            assert!(outline.nodes.len() > 1, "{file}");
            if file == "differential/v3014-behavior-v1.pdf" {
                let group = outline
                    .nodes
                    .iter()
                    .find(|n| n.title == "External docs")
                    .unwrap();
                assert_eq!((group.from, group.to, group.children), (3, 3, 1));
                let child = outline
                    .nodes
                    .iter()
                    .find(|n| n.title == "Page three")
                    .unwrap();
                assert_eq!(child.path, "External docs > Page three");
            }
            assert_eq!(outline.tree(), opened.build_outline().unwrap().tree());
            for node in &outline.nodes {
                assert!(
                    node.start <= node.end && node.end <= canonical.len(),
                    "{file}: {}",
                    node.id
                );
                assert!(
                    canonical.is_char_boundary(node.start) && canonical.is_char_boundary(node.end)
                );
                assert!(node.from <= node.to);
            }
        }
    }
}
