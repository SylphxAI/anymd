//! Text-layer metrics at page and document scope.

use serde_json::{json, Value};

use super::sources::{array, page_number};

pub(super) struct TextLayer<'a> {
    pages: &'a [Value],
    summary: Option<&'a Value>,
}

impl<'a> TextLayer<'a> {
    pub(super) fn new(text_layer: &'a Value) -> Self {
        Self {
            pages: text_layer.get("pages").map(array).unwrap_or_default(),
            summary: text_layer.get("summary"),
        }
    }

    pub(super) fn has_pages(&self) -> bool {
        !self.pages.is_empty()
    }

    pub(super) fn summary_metric(&self, key: &str) -> Value {
        metric(self.summary, key)
    }

    pub(super) fn project_page(&self, page: u32, value: &mut Value) {
        let Some((index, layer_page)) = self
            .pages
            .iter()
            .enumerate()
            .find(|(_, entry)| page_number(entry) == Some(u64::from(page)))
        else {
            return;
        };
        let lines = layer_page.get("lines").map(array).unwrap_or_default();
        let items = |key| {
            lines
                .iter()
                .flat_map(move |line| line.get(key).map(array).unwrap_or_default())
        };
        let run_count = |key| items("runs").filter(|run| run.get(key).is_some()).count();
        let boxes = |key| {
            items(key)
                .filter(|item| item.get("bounding_box").is_some())
                .count()
        };

        value["text_layer_page_index"] = json!(index);
        value["text_layer_run_count"] = json!(items("runs").count());
        value["text_layer_line_count"] = metric(Some(layer_page), "line_count");
        value["text_layer_word_count"] = metric(Some(layer_page), "word_count");
        value["text_layer_char_count"] = metric(Some(layer_page), "char_count");
        value["text_layer_runs_with_bounding_boxes"] = json!(boxes("runs"));
        value["text_layer_lines_with_bounding_boxes"] = json!(lines
            .iter()
            .filter(|line| line.get("bounding_box").is_some())
            .count());
        value["text_layer_words_with_bounding_boxes"] = json!(boxes("words"));
        value["text_layer_chars_with_bounding_boxes"] = json!(boxes("chars"));
        value["text_layer_runs_with_font_metadata"] = json!(run_count("font_name"));
        value["text_layer_runs_with_direction_metadata"] = json!(run_count("direction"));
        value["text_layer_runs_with_transform_metadata"] = json!(run_count("transform"));
        value["text_layer_runs_with_eol_metadata"] = json!(run_count("has_eol"));
    }
}

fn metric(value: Option<&Value>, key: &str) -> Value {
    value
        .and_then(|value| value.get(key))
        .cloned()
        .unwrap_or_else(|| json!(0))
}
