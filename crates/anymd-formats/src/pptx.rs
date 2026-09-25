//! PPTX → Markdown: one section per slide in presentation order, with titles,
//! bullet levels, tables, chart data, image alt text, and speaker notes.

use std::collections::HashMap;

use crate::ooxml::{self, Blocks, Element, Inline, ListIndent, Package, Rels};
use crate::{markdown_table, ConvertError, Converted, Options, Section};

pub fn convert(bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
    let mut package = Package::open(bytes, "PPTX")?;
    let main = package.main_part("ppt/presentation.xml")?;
    let presentation = package
        .xml(&main)?
        .ok_or_else(|| ooxml::invalid("PPTX has no ppt/presentation.xml part"))?;
    let rels = package.rels(&main)?;

    let mut slides: Vec<String> = presentation
        .path(&["sldIdLst"])
        .map(|list| {
            list.children_named("sldId")
                .filter_map(|id| id.rel_attr("id"))
                .filter_map(|id| rels.get(id))
                .map(|r| r.target.clone())
                .collect()
        })
        .unwrap_or_default();
    if slides.is_empty() {
        // No slide list: fall back to ppt/slides/slideN.xml in numeric order.
        let mut numbered: Vec<(u32, String)> = (1..=ooxml::MAX_ENTRIES as u32)
            .map(|n| (n, format!("ppt/slides/slide{n}.xml")))
            .take_while(|(_, name)| package.has(name))
            .collect();
        numbered.sort();
        slides = numbered.into_iter().map(|(_, name)| name).collect();
    }

    let (title, metadata) = ooxml::core_properties(&mut package);
    let mut sections = Vec::with_capacity(slides.len());
    let mut layouts: HashMap<String, Positions> = HashMap::new();
    for (index, path) in slides.iter().enumerate() {
        let markdown = match package.xml(path)? {
            Some(slide) => render_slide(&mut package, path, &slide, &mut layouts)?,
            None => String::new(),
        };
        sections.push(Section {
            label: format!("slide {}", index + 1),
            markdown,
        });
    }
    Ok(Converted {
        format: "pptx".into(),
        title,
        sections,
        metadata,
    })
}

/// Placeholder positions inherited from the slide layout and master.
#[derive(Default, Clone)]
struct Positions {
    by_idx: HashMap<String, (i64, i64)>,
    by_type: HashMap<String, (i64, i64)>,
}

impl Positions {
    fn collect(&mut self, tree: &Element) {
        for shape in tree.elements() {
            if shape.is("grpSp") {
                self.collect(shape);
                continue;
            }
            let (Some(ph), Some(pos)) = (placeholder(shape), offset(shape)) else {
                continue;
            };
            if let Some(idx) = ph.attr("idx") {
                self.by_idx.entry(idx.to_string()).or_insert(pos);
            }
            self.by_type
                .entry(ph.attr("type").unwrap_or("body").to_string())
                .or_insert(pos);
        }
    }

    fn lookup(&self, ph: &Element) -> Option<(i64, i64)> {
        ph.attr("idx")
            .and_then(|idx| self.by_idx.get(idx))
            .or_else(|| self.by_type.get(ph.attr("type").unwrap_or("body")))
            .copied()
    }
}

fn layout_positions(
    package: &mut Package<'_>,
    slide_rels: &Rels,
) -> Result<Positions, ConvertError> {
    let mut positions = Positions::default();
    let Some(layout) = slide_rels
        .first_of_type("/slideLayout")
        .map(|r| r.target.clone())
    else {
        return Ok(positions);
    };
    if let Some(root) = package.xml(&layout).ok().flatten() {
        if let Some(tree) = root.path(&["cSld", "spTree"]) {
            positions.collect(tree);
        }
    }
    let layout_rels = package.rels(&layout)?;
    if let Some(master) = layout_rels
        .first_of_type("/slideMaster")
        .map(|r| r.target.clone())
    {
        if let Some(root) = package.xml(&master).ok().flatten() {
            if let Some(tree) = root.path(&["cSld", "spTree"]) {
                positions.collect(tree);
            }
        }
    }
    Ok(positions)
}

fn render_slide(
    package: &mut Package<'_>,
    path: &str,
    slide: &Element,
    layouts: &mut HashMap<String, Positions>,
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

    // Chart parts are read up front so shape rendering needs no package access.
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

    let context = Context {
        rels: &rels,
        positions: &positions,
        charts: &charts,
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
    let mut markdown = blocks.finish();
    if !markdown.is_empty() {
        markdown.push('\n');
    }
    Ok(markdown)
}

struct Context<'a> {
    rels: &'a Rels,
    positions: &'a Positions,
    charts: &'a HashMap<String, Element>,
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
                        .map(|p| self.paragraph(p).render(false).replace('\n', " "))
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
                        blocks.push(&chart_markdown(root), false);
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
                if let Some(image) = ooxml::image_markdown(alt, target) {
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
            let inline = self.paragraph(p);
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

    fn paragraph(&self, p: &Element) -> Inline {
        let mut inline = Inline::default();
        self.runs(p, &mut inline);
        inline
    }

    fn runs(&self, container: &Element, inline: &mut Inline) {
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
                        .and_then(|id| self.rels.link(id));
                    let text = child.child("t").map(Element::text).unwrap_or_default();
                    inline.push(&text, on("b"), on("i"), link.as_deref());
                }
                "br" => inline.push("\n", false, false, None),
                "AlternateContent" => {
                    if let Some(choice) = child.child("Choice").or_else(|| child.child("Fallback"))
                    {
                        self.runs(choice, inline);
                    }
                }
                _ => {}
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
                                .map(|p| self.paragraph(p).render(true))
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
        markdown_table(&rows)
    }
}

fn placeholder(shape: &Element) -> Option<&Element> {
    let nv = shape.elements().find(|e| e.local().starts_with("nv"))?;
    nv.child("nvPr")?.child("ph")
}

/// The (y, x) offset of a shape from its transform, in EMU.
fn offset(shape: &Element) -> Option<(i64, i64)> {
    let xfrm = shape
        .path(&["spPr", "xfrm"])
        .or_else(|| shape.path(&["grpSpPr", "xfrm"]))
        .or_else(|| shape.child("xfrm"))?;
    let off = xfrm.child("off")?;
    Some((off.attr("y")?.parse().ok()?, off.attr("x")?.parse().ok()?))
}

fn notes_text(notes: &Element, rels: &Rels) -> String {
    let context = Context {
        rels,
        positions: &Positions::default(),
        charts: &HashMap::new(),
    };
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
                let text = context.paragraph(p).render(true);
                if !text.trim().is_empty() {
                    lines.push(text);
                }
            }
        }
    }
    lines.join("\n")
}

/// A chart's cached series data as a heading plus a Markdown table.
fn chart_markdown(space: &Element) -> String {
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
        return heading;
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
    format!("{heading}\n\n{}", markdown_table(&rows).trim_end())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    fn zip(parts: &[(String, String)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            writer.start_file(name.as_str(), options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn rels(entries: &[(&str, &str, &str)]) -> String {
        let body: String = entries
            .iter()
            .map(|(id, kind, target)| {
                let mode = if target.starts_with("http") {
                    r#" TargetMode="External""#
                } else {
                    ""
                };
                format!(r#"<Relationship Id="{id}" Type="{REL}/{kind}" Target="{target}"{mode}/>"#)
            })
            .collect();
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>"#
        )
    }

    fn sp(ph: &str, y: i64, paragraphs: &str) -> String {
        let ph = if ph.is_empty() {
            String::new()
        } else {
            format!("<p:ph {ph}/>")
        };
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="s"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="{y}"/><a:ext cx="1" cy="1"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/>{paragraphs}</p:txBody></p:sp>"#
        )
    }

    fn para(lvl: u32, text: &str) -> String {
        format!(
            r#"<a:p><a:pPr lvl="{lvl}"/><a:r><a:rPr lang="en-US"/><a:t>{text}</a:t></a:r></a:p>"#
        )
    }

    fn slide(shapes: &str) -> String {
        format!(
            r#"<p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr/><p:grpSpPr/>{shapes}</p:spTree></p:cSld></p:sld>"#
        )
    }

    fn deck() -> Vec<u8> {
        let first = slide(&format!(
            "{}{}{}{}",
            // Body before title in the tree; the title must still come first.
            sp(
                r#"idx="1""#,
                2_000_000,
                &format!(
                    "{}{}{}",
                    para(0, "Point one"),
                    para(1, "Detail"),
                    para(0, "Point two")
                )
            ),
            sp(r#"type="title""#, 100, &para(0, "Intro")),
            sp(
                "",
                5_000_000,
                r#"<a:p><a:r><a:rPr b="1"/><a:t>Bold</a:t></a:r><a:r><a:rPr/><a:t xml:space="preserve"> and </a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="rIdL"/></a:rPr><a:t>link</a:t></a:r></a:p>"#
            ),
            sp(r#"type="sldNum" idx="12""#, 6_000_000, &para(0, "1")),
        ));
        let table = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="t"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="3000000"/><a:ext cx="1" cy="1"/></p:xfrm><a:graphic><a:graphicData uri="t"><a:tbl><a:tr h="1"><a:tc><a:txBody><a:p><a:r><a:t>Region</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Sales</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr h="1"><a:tc><a:txBody><a:p><a:r><a:t>EU</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>12</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#;
        let second = slide(&format!(
            "{}{table}",
            sp(r#"type="title""#, 0, &para(0, "Numbers"))
        ));
        let notes = format!(
            r#"<p:notes {NS}><p:cSld><p:spTree>{}{}</p:spTree></p:cSld></p:notes>"#,
            sp(r#"type="sldImg""#, 0, ""),
            sp(
                r#"type="body" idx="1""#,
                0,
                &format!("{}{}", para(0, "Say hello"), para(0, "Then pause"))
            )
        );
        let presentation = format!(
            r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="257" r:id="rId3"/><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>"#
        );
        zip(&[
            ("_rels/.rels".into(), rels(&[("rId1", "officeDocument", "ppt/presentation.xml")])),
            ("ppt/presentation.xml".into(), presentation),
            ("ppt/_rels/presentation.xml.rels".into(), rels(&[("rId2", "slide", "slides/slide2.xml"), ("rId3", "slide", "slides/slide1.xml")])),
            ("ppt/slides/slide1.xml".into(), first),
            ("ppt/slides/_rels/slide1.xml.rels".into(), rels(&[("rIdL", "hyperlink", "https://example.com"), ("rIdN", "notesSlide", "../notesSlides/notesSlide1.xml")])),
            ("ppt/slides/slide2.xml".into(), second),
            ("ppt/notesSlides/notesSlide1.xml".into(), notes),
            ("docProps/core.xml".into(), r#"<cp:coreProperties xmlns:cp="c" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Deck</dc:title></cp:coreProperties>"#.into()),
        ])
    }

    #[test]
    fn slides_in_presentation_order_with_titles_lists_links_and_notes() {
        let out = convert(&deck(), &Options::default()).unwrap();
        assert_eq!(out.title.as_deref(), Some("Deck"));
        let labels: Vec<&str> = out.sections.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["slide 1", "slide 2"]);
        // sldIdLst lists slide1.xml first even though its relationship id sorts later.
        assert_eq!(
            out.sections[0].markdown,
            "## Intro\n\n- Point one\n  - Detail\n- Point two\n\n**Bold** and [link](https://example.com)\n\n> Notes: Say hello\n> Then pause\n"
        );
        assert_eq!(
            out.sections[1].markdown,
            "## Numbers\n\n|Region|Sales|\n|-|-|\n|EU|12|\n"
        );
    }

    #[test]
    fn chart_series_render_as_table() {
        let chart = Element::default();
        assert_eq!(chart_markdown(&chart), "### Chart: untitled");
        let xml = br#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Revenue</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>2025</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>12.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let root = ooxml::parse_xml(xml).unwrap();
        assert_eq!(
            chart_markdown(&root),
            "### Chart: Revenue\n\n|Category|2025|\n|-|-|\n|Q1|10|\n|Q2|12.5|"
        );
    }

    #[test]
    fn malformed_input_is_invalid() {
        assert!(matches!(
            convert(b"PK\x03\x04garbage", &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
        let empty = zip(&[("x".into(), "y".into())]);
        assert!(matches!(
            convert(&empty, &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
    }
}
