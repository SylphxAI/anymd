//! Element projection: selectable-text elements plus table and image elements in reading order.

use serde_json::{json, Value};

use super::semantic::{page_semantic_bounds, page_semantic_stats, semantic_hint};
use super::PageText;

#[cfg(test)]
pub(crate) fn build_elements(pages: &[PageText], semantic_hints: bool) -> Value {
    build_elements_with_geometry(pages, semantic_hints, None)
}

pub fn build_elements_with_geometry(
    pages: &[PageText],
    semantic_hints: bool,
    page_geometry: Option<&Value>,
) -> Value {
    let mut elements = Vec::new();
    let geometry_by_page = page_semantic_bounds(page_geometry);
    for page in pages {
        let page_no = page.page;
        let stats = page_semantic_stats(page, geometry_by_page.get(&page_no).copied());
        let mut element_index = 0usize;
        let lines = if page.positioned_items.is_empty() {
            page.text
                .lines()
                .map(|line| (line, None))
                .collect::<Vec<_>>()
        } else {
            let mut positioned = page.positioned_items.iter().enumerate().collect::<Vec<_>>();
            positioned.sort_by(|(left_index, left), (right_index, right)| {
                match (left.bounding_box, right.bounding_box) {
                    (Some(left_box), Some(right_box)) => right_box
                        .top
                        .total_cmp(&left_box.top)
                        .then_with(|| left_box.left.total_cmp(&right_box.left))
                        .then_with(|| left_index.cmp(right_index)),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => left_index.cmp(right_index),
                }
            });
            positioned
                .iter()
                .map(|(_, item)| (item.text.as_str(), item.bounding_box))
                .collect::<Vec<_>>()
        };
        for (line, bounding_box) in lines {
            let content = line.trim();
            if content.is_empty() {
                continue;
            }
            element_index += 1;
            let mut element = json!({
                "id": format!("p{page_no}-text-{element_index}"),
                "type": "text",
                "page": page_no,
                "content": content,
            });
            if let Some(box_) = bounding_box {
                element["bounding_box"] = json!(box_);
                element["provenance"] = json!({
                    "engine": "pdf-reader-core",
                    "source": "selectable-text",
                });
            }
            if semantic_hints {
                element["semantic_hint"] =
                    serde_json::to_value(semantic_hint(content, bounding_box, stats))
                        .expect("semantic hint serializes");
            }
            elements.push(element);
        }
    }
    json!(elements)
}

pub fn build_elements_with_tables_and_geometry(
    pages: &[PageText],
    tables: &Value,
    semantic_hints: bool,
    page_geometry: Option<&Value>,
) -> Value {
    build_elements_with_tables_images_and_geometry(
        pages,
        tables,
        &json!([]),
        semantic_hints,
        page_geometry,
    )
}

pub fn build_elements_with_tables_images_and_geometry(
    pages: &[PageText],
    tables: &Value,
    images: &Value,
    semantic_hints: bool,
    page_geometry: Option<&Value>,
) -> Value {
    let base = build_elements_with_geometry(pages, semantic_hints, page_geometry);
    let mut base_by_page = std::collections::BTreeMap::<u32, Vec<Value>>::new();
    for element in base.as_array().into_iter().flatten() {
        let page = element.get("page").and_then(Value::as_u64).unwrap_or(0) as u32;
        base_by_page.entry(page).or_default().push(element.clone());
    }

    let mut tables_by_page = std::collections::BTreeMap::<u32, Vec<Value>>::new();
    for table in tables.as_array().into_iter().flatten() {
        let page = table.get("page").and_then(Value::as_u64).unwrap_or(0) as u32;
        tables_by_page.entry(page).or_default().push(table.clone());
    }
    for page_tables in tables_by_page.values_mut() {
        page_tables
            .sort_by_key(|table| table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0));
    }

    let mut images_by_page = std::collections::BTreeMap::<u32, Vec<Value>>::new();
    for image in images.as_array().into_iter().flatten() {
        let page = image.get("page").and_then(Value::as_u64).unwrap_or(0) as u32;
        images_by_page.entry(page).or_default().push(image.clone());
    }
    for page_images in images_by_page.values_mut() {
        page_images.sort_by_key(|image| image.get("index").and_then(Value::as_u64).unwrap_or(0));
    }

    let mut elements = Vec::new();
    for page in pages {
        let page_elements = base_by_page.remove(&page.page).unwrap_or_default();
        let first_image_index = page_elements.len() + 1;
        elements.extend(page_elements);
        for (offset, mut image) in images_by_page
            .remove(&page.page)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let element_index = first_image_index + offset;
            let bounding_box = image.get("bounding_box").cloned();
            if let Some(object) = image.as_object_mut() {
                object.remove("data");
                object.remove("bounding_box");
            }
            let mut element = json!({
                "id": format!("p{}-image-{element_index}", page.page),
                "type": "image",
                "page": page.page,
                "image": image,
                "provenance": {
                    "engine": "pdf-reader-core",
                    "source": "image-xobject",
                },
            });
            if let Some(box_) = bounding_box {
                element["bounding_box"] = box_;
            }
            elements.push(element);
        }
        for table in tables_by_page.remove(&page.page).unwrap_or_default() {
            let table_index = table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0);
            let ocr = table.pointer("/provenance/source").and_then(Value::as_str)
                == Some("ocr_text_layer");
            let mut provenance = if ocr {
                json!({"engine":"external-command","source":"ocr-table-detector"})
            } else {
                json!({"engine":"pdf-reader-core","source":"table-detector"})
            };
            if ocr {
                if let Some(evidence_id) = table
                    .pointer("/provenance/ocr_source_render_evidence_id")
                    .cloned()
                {
                    provenance["ocr_source_render_evidence_id"] = evidence_id;
                }
            }
            let confidence = table.get("confidence").cloned().unwrap_or(Value::Null);
            let bounding_box = table.get("bounding_box").cloned();
            let mut nested_table = table.clone();
            if let Some(object) = nested_table.as_object_mut() {
                object.remove("page");
                object.remove("tableIndex");
            }
            let mut element = json!({
                "id": format!("p{}-table-{}", page.page, table_index + 1),
                "type": "table",
                "page": page.page,
                "table": nested_table,
                "confidence": confidence,
                "provenance": provenance,
            });
            if let Some(box_) = bounding_box {
                element["bounding_box"] = box_;
            }
            elements.push(element);
        }
    }
    for (_, remaining) in base_by_page {
        elements.extend(remaining);
    }
    for (page, remaining) in tables_by_page {
        for table in remaining {
            let table_index = table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0);
            let mut nested_table = table;
            if let Some(object) = nested_table.as_object_mut() {
                object.remove("page");
                object.remove("tableIndex");
            }
            elements.push(json!({
                "id": format!("p{page}-table-{}", table_index + 1),
                "type":"table",
                "page":page,
                "table":nested_table,
            }));
        }
    }
    json!(elements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_twin::*;
    use crate::text_index::{PositionedTextItem, TextBoundingBox};

    #[test]
    fn element_projection_uses_stable_pdfjs_reading_order() {
        let item = |text: &str, left: f64, bottom: f64| PositionedTextItem {
            text: text.into(),
            bounding_box: Some(TextBoundingBox {
                left,
                bottom,
                right: left + 40.0,
                top: bottom + 10.0,
            }),
            chars: Vec::new(),
            runs: Vec::new(),
        };
        let pages = vec![PageText {
            page: 1,
            text: String::new(),
            positioned_items: vec![
                item("bottom", 72.0, 36.0),
                item("right", 200.0, 55.0),
                item("left", 72.0, 55.0),
            ],
        }];
        let elements = build_elements(&pages, false);
        assert_eq!(elements[0]["id"], "p1-text-1");
        assert_eq!(elements[0]["content"], "left");
        assert_eq!(elements[1]["content"], "right");
        assert_eq!(elements[2]["content"], "bottom");
    }
}
