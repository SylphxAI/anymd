//! Citation chunks built from element projections.

use serde_json::{json, Value};

const DEFAULT_CHUNK_MAX_UTF16: u64 = 1_800;

#[derive(Debug)]
struct ChunkDraft {
    page_start: u32,
    page_end: u32,
    text_parts: Vec<String>,
    element_ids: Vec<String>,
    bounding_boxes: Vec<Value>,
    strategy: &'static str,
    heading: Option<String>,
    utf16_with_separators: u64,
}

fn chunk_element_text(element: &Value) -> Option<String> {
    match element.get("type").and_then(Value::as_str) {
        Some("text") => element
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        Some("table") => {
            let text = element
                .pointer("/table/rows")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|row| {
                    row.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then(|| text.trim().to_string())
        }
        _ => None,
    }
}

fn finalize_chunk(draft: ChunkDraft, index: usize) -> Option<Value> {
    let text = draft.text_parts.join("\n").trim().to_string();
    if text.is_empty() {
        return None;
    }
    let id = if draft.page_start == draft.page_end {
        format!("p{}-chunk-{index}", draft.page_start)
    } else {
        format!("p{}-p{}-chunk-{index}", draft.page_start, draft.page_end)
    };
    let mut chunk = json!({
        "id": id,
        "page_start": draft.page_start,
        "page_end": draft.page_end,
        "text": text,
        "element_ids": draft.element_ids,
        "strategy": draft.strategy,
    });
    if let Some(heading) = draft.heading {
        chunk["heading"] = json!(heading);
    }
    if !draft.bounding_boxes.is_empty() {
        chunk["bounding_boxes"] = json!(draft.bounding_boxes);
    }
    Some(chunk)
}

fn push_current_chunk(current: &mut Option<ChunkDraft>, chunks: &mut Vec<Value>) {
    if let Some(draft) = current.take() {
        if let Some(chunk) = finalize_chunk(draft, chunks.len() + 1) {
            chunks.push(chunk);
        }
    }
}

/// Build the v3.0.14 citation-chunk projection from already ordered elements.
/// The builder is one-pass, preserves repeated IDs and present boxes, and uses
/// JavaScript UTF-16 length semantics for the 1,800-unit size boundary.
pub fn build_citation_chunks(elements: &Value, use_semantic_boundaries: bool) -> Value {
    let mut chunks = Vec::new();
    let mut current: Option<ChunkDraft> = None;

    for element in elements.as_array().into_iter().flatten() {
        let Some(text) = chunk_element_text(element) else {
            continue;
        };
        let page = element.get("page").and_then(Value::as_u64).unwrap_or(0) as u32;
        let is_table = element.get("type").and_then(Value::as_str) == Some("table");
        let is_heading = use_semantic_boundaries
            && element
                .pointer("/semantic_hint/role")
                .and_then(Value::as_str)
                == Some("heading");
        let text_utf16 = text.encode_utf16().count() as u64;
        let exceeds_size = current.as_ref().is_some_and(|draft| {
            !draft.element_ids.is_empty()
                && draft
                    .utf16_with_separators
                    .checked_add(text_utf16)
                    .is_none_or(|total| total > DEFAULT_CHUNK_MAX_UTF16)
        });
        let crosses_page = current.as_ref().is_some_and(|draft| draft.page_end != page);

        if is_heading || is_table || exceeds_size || crosses_page {
            push_current_chunk(&mut current, &mut chunks);
        }

        if current.is_none() {
            current = Some(ChunkDraft {
                page_start: page,
                page_end: page,
                text_parts: Vec::new(),
                element_ids: Vec::new(),
                bounding_boxes: Vec::new(),
                strategy: if is_heading {
                    "semantic"
                } else if exceeds_size {
                    "size"
                } else {
                    "page"
                },
                heading: is_heading.then(|| text.clone()),
                utf16_with_separators: 0,
            });
        }

        let draft = current.as_mut().expect("chunk draft exists");
        if is_table && draft.element_ids.is_empty() {
            draft.strategy = "table";
        }
        draft.page_end = draft.page_end.max(page);
        draft.utf16_with_separators = draft
            .utf16_with_separators
            .saturating_add(text_utf16.saturating_add(1));
        draft.text_parts.push(text);
        draft.element_ids.push(
            element
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        );
        if let Some(box_) = element.get("bounding_box") {
            draft.bounding_boxes.push(box_.clone());
        }

        if is_table {
            push_current_chunk(&mut current, &mut chunks);
        }
    }

    push_current_chunk(&mut current, &mut chunks);
    json!(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citation_chunks_match_ts_boundaries_schema_and_utf16_size() {
        let box_ = json!({"left": 1, "bottom": 2, "right": 3, "top": 4});
        let first = "x".repeat(1_799);
        let elements = json!([
            {"id":"p1-text-1","type":"text","page":1,"content":"Intro","bounding_box":box_},
            {"id":"p1-text-2","type":"text","page":1,"content":"Heading","semantic_hint":{"role":"heading"}},
            {"id":"p1-table-1","type":"table","page":1,"table":{"rows":[["A","B"],["1","2"]]},"bounding_box":box_},
            {"id":"p2-text-1","type":"text","page":2,"content":first},
            {"id":"p2-text-2","type":"text","page":2,"content":"😀"}
        ]);

        let chunks = build_citation_chunks(&elements, true);
        assert_eq!(chunks.as_array().map(Vec::len), Some(5));
        assert_eq!(chunks[0]["id"], "p1-chunk-1");
        assert_eq!(chunks[0]["strategy"], "page");
        assert_eq!(chunks[0]["element_ids"], json!(["p1-text-1"]));
        assert_eq!(chunks[0]["bounding_boxes"], json!([box_]));
        assert_eq!(chunks[1]["strategy"], "semantic");
        assert_eq!(chunks[1]["heading"], "Heading");
        assert_eq!(chunks[2]["strategy"], "table");
        assert_eq!(chunks[2]["text"], "A | B\n1 | 2");
        assert_eq!(chunks[2]["bounding_boxes"], json!([box_]));
        assert_eq!(chunks[3]["id"], "p2-chunk-4");
        assert_eq!(chunks[3]["strategy"], "page");
        assert_eq!(chunks[4]["id"], "p2-chunk-5");
        assert_eq!(chunks[4]["strategy"], "size");
        assert_eq!(chunks[4]["text"], "😀");
    }

    #[test]
    fn chunk_builder_ignores_empty_non_content_and_preserves_repeated_ids() {
        let elements = json!([
            {"id":"same","type":"text","page":4,"content":"  "},
            {"id":"image","type":"image","page":4},
            {"id":"same","type":"text","page":4,"content":"One"},
            {"id":"same","type":"text","page":4,"content":"Two"}
        ]);
        let chunks = build_citation_chunks(&elements, false);
        assert_eq!(chunks[0]["text"], "One\nTwo");
        assert_eq!(chunks[0]["element_ids"], json!(["same", "same"]));
        assert!(chunks[0].get("bounding_boxes").is_none());
    }
}
