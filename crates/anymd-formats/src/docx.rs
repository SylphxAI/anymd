//! DOCX → Markdown: headings, emphasis, links, lists, tables, footnotes.

use std::collections::HashMap;

use crate::ooxml::{self, Blocks, Element, Inline, ListIndent, Package, Rels};
use crate::{markdown_table, ConvertError, Converted, Options, Section};

pub fn convert(bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
    let mut package = Package::open(bytes, "DOCX")?;
    let main = package.main_part("word/document.xml")?;
    let document = package
        .xml(&main)?
        .ok_or_else(|| ooxml::invalid("DOCX has no word/document.xml part"))?;
    let rels = package.rels(&main)?;

    let part = |kind: &str, fallback: &str| {
        rels.first_of_type(kind)
            .map(|r| r.target.clone())
            .unwrap_or_else(|| fallback.to_string())
    };
    // Auxiliary parts are best-effort: a broken styles part should not lose the text.
    let styles = package
        .xml(&part("/styles", "word/styles.xml"))
        .ok()
        .flatten()
        .map(|e| Styles::parse(&e))
        .unwrap_or_default();
    let numbering = package
        .xml(&part("/numbering", "word/numbering.xml"))
        .ok()
        .flatten()
        .map(|e| Numbering::parse(&e))
        .unwrap_or_default();
    let mut notes = HashMap::new();
    for (kind, fallback, name) in [
        ("/footnotes", "word/footnotes.xml", "footnote"),
        ("/endnotes", "word/endnotes.xml", "endnote"),
    ] {
        if let Ok(Some(root)) = package.xml(&part(kind, fallback)) {
            for note in root.children_named(name) {
                if let Some(id) = note.attr("id") {
                    notes.insert((name == "endnote", id.to_string()), note.clone());
                }
            }
        }
    }
    let (title, metadata) = ooxml::core_properties(&mut package);

    let mut writer = Writer {
        rels: &rels,
        styles,
        numbering,
        counters: HashMap::new(),
        started_nums: Vec::new(),
        note_refs: Vec::new(),
    };
    let body = document.child("body").unwrap_or(&document);
    let mut blocks = Blocks::new();
    let mut list = ListIndent::default();
    writer.blocks(body, &mut blocks, &mut list);

    // Footnotes and endnotes, numbered in reference order.
    let mut index = 0;
    let mut defs = Vec::new();
    while index < writer.note_refs.len() {
        let key = writer.note_refs[index].clone();
        index += 1;
        let Some(note) = notes.get(&key) else {
            continue;
        };
        let text: Vec<String> = note
            .children_named("p")
            .map(|p| {
                let (inline, _) = writer.paragraph_inline(p);
                inline.render(true).replace('\n', " ")
            })
            .filter(|t| !t.trim().is_empty())
            .collect();
        if !text.is_empty() {
            defs.push(format!("[^{index}]: {}", text.join(" ")));
        }
    }
    let mut markdown = blocks.finish();
    if !defs.is_empty() {
        markdown.push_str("\n\n");
        markdown.push_str(&defs.join("\n"));
    }
    if !markdown.is_empty() {
        markdown.push('\n');
    }

    Ok(Converted {
        format: "docx".into(),
        title,
        sections: vec![Section {
            label: "document".into(),
            markdown,
        }],
        metadata,
    })
}

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
struct Styles {
    by_id: HashMap<String, Style>,
}

impl Styles {
    fn parse(root: &Element) -> Self {
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

    fn heading_level(&self, id: &str) -> Option<usize> {
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

    fn num(&self, id: &str) -> Option<(String, usize)> {
        self.chain(id).into_iter().find_map(|s| s.num.clone())
    }

    fn emphasis(&self, id: &str) -> (Option<bool>, Option<bool>) {
        let chain = self.chain(id);
        (
            chain.iter().find_map(|s| s.bold),
            chain.iter().find_map(|s| s.italic),
        )
    }
}

fn num_pr(numpr: &Element) -> Option<(String, usize)> {
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
struct Numbering {
    /// numId → (abstractNumId, per-level start overrides)
    nums: HashMap<String, (String, HashMap<usize, u32>)>,
    abstracts: HashMap<String, HashMap<usize, Level>>,
}

impl Numbering {
    fn parse(root: &Element) -> Self {
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

/// An open complex field (`fldChar begin … separate … end`).
struct Field {
    instruction: String,
    in_result: bool,
    link: Option<String>,
}

struct Writer<'a> {
    rels: &'a Rels,
    styles: Styles,
    numbering: Numbering,
    /// abstractNumId → running counter per level (None = not started).
    counters: HashMap<String, [Option<i64>; 9]>,
    started_nums: Vec<String>,
    /// (is_endnote, id) in first-reference order.
    note_refs: Vec<(bool, String)>,
}

impl Writer<'_> {
    fn blocks(&mut self, container: &Element, blocks: &mut Blocks, list: &mut ListIndent) {
        for child in container.elements() {
            match child.local() {
                "p" => self.paragraph(child, blocks, list),
                "tbl" => {
                    list.reset();
                    let table = self.table(child);
                    blocks.push(&table, false);
                }
                "sdt" => {
                    if let Some(content) = child.child("sdtContent") {
                        self.blocks(content, blocks, list);
                    }
                }
                "customXml" | "ins" | "moveTo" | "sdtContent" | "txbxContent" => {
                    self.blocks(child, blocks, list)
                }
                _ => {}
            }
        }
    }

    fn paragraph(&mut self, p: &Element, blocks: &mut Blocks, list: &mut ListIndent) {
        let ppr = p.child("pPr");
        let style = ppr
            .and_then(|pr| pr.child("pStyle"))
            .and_then(|s| s.attr("val"))
            .map(str::to_string);
        let direct_outline = ppr
            .and_then(|pr| pr.child("outlineLvl"))
            .and_then(|o| o.attr("val"))
            .and_then(|v| v.parse::<usize>().ok())
            .map(|l| if l < 9 { Some((l + 1).min(6)) } else { None });
        let heading = match direct_outline {
            Some(level) => level,
            None => style.as_deref().and_then(|s| self.styles.heading_level(s)),
        };
        let num = ppr
            .and_then(|pr| pr.child("numPr"))
            .and_then(num_pr)
            .or_else(|| style.as_deref().and_then(|s| self.styles.num(s)));

        let (inline, extra) = self.paragraph_inline(p);
        if !inline.is_blank() {
            if let Some(level) = heading {
                list.reset();
                let text = inline.render(false).replace('\n', " ");
                blocks.push(&format!("{} {text}", "#".repeat(level)), false);
            } else if let Some(marker) =
                num.and_then(|(id, ilvl)| self.list_marker(&id, ilvl.min(8)).map(|m| (m, ilvl)))
            {
                let (marker, ilvl) = marker;
                let item = list.item(ilvl, &marker, &inline.render(true));
                blocks.push(&item, true);
            } else {
                list.reset();
                blocks.push(&inline.render(true), false);
            }
        }
        for block in extra {
            list.reset();
            blocks.push(&block, false);
        }
    }

    /// The Markdown marker for a numbered paragraph, advancing Word's counters.
    fn list_marker(&mut self, num_id: &str, ilvl: usize) -> Option<String> {
        if num_id == "0" {
            return None;
        }
        let (abs, level) = self.numbering.level(num_id, ilvl)?;
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
            if let Some((_, overrides)) = self.numbering.nums.get(num_id) {
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

    /// Inline content of a paragraph plus any text-box blocks found inside it.
    fn paragraph_inline(&mut self, p: &Element) -> (Inline, Vec<String>) {
        let mut inline = Inline::default();
        let mut extra = Vec::new();
        let mut fields = Vec::new();
        let (bold, italic) = p
            .path(&["pPr", "pStyle"])
            .and_then(|s| s.attr("val"))
            .map(|s| self.styles.emphasis(s))
            .unwrap_or((None, None));
        let base = (bold.unwrap_or(false), italic.unwrap_or(false));
        self.inline(p, None, base, &mut fields, &mut inline, &mut extra);
        (inline, extra)
    }

    fn inline(
        &mut self,
        container: &Element,
        link: Option<&str>,
        base: (bool, bool),
        fields: &mut Vec<Field>,
        out: &mut Inline,
        extra: &mut Vec<String>,
    ) {
        for child in container.elements() {
            match child.local() {
                "r" => self.run(child, link, base, fields, out, extra),
                "hyperlink" => {
                    let url = child.rel_attr("id").and_then(|id| self.rels.link(id));
                    self.inline(child, url.as_deref().or(link), base, fields, out, extra);
                }
                "fldSimple" => {
                    let url = child.attr("instr").and_then(hyperlink_instruction);
                    self.inline(child, url.as_deref().or(link), base, fields, out, extra);
                }
                "ins" | "smartTag" | "customXml" | "sdt" | "sdtContent" | "moveTo" | "bdo"
                | "dir" => self.inline(child, link, base, fields, out, extra),
                "oMathPara" => {
                    let equations: Vec<String> = child
                        .children_named("oMath")
                        .map(math)
                        .filter(|m| !m.is_empty())
                        .collect();
                    if !equations.is_empty() {
                        out.push(
                            &format!("$${}$$", equations.join(" \\\\ ")),
                            false,
                            false,
                            None,
                        );
                    }
                }
                "oMath" => {
                    let latex = math(child);
                    if !latex.is_empty() {
                        out.push(&format!("${latex}$"), false, false, None);
                    }
                }
                _ => {}
            }
        }
    }

    fn run(
        &mut self,
        run: &Element,
        link: Option<&str>,
        base: (bool, bool),
        fields: &mut Vec<Field>,
        out: &mut Inline,
        extra: &mut Vec<String>,
    ) {
        let rpr = run.child("rPr");
        if rpr.and_then(|r| r.toggle("vanish")).unwrap_or(false) {
            return;
        }
        let (style_bold, style_italic) = rpr
            .and_then(|r| r.child("rStyle"))
            .and_then(|s| s.attr("val"))
            .map(|s| self.styles.emphasis(s))
            .unwrap_or((None, None));
        let bold = rpr
            .and_then(|r| r.toggle("b"))
            .or(style_bold)
            .unwrap_or(base.0);
        let italic = rpr
            .and_then(|r| r.toggle("i"))
            .or(style_italic)
            .unwrap_or(base.1);
        for child in run.elements() {
            self.run_child(child, link, (bold, italic), fields, out, extra);
        }
    }

    fn run_child(
        &mut self,
        child: &Element,
        link: Option<&str>,
        (bold, italic): (bool, bool),
        fields: &mut Vec<Field>,
        out: &mut Inline,
        extra: &mut Vec<String>,
    ) {
        let hidden = fields.iter().any(|f| !f.in_result);
        let field_link = fields.iter().rev().find_map(|f| f.link.clone());
        let link = field_link.as_deref().or(link);
        match child.local() {
            "t" if !hidden => out.push(&child.text(), bold, italic, link),
            "tab" | "ptab" if !hidden => out.push("\t", bold, italic, link),
            "br" | "cr" if !hidden => {
                if child.attr("type") != Some("page") {
                    out.push("\n", false, false, None);
                }
            }
            "noBreakHyphen" if !hidden => out.push("-", bold, italic, link),
            "instrText" => {
                if let Some(field) = fields.last_mut() {
                    if !field.in_result {
                        field.instruction.push_str(&child.text());
                    }
                }
            }
            "fldChar" => match child.attr("fldCharType") {
                Some("begin") => fields.push(Field {
                    instruction: String::new(),
                    in_result: false,
                    link: None,
                }),
                Some("separate") => {
                    if let Some(field) = fields.last_mut() {
                        field.in_result = true;
                        field.link = hyperlink_instruction(&field.instruction);
                    }
                }
                Some("end") => {
                    fields.pop();
                }
                _ => {}
            },
            "footnoteReference" | "endnoteReference" if !hidden => {
                if let Some(id) = child.attr("id") {
                    let key = (child.is("endnoteReference"), id.to_string());
                    let number = match self.note_refs.iter().position(|k| *k == key) {
                        Some(i) => i + 1,
                        None => {
                            self.note_refs.push(key);
                            self.note_refs.len()
                        }
                    };
                    out.push(&format!("[^{number}]"), false, false, None);
                }
            }
            "drawing" | "pict" | "object" if !hidden => self.drawing(child, out, extra),
            "AlternateContent" => {
                if let Some(choice) = child.child("Choice").or_else(|| child.child("Fallback")) {
                    for inner in choice.elements() {
                        self.run_child(inner, link, (bold, italic), fields, out, extra);
                    }
                }
            }
            _ => {}
        }
    }

    /// Images with alt text become `![alt](name)`; text boxes become extra blocks.
    fn drawing(&mut self, drawing: &Element, out: &mut Inline, extra: &mut Vec<String>) {
        let mut boxes = Vec::new();
        drawing.find_all("txbxContent", &mut boxes);
        for content in boxes {
            let mut blocks = Blocks::new();
            let mut list = ListIndent::default();
            self.blocks(content, &mut blocks, &mut list);
            extra.push(blocks.finish());
        }
        if let Some(doc_pr) = drawing.find("docPr") {
            let alt = doc_pr
                .attr("descr")
                .filter(|d| !d.trim().is_empty())
                .or_else(|| doc_pr.attr("title"))
                .unwrap_or("");
            let target = drawing
                .find("blip")
                .and_then(|b| b.rel_attr("embed").or_else(|| b.rel_attr("link")))
                .and_then(|id| self.rels.get(id))
                .map(|r| r.target.clone());
            if let Some(image) = ooxml::image_markdown(alt, target.as_deref()) {
                out.push(&image, false, false, None);
            }
        }
    }

    fn table(&mut self, table: &Element) -> String {
        let mut rows = Vec::new();
        for tr in table_rows(table) {
            let mut row = Vec::new();
            let before = tr
                .path(&["trPr", "gridBefore"])
                .and_then(|g| g.attr("val"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            row.extend(std::iter::repeat_n(String::new(), before.min(64)));
            for tc in row_cells(tr) {
                row.push(self.cell_text(tc));
                let span = tc
                    .path(&["tcPr", "gridSpan"])
                    .and_then(|g| g.attr("val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1);
                row.extend(std::iter::repeat_n(String::new(), span.clamp(1, 64) - 1));
            }
            rows.push(row);
        }
        // Drop trailing empty columns and fully empty rows.
        rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
        if rows.is_empty() {
            return String::new();
        }
        let width = (0..rows.iter().map(Vec::len).max().unwrap_or(0))
            .rev()
            .find(|&c| {
                rows.iter()
                    .any(|r| r.get(c).is_some_and(|v| !v.trim().is_empty()))
            })
            .map_or(0, |c| c + 1);
        for row in &mut rows {
            row.truncate(width);
        }
        markdown_table(&rows)
    }

    fn cell_text(&mut self, cell: &Element) -> String {
        let mut parts = Vec::new();
        self.cell_parts(cell, &mut parts);
        parts.retain(|p| !p.trim().is_empty());
        parts.join("<br>").replace('\n', "<br>")
    }

    fn cell_parts(&mut self, container: &Element, parts: &mut Vec<String>) {
        for child in container.elements() {
            match child.local() {
                "p" => {
                    let (inline, extra) = self.paragraph_inline(child);
                    parts.push(inline.render(true));
                    parts.extend(extra);
                }
                "tbl" => {
                    // Nested tables are flattened to text, one row per line.
                    for tr in table_rows(child) {
                        let cells: Vec<String> = row_cells(tr)
                            .map(|tc| self.cell_text(tc))
                            .filter(|t| !t.is_empty())
                            .collect();
                        parts.push(cells.join("; "));
                    }
                }
                "sdt" | "sdtContent" | "customXml" | "ins" => self.cell_parts(child, parts),
                _ => {}
            }
        }
    }
}

fn table_rows(table: &Element) -> Vec<&Element> {
    let mut rows = Vec::new();
    collect_named(
        table,
        "tr",
        &["sdt", "sdtContent", "customXml", "ins"],
        &mut rows,
    );
    rows
}

fn row_cells(row: &Element) -> impl Iterator<Item = &Element> {
    let mut cells = Vec::new();
    collect_named(
        row,
        "tc",
        &["sdt", "sdtContent", "customXml", "ins"],
        &mut cells,
    );
    cells.into_iter()
}

fn collect_named<'a>(
    container: &'a Element,
    name: &str,
    wrappers: &[&str],
    out: &mut Vec<&'a Element>,
) {
    for child in container.elements() {
        if child.is(name) {
            out.push(child);
        } else if wrappers.contains(&child.local()) {
            collect_named(child, name, wrappers, out);
        }
    }
}

/// Office Math (OMML) to LaTeX, covering the structures Word's equation editor emits.
fn math(element: &Element) -> String {
    ooxml::collapse_ws(&math_inner(element, 0))
}

fn math_inner(element: &Element, depth: usize) -> String {
    if depth > 64 {
        return String::new();
    }
    let part = |name: &str| {
        element
            .child(name)
            .map(|e| math_inner(e, depth + 1))
            .unwrap_or_default()
    };
    let prop = |pr: &str, name: &str| {
        element
            .path(&[pr, name])
            .and_then(|e| e.attr("val"))
            .map(str::to_string)
    };
    match element.local() {
        "r" => {
            let text: String = element
                .elements()
                .filter(|e| e.is("t"))
                .map(Element::text)
                .collect();
            text.replace('\u{2061}', "")
        }
        "f" => format!("\\frac{{{}}}{{{}}}", part("num"), part("den")),
        "sSup" => format!("{}^{}", group(&part("e")), group(&part("sup"))),
        "sSub" => format!("{}_{}", group(&part("e")), group(&part("sub"))),
        "sSubSup" => format!(
            "{}_{}^{}",
            group(&part("e")),
            group(&part("sub")),
            group(&part("sup"))
        ),
        "sPre" => format!(
            "{{}}_{}^{}{}",
            group(&part("sub")),
            group(&part("sup")),
            part("e")
        ),
        "rad" => {
            let degree = part("deg");
            if degree.trim().is_empty() {
                format!("\\sqrt{{{}}}", part("e"))
            } else {
                format!("\\sqrt[{degree}]{{{}}}", part("e"))
            }
        }
        "d" => {
            let open = prop("dPr", "begChr").unwrap_or_else(|| "(".into());
            let close = prop("dPr", "endChr").unwrap_or_else(|| ")".into());
            let separator = prop("dPr", "sepChr").unwrap_or_else(|| "|".into());
            let items: Vec<String> = element
                .children_named("e")
                .map(|e| math_inner(e, depth + 1))
                .collect();
            let brace = |c: &str| match c {
                "{" => "\\{".to_string(),
                "}" => "\\}".to_string(),
                other => other.to_string(),
            };
            format!(
                "{}{}{}",
                brace(&open),
                items.join(&separator),
                brace(&close)
            )
        }
        "func" => {
            let name = part("fName");
            let known = [
                "sin", "cos", "tan", "cot", "sec", "csc", "log", "ln", "exp", "lim", "max", "min",
                "sinh", "cosh", "tanh", "arcsin", "arccos", "arctan", "det",
            ];
            let name = match known.iter().find(|k| name.trim() == **k) {
                Some(k) => format!("\\{k}"),
                None => name,
            };
            format!("{name}{}", group_always(&part("e")))
        }
        "nary" => {
            let symbol = match prop("naryPr", "chr").as_deref() {
                None | Some("∫") => "\\int",
                Some("∑") => "\\sum",
                Some("∏") => "\\prod",
                Some("∬") => "\\iint",
                Some("∮") => "\\oint",
                Some("⋃") => "\\bigcup",
                Some("⋂") => "\\bigcap",
                Some(_) => "",
            }
            .to_string();
            let symbol = if symbol.is_empty() {
                prop("naryPr", "chr").unwrap_or_default()
            } else {
                symbol
            };
            let (sub, sup) = (part("sub"), part("sup"));
            let mut out = symbol;
            if !sub.trim().is_empty() {
                out.push_str(&format!("_{}", group(&sub)));
            }
            if !sup.trim().is_empty() {
                out.push_str(&format!("^{}", group(&sup)));
            }
            format!("{out} {}", part("e"))
        }
        "acc" => {
            let command = match prop("accPr", "chr").as_deref() {
                Some("\u{303}" | "~") => "tilde",
                Some("\u{304}" | "\u{305}" | "¯") => "bar",
                Some("\u{307}" | "˙") => "dot",
                Some("\u{308}") => "ddot",
                Some("\u{20d7}" | "→") => "vec",
                _ => "hat",
            };
            format!("\\{command}{{{}}}", part("e"))
        }
        "bar" => {
            let command = if prop("barPr", "pos").as_deref() == Some("top") {
                "overline"
            } else {
                "underline"
            };
            format!("\\{command}{{{}}}", part("e"))
        }
        "limLow" => format!("{}_{}", group(&part("e")), group(&part("lim"))),
        "limUpp" => format!("{}^{}", group(&part("e")), group(&part("lim"))),
        "m" => {
            let rows: Vec<String> = element
                .children_named("mr")
                .map(|row| {
                    row.children_named("e")
                        .map(|e| math_inner(e, depth + 1))
                        .collect::<Vec<_>>()
                        .join(" & ")
                })
                .collect();
            format!("\\begin{{matrix}}{}\\end{{matrix}}", rows.join(" \\\\ "))
        }
        "eqArr" => {
            let rows: Vec<String> = element
                .children_named("e")
                .map(|e| math_inner(e, depth + 1))
                .collect();
            format!("\\begin{{aligned}}{}\\end{{aligned}}", rows.join(" \\\\ "))
        }
        name if name.ends_with("Pr") => String::new(),
        _ => element
            .elements()
            .map(|e| math_inner(e, depth + 1))
            .collect(),
    }
}

fn group(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() == 1 {
        s.to_string()
    } else {
        format!("{{{s}}}")
    }
}

fn group_always(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('(') || s.starts_with('[') {
        s.to_string()
    } else {
        format!("{{{s}}}")
    }
}

/// The URL of a `HYPERLINK "url"` field instruction; internal `\l` anchors are ignored.
fn hyperlink_instruction(instruction: &str) -> Option<String> {
    let rest = instruction.trim().strip_prefix("HYPERLINK")?.trim();
    if rest.starts_with("\\l") {
        return None;
    }
    let url = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next()?
    } else {
        rest.split_whitespace().next()?
    };
    (!url.is_empty()).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

    pub(crate) fn zip(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn docx(body: &str, extra: &[(&str, &str)]) -> Vec<u8> {
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}</w:body></w:document>"#
        );
        let mut parts: Vec<(&str, &str)> = vec![
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            ("word/document.xml", &document),
        ];
        parts.extend_from_slice(extra);
        zip(&parts)
    }

    fn md(bytes: &[u8]) -> String {
        convert(bytes, &Options::default()).unwrap().sections[0]
            .markdown
            .clone()
    }

    fn p(style: &str, runs: &str) -> String {
        let ppr = if style.is_empty() {
            String::new()
        } else {
            format!(r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#)
        };
        format!("<w:p>{ppr}{runs}</w:p>")
    }

    fn r(text: &str) -> String {
        format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#)
    }

    #[test]
    fn headings_emphasis_links_and_core_title() {
        let body = [
            p("Title", &r("Report")),
            p("Heading2", r#"<w:r><w:rPr><w:b/></w:rPr><w:t>Scope</w:t></w:r>"#),
            p(
                "",
                &format!(
                    r#"{}<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">bold </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>run</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t xml:space="preserve"> it</w:t></w:r><w:r><w:t>, see </w:t></w:r><w:hyperlink r:id="rId9"><w:r><w:t>docs</w:t></w:r></w:hyperlink><w:r><w:t>.</w:t></w:r>"#,
                    r("Plain ")
                ),
            ),
            r#"<w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>Outline</w:t></w:r></w:p>"#.to_string(),
            r#"<w:p><w:r><w:t>a</w:t></w:r><w:r><w:br/><w:t>b</w:t><w:tab/><w:t>c</w:t></w:r></w:p>"#.to_string(),
            "<w:p/>".to_string(),
        ]
        .concat();
        let bytes = docx(
            &body,
            &[
                (
                    "word/_rels/document.xml.rels",
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a b" TargetMode="External"/></Relationships>"#,
                ),
                (
                    "docProps/core.xml",
                    r#"<cp:coreProperties xmlns:cp="x" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Quarterly</dc:title><dc:creator>Ada</dc:creator></cp:coreProperties>"#,
                ),
            ],
        );
        let out = convert(&bytes, &Options::default()).unwrap();
        assert_eq!(out.title.as_deref(), Some("Quarterly"));
        assert_eq!(
            out.metadata,
            vec![("author".to_string(), "Ada".to_string())]
        );
        assert_eq!(
            out.sections[0].markdown,
            "# Report\n\n## Scope\n\nPlain **bold run** _it_, see [docs](https://example.com/a%20b).\n\n### Outline\n\na\nb\tc\n"
        );
    }

    #[test]
    fn numbered_and_bulleted_lists() {
        let numbering = format!(
            r#"<w:numbering {W}>
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/></w:lvl></w:abstractNum>
<w:num w:numId="5"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="6"><w:abstractNumId w:val="2"/></w:num>
</w:numbering>"#
        );
        let item = |num: u32, lvl: u32, text: &str| {
            format!(
                r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="{num}"/></w:numPr></w:pPr>{}</w:p>"#,
                r(text)
            )
        };
        let body = [
            item(5, 0, "apple"),
            item(5, 1, "green"),
            item(5, 0, "pear"),
            p("", &r("between")),
            item(6, 0, "one"),
            item(6, 1, "sub"),
            item(6, 1, "sub2"),
            item(6, 0, "two"),
        ]
        .concat();
        let bytes = docx(&body, &[("word/numbering.xml", &numbering)]);
        assert_eq!(
            md(&bytes),
            "- apple\n  - green\n- pear\n\nbetween\n\n1. one\n   1. sub\n   2. sub2\n2. two\n"
        );
    }

    #[test]
    fn tables_with_spans_and_nested_tables() {
        let tc = |content: &str| format!("<w:tc>{content}</w:tc>");
        let nested = format!(
            "<w:tbl><w:tr>{}{}</w:tr></w:tbl>",
            tc(&p("", &r("n1"))),
            tc(&p("", &r("n2")))
        );
        let body = format!(
            r#"<w:tbl><w:tr>{}{}{}</w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr>{}</w:tc>{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>"#,
            tc(&p("", &r("Name"))),
            tc(&p("", &r("Q1"))),
            tc(&p("", &r("Q2"))),
            p("", &r("wide|cell")),
            tc(&p("", &r("x"))),
            tc(&format!("{}{}", p("", &r("line1")), p("", &r("line2")))),
            tc(&nested),
            tc(""),
        );
        assert_eq!(
            md(&docx(&body, &[])),
            "|Name|Q1|Q2|\n|-|-|-|\n|wide\\|cell||x|\n|line1<br>line2|n1; n2||\n"
        );
    }

    #[test]
    fn footnotes_images_and_fields() {
        let body = [
            p(
                "",
                r#"<w:r><w:t>Claim</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r>"#,
            ),
            p(
                "",
                r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> HYPERLINK "https://f.example" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>field link</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
            ),
            p(
                "",
                r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="1" name="Picture 1" descr="A chart of sales"/><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="p"><pic:blipFill><a:blip r:embed="rIdImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
            ),
            p("", r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="2" name="Picture 2"/></wp:inline></w:drawing></w:r>"#),
        ]
        .concat();
        let footnotes = format!(
            r#"<w:footnotes {W}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> Source: survey.</w:t></w:r></w:p></w:footnote></w:footnotes>"#
        );
        let bytes = docx(
            &body,
            &[
                ("word/footnotes.xml", &footnotes),
                (
                    "word/_rels/document.xml.rels",
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#,
                ),
            ],
        );
        assert_eq!(
            md(&bytes),
            "Claim[^1]\n\n[field link](https://f.example)\n\n![A chart of sales](image1.png)\n\n[^1]: Source: survey.\n"
        );
    }

    #[test]
    fn malformed_input_is_invalid_not_panic() {
        assert!(matches!(
            convert(b"not a zip", &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
        let no_doc = zip(&[("hello.txt", "hi")]);
        assert!(matches!(
            convert(&no_doc, &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
        let broken = zip(&[("word/document.xml", "<w:document><w:body><w:p></w:body>")]);
        assert!(convert(&broken, &Options::default()).is_err());
        let truncated = docx(&p("", &r("ok")), &[]);
        assert!(convert(&truncated[..truncated.len() / 2], &Options::default()).is_err());
    }
}
