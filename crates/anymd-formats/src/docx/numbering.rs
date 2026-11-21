//! Word numbering definitions and the running counters for list markers.

use std::collections::HashMap;

use crate::ooxml::Element;

pub(super) fn num_pr(numpr: &Element) -> Option<(String, usize)> {
    let id = numpr
        .child("numId")
        .and_then(|n| n.attr("val"))?
        .to_string();
    let level = numpr
        .child("ilvl")
        .and_then(|n| n.attr("val"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    Some((id, level))
}

#[derive(Clone)]
struct Level {
    format: String,
    start: u32,
}

#[derive(Default)]
pub(super) struct Numbering {
    /// numId → (abstractNumId, per-level start overrides)
    nums: HashMap<String, (String, HashMap<usize, u32>)>,
    abstracts: HashMap<String, HashMap<usize, Level>>,
    /// abstractNumId → running counter per level (None = not started).
    counters: HashMap<String, [Option<i64>; 9]>,
    started_nums: Vec<String>,
}

impl Numbering {
    pub(super) fn parse(root: &Element) -> Self {
        let mut numbering = Self::default();
        for abs in root.children_named("abstractNum") {
            let Some(id) = abs.attr("abstractNumId") else {
                continue;
            };
            let mut levels = HashMap::new();
            for lvl in abs.children_named("lvl") {
                let Some(ilvl) = lvl.attr("ilvl").and_then(|v| v.parse().ok()) else {
                    continue;
                };
                levels.insert(ilvl, level_of(lvl));
            }
            numbering.abstracts.insert(id.to_string(), levels);
        }
        for num in root.children_named("num") {
            let (Some(id), Some(abs)) = (
                num.attr("numId"),
                num.child("abstractNumId").and_then(|a| a.attr("val")),
            ) else {
                continue;
            };
            let mut overrides = HashMap::new();
            for o in num.children_named("lvlOverride") {
                let Some(ilvl) = o.attr("ilvl").and_then(|v| v.parse().ok()) else {
                    continue;
                };
                let start = o
                    .child("startOverride")
                    .and_then(|s| s.attr("val"))
                    .and_then(|v| v.parse().ok())
                    .or_else(|| o.child("lvl").map(|l| level_of(l).start));
                if let Some(start) = start {
                    overrides.insert(ilvl, start);
                }
            }
            numbering
                .nums
                .insert(id.to_string(), (abs.to_string(), overrides));
        }
        numbering
    }

    fn level(&self, num_id: &str, ilvl: usize) -> Option<(&str, Level)> {
        let (abs, _) = self.nums.get(num_id)?;
        let level = self
            .abstracts
            .get(abs)
            .and_then(|l| l.get(&ilvl))
            .cloned()
            .unwrap_or(Level {
                format: "bullet".into(),
                start: 1,
            });
        Some((abs.as_str(), level))
    }

    /// The Markdown marker for a numbered paragraph, advancing Word's counters.
    pub(super) fn list_marker(&mut self, num_id: &str, ilvl: usize) -> Option<String> {
        if num_id == "0" {
            return None;
        }
        let (abs, level) = self.level(num_id, ilvl)?;
        let abs = abs.to_string();
        match level.format.as_str() {
            "none" => return None,
            "bullet" => return Some("-".into()),
            _ => {}
        }
        let first_use = !self.started_nums.iter().any(|n| n == num_id);
        let counters = self.counters.entry(abs).or_insert([None; 9]);
        if first_use {
            self.started_nums.push(num_id.to_string());
            // A num with startOverride restarts the shared abstract list.
            if let Some((_, overrides)) = self.nums.get(num_id) {
                for (&lvl, &start) in overrides {
                    if lvl < 9 {
                        counters[lvl] = Some(i64::from(start) - 1);
                    }
                }
            }
        }
        let value = counters[ilvl].map_or(i64::from(level.start), |c| c + 1);
        counters[ilvl] = Some(value);
        for deeper in counters.iter_mut().skip(ilvl + 1) {
            *deeper = None;
        }
        Some(format!("{value}."))
    }
}

fn level_of(lvl: &Element) -> Level {
    Level {
        format: lvl
            .child("numFmt")
            .and_then(|f| f.attr("val"))
            .unwrap_or("decimal")
            .to_string(),
        start: lvl
            .child("start")
            .and_then(|s| s.attr("val"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1),
    }
}
