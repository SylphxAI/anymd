//! Style inheritance for headings, numbering, and emphasis.

use std::collections::HashMap;

use super::numbering::num_pr;
use crate::ooxml::Element;

#[derive(Default, Clone)]
struct Style {
    name: String,
    based_on: Option<String>,
    outline: Option<usize>,
    num: Option<(String, usize)>,
    bold: Option<bool>,
    italic: Option<bool>,
}

#[derive(Default)]
pub(super) struct Styles {
    by_id: HashMap<String, Style>,
}

impl Styles {
    pub(super) fn parse(root: &Element) -> Self {
        let mut by_id = HashMap::new();
        for style in root.children_named("style") {
            let Some(id) = style.attr("styleId") else {
                continue;
            };
            let ppr = style.child("pPr");
            let rpr = style.child("rPr");
            by_id.insert(
                id.to_string(),
                Style {
                    name: style
                        .path(&["name"])
                        .and_then(|n| n.attr("val"))
                        .unwrap_or(id)
                        .to_ascii_lowercase(),
                    based_on: style
                        .path(&["basedOn"])
                        .and_then(|n| n.attr("val"))
                        .map(str::to_string),
                    outline: ppr
                        .and_then(|p| p.child("outlineLvl"))
                        .and_then(|o| o.attr("val"))
                        .and_then(|v| v.parse().ok()),
                    num: ppr.and_then(|p| p.child("numPr")).and_then(num_pr),
                    bold: rpr.and_then(|r| r.toggle("b")),
                    italic: rpr.and_then(|r| r.toggle("i")),
                },
            );
        }
        Self { by_id }
    }

    /// The style and its `basedOn` ancestors, nearest first.
    fn chain(&self, id: &str) -> Vec<&Style> {
        let mut out = Vec::new();
        let mut current = Some(id.to_string());
        while let Some(id) = current {
            if out.len() >= 16 {
                break;
            }
            match self.by_id.get(&id) {
                Some(style) => {
                    out.push(style);
                    current = style.based_on.clone();
                }
                None => break,
            }
        }
        out
    }

    pub(super) fn heading_level(&self, id: &str) -> Option<usize> {
        for style in self.chain(id) {
            if style.name == "title" {
                return Some(1);
            }
            if let Some(n) = style
                .name
                .strip_prefix("heading ")
                .and_then(|n| n.trim().parse::<usize>().ok())
            {
                return Some(n.clamp(1, 6));
            }
            if let Some(level) = style.outline {
                return (level < 9).then_some((level + 1).min(6));
            }
        }
        // Unknown style ids that still follow the built-in naming.
        let lower = id.to_ascii_lowercase();
        if lower == "title" {
            return Some(1);
        }
        lower
            .strip_prefix("heading")
            .and_then(|n| n.parse::<usize>().ok())
            .map(|n| n.clamp(1, 6))
    }

    pub(super) fn num(&self, id: &str) -> Option<(String, usize)> {
        self.chain(id).into_iter().find_map(|s| s.num.clone())
    }

    pub(super) fn emphasis(&self, id: &str) -> (Option<bool>, Option<bool>) {
        let chain = self.chain(id);
        (
            chain.iter().find_map(|s| s.bold),
            chain.iter().find_map(|s| s.italic),
        )
    }
}
