use crate::bounded_markdown_table;
use crate::ooxml::{Blocks, Element};

use super::super::extras::chart_markdown;
use super::{paragraph, Context};

impl Context<'_> {
    pub(super) fn graphic_frame(&self, shape: &Element, blocks: &mut Blocks) {
        let Some(data) = shape.path(&["graphic", "graphicData"]) else {
            return;
        };
        if let Some(table) = data.child("tbl") {
            blocks.push(&self.table(table), false);
        } else if let Some(chart) = data.find("chart") {
            let target = chart
                .rel_attr("id")
                .and_then(|id| self.rels.get(id))
                .map(|r| r.target.as_str());
            if let Some(root) = target.and_then(|t| self.charts.get(t)) {
                match chart_markdown(root) {
                    Ok(markdown) => blocks.push(&markdown, false),
                    Err(error) => *self.failure.borrow_mut() = Some(error),
                }
            }
        }
    }

    pub(super) fn picture(&self, shape: &Element, blocks: &mut Blocks) {
        let alt = shape
            .path(&["nvPicPr", "cNvPr"])
            .and_then(|c| c.attr("descr"))
            .unwrap_or("");
        let target = shape
            .find("blip")
            .and_then(|b| b.rel_attr("embed"))
            .and_then(|id| self.rels.get(id))
            .map(|r| r.target.as_str());
        let place = format!("slide {}", self.slide);
        if let Some(image) = self.media.markdown(alt, target, Some(&place)) {
            blocks.push(&image, false);
        }
    }

    fn table(&self, table: &Element) -> String {
        let mut rows: Vec<Vec<String>> = Vec::new();
        for tr in table.children_named("tr") {
            let row = tr
                .children_named("tc")
                .map(|tc| {
                    if tc.attr("hMerge").is_some_and(|v| v == "1" || v == "true")
                        || tc.attr("vMerge").is_some_and(|v| v == "1" || v == "true")
                    {
                        return String::new();
                    }
                    tc.child("txBody")
                        .map(|body| {
                            body.children_named("p")
                                .map(|p| paragraph(p, self.rels).render(true))
                                .filter(|t| !t.trim().is_empty())
                                .collect::<Vec<_>>()
                                .join("<br>")
                                .replace('\n', "<br>")
                        })
                        .unwrap_or_default()
                })
                .collect();
            rows.push(row);
        }
        rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
        bounded_markdown_table(&rows).unwrap_or_else(|error| {
            *self.failure.borrow_mut() = Some(error);
            String::new()
        })
    }
}
