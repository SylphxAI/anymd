use std::collections::HashMap;

use crate::ooxml::{self, Element, Package, Rels};
use crate::{bounded_markdown_table, ConvertError};

use super::slide::{paragraph, placeholder};

pub(super) fn notes_text(notes: &Element, rels: &Rels) -> String {
    let mut lines = Vec::new();
    let mut shapes = Vec::new();
    if let Some(tree) = notes.path(&["cSld", "spTree"]) {
        tree.find_all("sp", &mut shapes);
    }
    for shape in shapes {
        let is_body = placeholder(shape).is_some_and(|p| p.attr("type") == Some("body"));
        if !is_body {
            continue;
        }
        if let Some(body) = shape.child("txBody") {
            for p in body.children_named("p") {
                let text = paragraph(p, rels).render(true);
                if !text.trim().is_empty() {
                    lines.push(text);
                }
            }
        }
    }
    lines.join("\n")
}

// Chart parts are read up front so shape rendering needs no package access.
pub(super) fn load_charts(
    package: &mut Package<'_>,
    slide: &Element,
    rels: &Rels,
) -> HashMap<String, Element> {
    let mut charts = HashMap::new();
    let mut chart_refs = Vec::new();
    slide.find_all("chart", &mut chart_refs);
    for chart in chart_refs {
        if let Some(target) = chart
            .rel_attr("id")
            .and_then(|id| rels.get(id))
            .map(|r| r.target.clone())
        {
            if let Some(root) = package.xml(&target).ok().flatten() {
                charts.insert(target, root);
            }
        }
    }
    charts
}

/// A chart's cached series data as a heading plus a Markdown table.
pub(super) fn chart_markdown(space: &Element) -> Result<String, ConvertError> {
    let chart = space.child("chart").unwrap_or(space);
    let title = chart
        .child("title")
        .map(|t| {
            let mut runs = Vec::new();
            t.find_all("t", &mut runs);
            ooxml::collapse_ws(&runs.iter().map(|r| r.text()).collect::<Vec<_>>().join(""))
        })
        .filter(|t| !t.is_empty());
    let mut series = Vec::new();
    chart.find_all("ser", &mut series);
    let mut names = Vec::new();
    let mut columns: Vec<HashMap<usize, String>> = Vec::new();
    let mut categories: HashMap<usize, String> = HashMap::new();
    let mut count = 0;
    for (index, ser) in series.iter().enumerate() {
        let name = ser
            .child("tx")
            .and_then(|tx| tx.find("v"))
            .map(Element::text)
            .unwrap_or_else(|| format!("Series {}", index + 1));
        names.push(name);
        if let Some(cat) = ser.child("cat").or_else(|| ser.child("xVal")) {
            for (idx, value) in points(cat) {
                count = count.max(idx + 1);
                categories.entry(idx).or_insert(value);
            }
        }
        let values: HashMap<usize, String> = ser
            .child("val")
            .or_else(|| ser.child("yVal"))
            .map(points)
            .unwrap_or_default()
            .into_iter()
            .collect();
        count = count.max(values.keys().max().map_or(0, |m| m + 1));
        columns.push(values);
    }
    let heading = format!("### Chart: {}", title.as_deref().unwrap_or("untitled"));
    if names.is_empty() || count == 0 {
        return Ok(heading);
    }
    let mut rows = vec![std::iter::once("Category".to_string())
        .chain(names)
        .collect::<Vec<_>>()];
    for idx in 0..count.min(2000) {
        let mut row = vec![categories.get(&idx).cloned().unwrap_or_default()];
        row.extend(
            columns
                .iter()
                .map(|c| c.get(&idx).cloned().unwrap_or_default()),
        );
        rows.push(row);
    }
    Ok(format!(
        "{heading}\n\n{}",
        bounded_markdown_table(&rows)?.trim_end()
    ))
}

fn points(data: &Element) -> Vec<(usize, String)> {
    let mut pts = Vec::new();
    data.find_all("pt", &mut pts);
    pts.iter()
        .filter_map(|pt| {
            let idx = pt.attr("idx")?.parse().ok()?;
            Some((idx, pt.child("v").map(Element::text).unwrap_or_default()))
        })
        .collect()
}
