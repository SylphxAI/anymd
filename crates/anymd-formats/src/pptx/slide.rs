use std::cell::RefCell;
use std::collections::HashMap;

use crate::ooxml::{Blocks, Element, Inline, ListIndent, Media, Package, Rels};
use crate::{bounded_markdown_table, ConvertError};

use super::extras::{chart_markdown, load_charts, notes_text};
use super::{layout_positions, Positions, SlideImages};

pub(super) fn render_slide(
    package: &mut Package<'_>,
    path: &str,
    slide: &Element,
    layouts: &mut HashMap<String, Positions>,
    images: &SlideImages<'_>,
) -> Result<String, ConvertError> {
    let rels = package.rels(path)?;
    let layout_key = rels
        .first_of_type("/slideLayout")
        .map(|r| r.target.clone())
        .unwrap_or_default();
    if !layouts.contains_key(&layout_key) {
        let positions = layout_positions(package, &rels)?;
        layouts.insert(layout_key.clone(), positions);
    }
    let positions = layouts.get(&layout_key).cloned().unwrap_or_default();

    let charts = load_charts(package, slide, &rels);

    let media = Media::load(package, &rels, images.store, images.repeated);
    package.limits.check()?;
    let context = Context {
        failure: RefCell::new(None),
        rels: &rels,
        positions: &positions,
        charts: &charts,
        media: &media,
        slide: images.number,
    };
    let mut blocks = Blocks::new();
    if let Some(tree) = slide.path(&["cSld", "spTree"]) {
        context.tree(tree, &mut blocks);
    }

    if let Some(notes_path) = rels.first_of_type("/notesSlide").map(|r| r.target.clone()) {
        if let Some(notes) = package.xml(&notes_path).ok().flatten() {
            let notes_rels = package.rels(&notes_path)?;
            let text = notes_text(&notes, &notes_rels);
            if !text.is_empty() {
                blocks.push(&format!("> Notes: {}", text.replace('\n', "\n> ")), false);
            }
        }
    }
    package.limits.check()?;
    if let Some(error) = context.failure.into_inner() {
        return Err(error);
    }
    let mut markdown = blocks.finish()?;
    if !markdown.is_empty() {
        markdown.push('\n');
    }
    Ok(markdown)
}

struct Context<'a> {
    failure: RefCell<Option<ConvertError>>,
    rels: &'a Rels,
    positions: &'a Positions,
    charts: &'a HashMap<String, Element>,
    media: &'a Media,
    slide: usize,
}

/// A shape to render, with its reading position when known.
struct Item<'e> {
    element: &'e Element,
    title: bool,
    position: Option<(i64, i64)>,
}

impl Context<'_> {
    fn tree(&self, tree: &Element, blocks: &mut Blocks) {
        let mut items = Vec::new();
        self.collect(tree, &mut items);
        // Titles first, then reading order (top-to-bottom, left-to-right) when every shape has a position.
        let all_placed = items.iter().all(|i| i.position.is_some());
        items.sort_by_key(|i| {
            (
                !i.title,
                if all_placed {
                    i.position.map(|(y, x)| (y / 50_000, x))
                } else {
                    None
                },
            )
        });
        for item in items {
            if blocks.is_full() || self.failure.borrow().is_some() {
                break;
            }
            self.shape(item.element, item.title, blocks);
        }
    }

    fn collect<'e>(&self, tree: &'e Element, items: &mut Vec<Item<'e>>) {
        for element in tree.elements() {
            match element.local() {
                "sp" | "grpSp" | "graphicFrame" | "pic" => {
                    let ph = placeholder(element);
                    let kind = ph.map(|p| p.attr("type").unwrap_or("obj"));
                    if matches!(kind, Some("dt" | "ftr" | "sldNum" | "hdr")) {
                        continue;
                    }
                    let title = matches!(kind, Some("title" | "ctrTitle"));
                    let position =
                        offset(element).or_else(|| ph.and_then(|p| self.positions.lookup(p)));
                    items.push(Item {
                        element,
                        title,
                        position,
                    });
                }
                "AlternateContent" => {
                    if let Some(choice) = element
                        .child("Choice")
                        .or_else(|| element.child("Fallback"))
                    {
                        self.collect(choice, items);
                    }
                }
                _ => {}
            }
        }
    }

    fn shape(&self, shape: &Element, title: bool, blocks: &mut Blocks) {
        match shape.local() {
            "grpSp" => self.tree(shape, blocks),
            "sp" => {
                let Some(body) = shape.child("txBody") else {
                    return;
                };
                if title {
                    let text: Vec<String> = body
                        .children_named("p")
                        .map(|p| paragraph(p, self.rels).render(false).replace('\n', " "))
                        .filter(|t| !t.trim().is_empty())
                        .collect();
                    if !text.is_empty() {
                        blocks.push(&format!("## {}", text.join(" ")), false);
                    }
                } else {
                    let kind = placeholder(shape).map(|p| p.attr("type").unwrap_or("obj"));
                    let bulleted = matches!(kind, Some("body" | "obj"));
                    self.text_body(body, bulleted, blocks);
                }
            }
            "graphicFrame" => {
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
            "pic" => {
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
            _ => {}
        }
    }

    fn text_body(&self, body: &Element, bulleted_default: bool, blocks: &mut Blocks) {
        let mut list = ListIndent::default();
        let mut counters: [Option<u32>; 9] = [None; 9];
        for p in body.children_named("p") {
            if blocks.is_full() {
                break;
            }
            let inline = paragraph(p, self.rels);
            if inline.is_blank() {
                continue;
            }
            let ppr = p.child("pPr");
            let level: usize = ppr
                .and_then(|pr| pr.attr("lvl"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
                .min(8);
            let explicit_none = ppr.is_some_and(|pr| pr.child("buNone").is_some());
            let auto = ppr.and_then(|pr| pr.child("buAutoNum"));
            let bullet =
                ppr.is_some_and(|pr| pr.child("buChar").is_some() || pr.child("buBlip").is_some());
            let text = inline.render(true);
            if let Some(auto) = auto.filter(|_| !explicit_none) {
                let start = auto
                    .attr("startAt")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                let value = counters[level].map_or(start, |c| c + 1);
                counters[level] = Some(value);
                counters.iter_mut().skip(level + 1).for_each(|c| *c = None);
                blocks.push(&list.item(level, &format!("{value}."), &text), true);
            } else if !explicit_none && (bullet || bulleted_default) {
                counters.iter_mut().skip(level).for_each(|c| *c = None);
                blocks.push(&list.item(level, "-", &text), true);
            } else {
                list.reset();
                counters = [None; 9];
                blocks.push(&text, false);
            }
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

pub(super) fn paragraph(p: &Element, rels: &Rels) -> Inline {
    let mut inline = Inline::default();
    runs(p, &mut inline, rels);
    inline
}

fn runs(container: &Element, inline: &mut Inline, rels: &Rels) {
    for child in container.elements() {
        match child.local() {
            "r" | "fld" => {
                let rpr = child.child("rPr");
                let on = |name: &str| {
                    rpr.and_then(|r| r.attr(name))
                        .is_some_and(|v| v == "1" || v == "true")
                };
                let link = rpr
                    .and_then(|r| r.child("hlinkClick"))
                    .and_then(|h| h.rel_attr("id"))
                    .and_then(|id| rels.link(id));
                let text = child.child("t").map(Element::text).unwrap_or_default();
                inline.push(&text, on("b"), on("i"), link.as_deref());
            }
            "br" => inline.push("\n", false, false, None),
            "AlternateContent" => {
                if let Some(choice) = child.child("Choice").or_else(|| child.child("Fallback")) {
                    runs(choice, inline, rels);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn placeholder(shape: &Element) -> Option<&Element> {
    let nv = shape.elements().find(|e| e.local().starts_with("nv"))?;
    nv.child("nvPr")?.child("ph")
}

/// The (y, x) offset of a shape from its transform, in EMU.
pub(super) fn offset(shape: &Element) -> Option<(i64, i64)> {
    let xfrm = shape
        .path(&["spPr", "xfrm"])
        .or_else(|| shape.path(&["grpSpPr", "xfrm"]))
        .or_else(|| shape.child("xfrm"))?;
    let off = xfrm.child("off")?;
    Some((off.attr("y")?.parse().ok()?, off.attr("x")?.parse().ok()?))
}
