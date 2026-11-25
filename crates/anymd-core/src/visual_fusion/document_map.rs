use serde_json::{json, Map, Value};

fn insert_layer_after(layers: &mut Vec<Value>, layer: &str, after: &str) {
    let layer_value = json!(layer);
    if layers.iter().any(|entry| entry.as_str() == Some(layer)) {
        return;
    }
    if let Some(index) = layers
        .iter()
        .position(|entry| entry.as_str() == Some(after))
    {
        layers.insert(index + 1, layer_value);
    } else {
        layers.push(layer_value);
    }
}

fn remove_layer(layers: &mut Vec<Value>, layer: &str) {
    layers.retain(|entry| entry.as_str() != Some(layer));
}

fn kind_counts(enrichments: &[Value]) -> Map<String, Value> {
    let mut counts = Map::new();
    for enrichment in enrichments {
        let kind = enrichment
            .get("target_element_type")
            .and_then(Value::as_str)
            .unwrap_or("visual_region");
        let entry = counts.entry(kind.to_string()).or_insert(json!(0));
        if let Some(value) = entry.as_u64() {
            *entry = json!(value + 1);
        }
    }
    counts
}

pub(super) fn fuse_document_map(
    map: &mut Value,
    enrichments: &[Value],
    warnings: &Option<Vec<String>>,
) {
    let Some(object) = map.as_object_mut() else {
        return;
    };

    let layers = object
        .entry("layers")
        .or_insert_with(|| json!([]))
        .as_array_mut();
    if let Some(layers) = layers {
        if enrichments.is_empty() {
            remove_layer(layers, "visual_enrichment");
        } else {
            insert_layer_after(layers, "visual_enrichment", "visual_region_candidates");
        }
    }

    let mut indexes_by_page: std::collections::BTreeMap<u64, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, enrichment) in enrichments.iter().enumerate() {
        if let Some(page) = enrichment.get("page").and_then(Value::as_u64) {
            indexes_by_page.entry(page).or_default().push(index);
        }
    }

    if let Some(map_pages) = object.get_mut("pages").and_then(Value::as_array_mut) {
        for map_page in map_pages {
            let Some(page_number) = map_page.get("page").and_then(Value::as_u64) else {
                continue;
            };
            let Some(page_object) = map_page.as_object_mut() else {
                continue;
            };
            let indexes = indexes_by_page
                .get(&page_number)
                .cloned()
                .unwrap_or_default();
            page_object.insert("visual_enrichment_indexes".into(), json!(indexes));
            page_object.insert(
                "visual_enrichment_count".into(),
                json!(indexes_by_page.get(&page_number).map_or(0, Vec::len)),
            );
        }
    }

    object.insert("visual_enrichments".into(), json!(enrichments));

    let summary = object.entry("summary").or_insert_with(|| json!({}));
    if let Some(summary) = summary.as_object_mut() {
        summary.insert("visual_enrichment_count".into(), json!(enrichments.len()));
        summary.insert(
            "visual_enrichment_kind_counts".into(),
            Value::Object(kind_counts(enrichments)),
        );
    }

    match warnings {
        Some(values) if !values.is_empty() => {
            object.insert("warnings".into(), json!(values));
        }
        _ => {
            object.remove("warnings");
        }
    }
}
