//! DOCX → Markdown: headings, emphasis, links, lists, tables, footnotes, and
//! tracked changes and comments as CriticMarkup.

use std::collections::{HashMap, HashSet};

use crate::critic::{self, Change, Critic, Mark};
use crate::ooxml::{self, Blocks, Element, ListIndent, Media, Package, Rels};
use crate::revise;
use crate::{markdown_table, ConvertError, Converted, Options, Revisions, Section};

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    let mut package = Package::open(bytes, "DOCX")?;
    let main = package.main_part("word/document.xml")?;
    let document = package
        .xml(&main)?
        .ok_or_else(|| ooxml::invalid("DOCX has no word/document.xml part"))?;
    let accept = match options.revisions {
        Revisions::Markup => None,
        Revisions::Accept => Some(true),
        Revisions::Reject => Some(false),
    };
    let document = match accept {
        Some(accept) => revise::resolve(&document, accept),
        None => document,
    };
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
                    let note = match accept {
                        Some(accept) => revise::resolve(note, accept),
                        None => note.clone(),
                    };
                    notes.insert((name == "endnote", id.to_string()), note);
                }
            }
        }
    }
    // Accepting or rejecting the changes leaves comments out as well.
    let comments = package
        .xml(&part("/comments", "word/comments.xml"))
        .ok()
        .flatten()
        .filter(|_| accept.is_none());
    let (title, metadata) = ooxml::core_properties(&mut package);

    let media = Media::load(
        &mut package,
        &rels,
        options.images.as_ref(),
        &std::collections::HashSet::new(),
    );
    let mut writer = Writer {
        rels: &rels,
        media,
        styles,
        numbering,
        counters: HashMap::new(),
        started_nums: Vec::new(),
        note_refs: Vec::new(),
        note_changes: Vec::new(),
        lead: None,
        flattening: 0,
        revisions: Vec::new(),
        comments: HashMap::new(),
        notes: HashMap::new(),
        referenced: HashSet::new(),
        open_comments: Vec::new(),
        pending_notes: Vec::new(),
        in_comment: false,
        plain: false,
    };
    if let Some(root) = &comments {
        writer.comments = root
            .children_named("comment")
            .filter_map(|c| c.attr("id").map(|id| (id.to_string(), c.clone())))
            .collect();
    }
    let mut references = Vec::new();
    document.find_all("commentReference", &mut references);
    for note in notes.values() {
        note.find_all("commentReference", &mut references);
    }
    writer.referenced = references
        .iter()
        .filter_map(|r| r.attr("id"))
        .map(str::to_string)
        .collect();
    writer.plain = !has_markup(&document, &writer.comments)
        && !notes
            .values()
            .any(|note| has_markup(note, &writer.comments));
    let body = document.child("body").unwrap_or(&document);
    let mut blocks = Blocks::new();
    let mut list = ListIndent::default();
    writer.blocks(body, &mut blocks, &mut list);
    // Notes of ranges that ended after the last paragraph.
    let trailing = writer.take_notes();
    blocks.push(&trailing, false);

    // Footnotes and endnotes, numbered in reference order. A comment range the
    // body never closed does not run on into them.
    writer.open_comments.clear();
    let mut index = 0;
    let mut defs = Vec::new();
    while index < writer.note_refs.len() {
        let key = writer.note_refs[index].clone();
        // A note whose reference was inserted or deleted was inserted or
        // deleted with it, label and all.
        let changes = writer.note_changes[index].clone();
        index += 1;
        let Some(note) = notes.get(&key) else {
            continue;
        };
        let around = std::mem::replace(&mut writer.revisions, changes);
        writer.lead = Some(format!("[^{index}]: "));
        let parts = note
            .children_named("p")
            .map(|p| {
                let inline = writer.paragraph_inline(p).0.into_inline();
                (
                    inline.render(true).replace('\n', " "),
                    paragraph_mark(p),
                    inline.edges(),
                )
            })
            .collect();
        writer.revisions = around;
        // Still set: no paragraph had text, so there is no note.
        if writer.lead.take().is_none() {
            defs.push(critic::join_marked(parts, " "));
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

/// Whether a part holds a tracked change, or a comment anchor for a comment
/// that exists. Formatting-only changes (`w:rPrChange`...) do not count: they
/// are not shown.
fn has_markup(part: &Element, comments: &HashMap<String, Element>) -> bool {
    part.elements().any(|child| {
        let anchored = matches!(
            child.local(),
            "commentRangeStart" | "commentRangeEnd" | "commentReference"
        ) && child.attr("id").is_some_and(|id| comments.contains_key(id));
        anchored
            || matches!(
                child.local(),
                "ins" | "del" | "moveTo" | "moveFrom" | "cellIns" | "cellDel"
            )
            || has_markup(child, comments)
    })
}

/// A revision element (`w:ins`, `w:del`, `w:moveTo`, `w:moveFrom`) as a tracked
/// change. Insertions and move destinations are insertions; deletions and move
/// sources are deletions (the representation pandiff uses too).
fn change_of(element: &Element) -> Change {
    let mark = if matches!(element.local(), "ins" | "moveTo") {
        Mark::Insertion
    } else {
        Mark::Deletion
    };
    (mark, attribution(element))
}

/// Who made a tracked change and when: `w:author` and `w:date` exactly as
/// stored, in the same form as a comment's (`Ana Lima (2026-09-29T14:05:00Z)`).
fn attribution(element: &Element) -> Option<String> {
    let by = comment_note(element.attr("author"), element.attr("date"), "");
    (!by.is_empty()).then_some(by)
}

/// Tracked changes recorded in a properties element (`trPr`, `tcPr`, a
/// paragraph mark's `rPr`), in document order.
fn revision_marks(properties: &Element) -> Vec<Change> {
    properties
        .elements()
        .filter_map(|p| match p.local() {
            "ins" | "moveTo" | "cellIns" => Some((Mark::Insertion, attribution(p))),
            "del" | "moveFrom" | "cellDel" => Some((Mark::Deletion, attribution(p))),
            _ => None,
        })
        .collect()
}

/// The tracked change on the mark that ends a paragraph, that is on the break
/// between it and the next one. A mark inserted and then deleted counts as
/// deleted, the later of the two changes.
fn paragraph_mark(p: &Element) -> Option<Change> {
    let marks = p
        .path(&["pPr", "rPr"])
        .map(revision_marks)
        .unwrap_or_default();
    let deleted = marks.iter().position(|(m, _)| *m == Mark::Deletion);
    marks.into_iter().nth(deleted.unwrap_or(0))
}

/// A comment's `{>>…<<}` text: `Author (date): comment`. The date is Word's
/// `w:date` exactly as stored (for example `2024-04-08T10:32:00Z`, which Word
/// writes as the author's local time with a `Z`), so it is never converted.
fn comment_note(author: Option<&str>, date: Option<&str>, text: &str) -> String {
    let mut head = Vec::new();
    if let Some(author) = author.map(str::trim).filter(|a| !a.is_empty()) {
        head.push(critic::escape(author));
    }
    if let Some(date) = date.map(str::trim).filter(|d| !d.is_empty()) {
        head.push(format!("({})", critic::escape(date)));
    }
    let head = head.join(" ");
    match (head.is_empty(), text.is_empty()) {
        (true, _) => text.to_string(),
        (false, true) => head,
        (false, false) => format!("{head}: {text}"),
    }
}

struct Writer<'a> {
    rels: &'a Rels,
    media: Media,
    styles: Styles,
    numbering: Numbering,
    /// abstractNumId → running counter per level (None = not started).
    counters: HashMap<String, [Option<i64>; 9]>,
    started_nums: Vec<String>,
    /// (is_endnote, id) in first-reference order.
    note_refs: Vec<(bool, String)>,
    /// The tracked changes around each note's first reference.
    note_changes: Vec<Vec<Change>>,
    /// Markdown for the start of the next paragraph with text, inside its
    /// tracked changes: a note's `[^N]: ` label.
    lead: Option<String>,
    /// Inside a nested table, whose cells are flattened onto one line.
    flattening: usize,
    /// Tracked changes around the content being written, outermost first.
    revisions: Vec<Change>,
    /// Comment id → its `w:comment` element.
    comments: HashMap<String, Element>,
    /// Comment id → the inside of its `{>>…<<}` note, rendered at first use so
    /// that anything numbered inside it (a footnote) is numbered where the
    /// comment is.
    notes: HashMap<String, String>,
    /// Comments with a `commentReference`, where their note goes. Others get
    /// their note where their range ends.
    referenced: HashSet<String>,
    /// Comment ranges open at this point, in the order they started.
    open_comments: Vec<String>,
    /// Notes of unreferenced comments whose range ended between paragraphs;
    /// they open the next paragraph.
    pending_notes: Vec<String>,
    /// Writing a comment's own text, where a `{>>…<<}` cannot appear: tracked
    /// changes keep their marks but not their author, and comment anchors are
    /// ignored.
    in_comment: bool,
    /// No tracked change or comment anywhere: text is written as is, the way
    /// it was before tracked changes were rendered.
    plain: bool,
}

impl Writer<'_> {
    /// The notes waiting for the next paragraph, as CriticMarkup comments.
    fn take_notes(&mut self) -> String {
        std::mem::take(&mut self.pending_notes)
            .iter()
            .map(|note| format!("{{>>{note}<<}}"))
            .collect()
    }

    fn blocks(&mut self, container: &Element, blocks: &mut Blocks, list: &mut ListIndent) {
        for child in container.elements() {
            match child.local() {
                "p" => self.paragraph(child, blocks, list),
                "tbl" => {
                    list.reset();
                    // A note waiting for the next paragraph would open the
                    // first cell; it gets its own block before the table.
                    let notes = self.take_notes();
                    blocks.push_prefixed("", &notes, false);
                    let table = self.table(child);
                    blocks.push(&table, false);
                }
                "sdt" => {
                    if let Some(content) = child.child("sdtContent") {
                        self.blocks(content, blocks, list);
                    }
                }
                "ins" | "moveTo" | "del" | "moveFrom" => {
                    self.revised(change_of(child), |w| w.blocks(child, blocks, list));
                }
                "commentRangeStart" | "commentRangeEnd" => self.comment_range(child, None),
                "customXml" | "sdtContent" | "txbxContent" => self.blocks(child, blocks, list),
                _ => {}
            }
        }
    }

    /// Writes content inside a tracked change.
    fn revised(&mut self, change: Change, write: impl FnOnce(&mut Self)) {
        self.revisions.push(change);
        write(self);
        self.revisions.pop();
    }

    /// A comment range start or end. The commented text is highlighted; a range
    /// that spans paragraphs gets one highlight per paragraph, because
    /// CriticMarkup cannot cross a block.
    fn comment_range(&mut self, range: &Element, out: Option<&mut Critic>) {
        let Some(id) = range
            .attr("id")
            .filter(|id| !self.in_comment && self.comments.contains_key(*id))
        else {
            return;
        };
        let open = self.open_comments.iter().position(|open| open == id);
        match (range.is("commentRangeStart"), open) {
            (true, None) => {
                self.open_comments.push(id.to_string());
                if let Some(out) = out {
                    out.open(Mark::Highlight, None);
                }
            }
            (false, Some(at)) => {
                self.open_comments.remove(at);
                let note = (!self.referenced.contains(id))
                    .then(|| self.note(id))
                    .flatten();
                match out {
                    Some(out) => {
                        out.close(Mark::Highlight);
                        if let Some(note) = note {
                            out.comment(&note);
                        }
                    }
                    None => self.pending_notes.extend(note),
                }
            }
            _ => {}
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
        let written = !inline.is_blank();
        if written {
            let inline = inline.into_inline();
            let edges = inline.edges();
            if let Some(level) = heading {
                list.reset();
                let text = inline.render(false).replace('\n', " ");
                blocks.push_paragraph(&format!("{} ", "#".repeat(level)), &text, false, edges);
            } else if let Some(marker) =
                num.and_then(|(id, ilvl)| self.list_marker(&id, ilvl.min(8)).map(|m| (m, ilvl)))
            {
                let (marker, ilvl) = marker;
                let item = list.item(ilvl, &marker, &inline.render(true));
                let prefix = item.len() - item.trim_start().len() + marker.len() + 1;
                blocks.push_paragraph(&item[..prefix], &item[prefix..], true, edges);
            } else {
                list.reset();
                blocks.push_paragraph("", &inline.render(true), false, edges);
            }
        }
        // A text box between this paragraph and the next one takes the break.
        let boxed = !extra.is_empty();
        for block in extra {
            list.reset();
            blocks.push(&block, false);
        }
        blocks.set_separator(self.paragraph_mark(p).filter(|_| written && !boxed));
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
    /// Tracked changes around the paragraph and comment ranges still open from
    /// earlier paragraphs are opened again at its start.
    fn paragraph_inline(&mut self, p: &Element) -> (Critic, Vec<String>) {
        let mut inline = if self.plain {
            Critic::plain()
        } else {
            Critic::default()
        };
        if self.flattening > 0 {
            inline.set_inline_end();
        }
        let mut extra = Vec::new();
        let mut fields = Vec::new();
        let (bold, italic) = p
            .path(&["pPr", "pStyle"])
            .and_then(|s| s.attr("val"))
            .map(|s| self.styles.emphasis(s))
            .unwrap_or((None, None));
        let base = (bold.unwrap_or(false), italic.unwrap_or(false));
        if !self.in_comment {
            for note in std::mem::take(&mut self.pending_notes) {
                inline.comment(&note);
            }
        }
        for (mark, by) in &self.revisions {
            inline.open(*mark, by.as_deref());
        }
        // A note's label goes inside its tracked changes, before any comment.
        let start = inline.len();
        for _ in &self.open_comments {
            inline.open(Mark::Highlight, None);
        }
        self.inline(p, None, base, &mut fields, &mut inline, &mut extra);
        if !self.in_comment && !inline.is_blank() {
            if let Some(lead) = self.lead.take() {
                inline.insert_raw(start, &lead);
            }
        }
        if self.in_comment {
            inline = inline.unattributed();
        }
        (inline, extra)
    }

    fn inline(
        &mut self,
        container: &Element,
        link: Option<&str>,
        base: (bool, bool),
        fields: &mut Vec<Field>,
        out: &mut Critic,
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
                "ins" | "moveTo" | "del" | "moveFrom" => {
                    let (mark, by) = change_of(child);
                    out.open(mark, by.as_deref());
                    self.revised((mark, by), |w| {
                        w.inline(child, link, base, fields, out, extra);
                    });
                    out.close(mark);
                }
                "commentRangeStart" | "commentRangeEnd" => self.comment_range(child, Some(out)),
                "smartTag" | "customXml" | "sdt" | "sdtContent" | "bdo" | "dir" => {
                    self.inline(child, link, base, fields, out, extra)
                }
                "oMathPara" => {
                    let equations: Vec<String> = child
                        .children_named("oMath")
                        .map(math)
                        .filter(|m| !m.is_empty())
                        .collect();
                    if !equations.is_empty() {
                        out.raw(&format!("$${}$$", equations.join(" \\\\ ")));
                    }
                }
                "oMath" => {
                    let latex = math(child);
                    if !latex.is_empty() {
                        out.raw(&format!("${latex}$"));
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
        out: &mut Critic,
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
        out: &mut Critic,
        extra: &mut Vec<String>,
    ) {
        let hidden = fields.iter().any(|f| !f.in_result);
        let field_link = fields.iter().rev().find_map(|f| f.link.clone());
        let link = field_link.as_deref().or(link);
        match child.local() {
            "t" | "delText" if !hidden => out.push(&child.text(), bold, italic, link),
            "tab" | "ptab" if !hidden => out.push("\t", bold, italic, link),
            "br" | "cr" if !hidden => {
                if child.attr("type") != Some("page") {
                    out.push("\n", false, false, None);
                }
            }
            "noBreakHyphen" if !hidden => out.push("-", bold, italic, link),
            "instrText" | "delInstrText" => {
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
                            self.note_changes.push(self.revisions.clone());
                            self.note_refs.len()
                        }
                    };
                    out.raw(&format!("[^{number}]"));
                }
            }
            "commentReference" if !hidden && !self.in_comment => {
                if let Some(note) = child.attr("id").and_then(|id| self.note(id)) {
                    out.comment(&note);
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

    /// Images with alt text become `![alt](name)`; text boxes become extra blocks,
    /// inside any tracked change around the drawing. A text box is a story of
    /// its own, so comment ranges around its anchor do not cover it.
    fn drawing(&mut self, drawing: &Element, out: &mut Critic, extra: &mut Vec<String>) {
        let mut boxes = Vec::new();
        drawing.find_all("txbxContent", &mut boxes);
        let open_comments = std::mem::take(&mut self.open_comments);
        for content in boxes {
            let mut blocks = Blocks::new();
            let mut list = ListIndent::default();
            self.blocks(content, &mut blocks, &mut list);
            extra.push(blocks.finish());
        }
        self.open_comments = open_comments;
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
            if let Some(image) = self.media.markdown(alt, target.as_deref(), None) {
                out.raw(&image);
            }
        }
    }

    fn table(&mut self, table: &Element) -> String {
        let mut rows = Vec::new();
        for tr in table_rows(table) {
            let mut row = Vec::new();
            let row_marks = tr.child("trPr").map(revision_marks).unwrap_or_default();
            let before = tr
                .path(&["trPr", "gridBefore"])
                .and_then(|g| g.attr("val"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            row.extend(std::iter::repeat_n(String::new(), before.min(64)));
            for tc in row_cells(tr) {
                row.push(self.cell_text(&row_marks, tc));
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

    /// A cell's paragraphs joined with `<br>`, inside the tracked changes of its
    /// row (`trPr`) and of the cell itself (`tcPr`).
    fn cell_text(&mut self, row_marks: &[Change], cell: &Element) -> String {
        let depth = self.revisions.len();
        self.revisions.extend_from_slice(row_marks);
        if let Some(properties) = cell.child("tcPr") {
            self.revisions.extend(revision_marks(properties));
        }
        let mut parts = Vec::new();
        self.cell_parts(cell, &mut parts);
        self.revisions.truncate(depth);
        critic::join_marked(parts, "<br>").replace('\n', "<br>")
    }

    /// Paragraph texts in a cell, each with the tracked change on its mark.
    fn cell_parts(&mut self, container: &Element, parts: &mut Vec<critic::Part>) {
        for child in container.elements() {
            match child.local() {
                "p" => {
                    let (inline, extra) = self.paragraph_inline(child);
                    let mark = self.paragraph_mark(child).filter(|_| extra.is_empty());
                    let inline = inline.into_inline();
                    parts.push((inline.render(true), mark, inline.edges()));
                    parts.extend(extra.into_iter().map(|block| (block, None, (false, false))));
                }
                "tbl" => {
                    // Nested tables are flattened to text, one row per line.
                    self.flattening += 1;
                    for tr in table_rows(child) {
                        let row_marks = tr.child("trPr").map(revision_marks).unwrap_or_default();
                        let cells: Vec<String> = row_cells(tr)
                            .map(|tc| self.cell_text(&row_marks, tc))
                            .filter(|t| !t.is_empty())
                            .collect();
                        parts.push((critic::join_changed(cells, "; "), None, (false, false)));
                    }
                    self.flattening -= 1;
                }
                "ins" | "moveTo" | "del" | "moveFrom" => {
                    self.revised(change_of(child), |w| w.cell_parts(child, parts));
                }
                "commentRangeStart" | "commentRangeEnd" => self.comment_range(child, None),
                "sdt" | "sdtContent" | "customXml" => self.cell_parts(child, parts),
                _ => {}
            }
        }
    }

    /// The tracked change on a paragraph's mark, without its author inside a
    /// comment's own text.
    fn paragraph_mark(&self, p: &Element) -> Option<Change> {
        paragraph_mark(p).map(|(mark, by)| (mark, by.filter(|_| !self.in_comment)))
    }

    /// A comment's note, rendered the first time it is needed. Its text is
    /// written apart from the document around its anchor: no tracked change
    /// or comment range there applies to it.
    fn note(&mut self, id: &str) -> Option<String> {
        if let Some(note) = self.notes.get(id) {
            return Some(note.clone());
        }
        let comment = self.comments.get(id)?.clone();
        let revisions = std::mem::take(&mut self.revisions);
        let open_comments = std::mem::take(&mut self.open_comments);
        self.in_comment = true;
        let mut parts = Vec::new();
        for p in comment.children_named("p") {
            let (inline, extra) = self.paragraph_inline(p);
            let mark = self.paragraph_mark(p).filter(|_| extra.is_empty());
            let inline = inline.into_inline();
            parts.push((inline.render(true), mark, inline.edges()));
            parts.extend(extra.into_iter().map(|block| (block, None, (false, false))));
        }
        self.in_comment = false;
        self.revisions = revisions;
        self.open_comments = open_comments;
        let text = critic::join_marked(parts, " ").replace('\n', " ");
        let note = comment_note(comment.attr("author"), comment.attr("date"), &text);
        self.notes.insert(id.to_string(), note.clone());
        Some(note)
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
    fn image_names_keep_delimiter_characters_out_of_the_markup() {
        let image = r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="1" name="P" descr="x"/><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="p"><pic:blipFill><a:blip r:embed="rIdImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;
        let bytes = docx(
            &p("", &format!("<w:del>{image}</w:del>")),
            &[(
                "word/_rels/document.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/a--}~&gt;{b}&lt;.png"/></Relationships>"#,
            )],
        );
        // `{`, `}`, `<` and `>` may not appear in a URI; encoded, they cannot form a
        // delimiter, so the destination reaches the Markdown unchanged.
        assert_eq!(md(&bytes), "{--![x](a--%7D~%3E%7Bb%7D%3C.png)--}\n");
    }

    #[test]
    fn tracked_changes_become_critic_markup_without_losing_formatting() {
        let body = [
            p(
                "",
                r#"<w:r><w:t xml:space="preserve">Keep </w:t></w:r><w:del w:id="1"><w:r><w:rPr><w:i/></w:rPr><w:delText>old</w:delText></w:r></w:del><w:ins w:id="2"><w:r><w:rPr><w:b/></w:rPr><w:t>new</w:t></w:r></w:ins><w:r><w:t>.</w:t></w:r>"#,
            ),
            p(
                "",
                r#"<w:moveTo w:id="3"><w:r><w:t>destination</w:t></w:r></w:moveTo><w:r><w:t xml:space="preserve"> / </w:t></w:r><w:moveFrom w:id="3"><w:r><w:delText>source</w:delText></w:r></w:moveFrom>"#,
            ),
        ]
        .concat();

        // A deletion next to an insertion is a replacement: CriticMarkup's
        // substitution, as pandiff writes it.
        assert_eq!(
            md(&docx(&body, &[])),
            "Keep {~~_old_~>**new**~~}.\n\n{++destination++} / {--source--}\n"
        );
    }

    #[test]
    fn tracked_whole_paragraphs_keep_markdown_block_structure() {
        let body = format!(
            r#"<w:ins w:id="1">{}</w:ins><w:del w:id="2">{}</w:del>"#,
            p("Heading2", &r("Added heading")),
            p("", &r("Deleted paragraph")),
        );

        assert_eq!(
            md(&docx(&body, &[])),
            "## {++Added heading++}\n\n{--Deleted paragraph--}\n"
        );
    }

    #[test]
    fn tracked_table_rows_and_cells_keep_the_table_valid() {
        let body = format!(
            r#"<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellIns/></w:tcPr>{}</w:tc></w:tr></w:tbl>"#,
            p("", &r("Before")),
            p("", &r("After")),
            p("", &r("old")),
            p("", &r("new")),
        );

        // The inserted cell sits in a deleted row: accepting removes the row and
        // rejecting removes the cell, so the text is gone either way.
        assert_eq!(
            md(&docx(&body, &[])),
            "|Before|After|\n|-|-|\n|{--old--}|{--{++new++}--}|\n"
        );
    }

    // Tracked-change building blocks, written the way Word writes them.
    fn ins(content: &str) -> String {
        format!(
            r#"<w:ins w:id="90" w:author="Ana" w:date="2026-01-02T03:04:00Z">{content}</w:ins>"#
        )
    }

    fn del(content: &str) -> String {
        format!(
            r#"<w:del w:id="91" w:author="Ana" w:date="2026-01-02T03:04:00Z">{content}</w:del>"#
        )
    }

    fn dr(text: &str) -> String {
        format!(r#"<w:r><w:delText xml:space="preserve">{text}</w:delText></w:r>"#)
    }

    /// A paragraph whose mark (the break after it) carries `mark` (`ins`, `del`,
    /// `moveFrom`, `moveTo`, or several), with an optional style.
    fn pm(style: &str, marks: &[&str], runs: &str) -> String {
        let style = if style.is_empty() {
            String::new()
        } else {
            format!(r#"<w:pStyle w:val="{style}"/>"#)
        };
        let marks: String = marks
            .iter()
            .map(|m| format!(r#"<w:{m} w:id="92" w:author="Ana" w:date="2026-01-02T03:04:00Z"/>"#))
            .collect();
        format!("<w:p><w:pPr>{style}<w:rPr>{marks}</w:rPr></w:pPr>{runs}</w:p>")
    }

    fn text_box(inner: &str) -> String {
        format!(
            r#"<w:r><w:drawing><wp:anchor xmlns:wp="wp"><wp:docPr id="7" name="Text Box 7"/><a:graphic xmlns:a="a"><a:graphicData><wps:wsp xmlns:wps="wps"><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#
        )
    }

    #[test]
    fn review_deleted_row_with_deleted_runs_is_marked_once() {
        // Word deletes a row by marking the row, every run and every paragraph mark.
        let body = format!(
            r#"<w:tbl><w:tr><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:del w:id="1" w:author="Ana" w:date="2026-01-02T03:04:00Z"/></w:trPr><w:tc>{}</w:tc></w:tr></w:tbl>"#,
            p("", &r("Head")),
            pm("", &["del"], &del(&dr("old"))),
        );
        assert_eq!(
            md(&docx(&body, &[])),
            "|Head|\n|-|\n|{--old--}{>>Ana (2026-01-02T03:04:00Z)<<}|\n"
        );
    }

    #[test]
    fn review_text_box_inside_a_tracked_change_keeps_the_change() {
        let body = [
            p("", &del(&text_box(&p("", &r("Deleted box"))))),
            p(
                "",
                &format!("{}{}", r("Keep"), ins(&text_box(&p("", &r("Added box"))))),
            ),
        ]
        .concat();
        // The deleted box leaves no empty `{----}` paragraph behind.
        assert_eq!(
            md(&docx(&body, &[])),
            "{--Deleted box--}{>>Ana (2026-01-02T03:04:00Z)<<}\n\nKeep\n\n{++Added box++}{>>Ana (2026-01-02T03:04:00Z)<<}\n"
        );
    }

    #[test]
    fn review_tracked_paragraph_marks_mark_the_break() {
        let deleted = [pm("", &["del"], &r("A")), p("", &r("B"))].concat();
        assert_eq!(
            md(&docx(&deleted, &[])),
            "A{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}B\n"
        );
        let inserted = [pm("", &["ins"], &r("A")), p("", &r("B"))].concat();
        assert_eq!(
            md(&docx(&inserted, &[])),
            "A{++\n\n++}{>>Ana (2026-01-02T03:04:00Z)<<}B\n"
        );
        // A whole deleted paragraph: its text and its break are one deletion.
        let whole = [
            pm("", &["del"], &del(&dr("Deleted paragraph"))),
            p("", &r("Next")),
        ]
        .concat();
        assert_eq!(
            md(&docx(&whole, &[])),
            "{--Deleted paragraph\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}Next\n"
        );
    }

    #[test]
    fn review_footnote_revisions_are_marked() {
        let body = p(
            "",
            r#"<w:r><w:t>Claim</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r>"#,
        );
        let footnotes = format!(
            r#"<w:footnotes {W}><w:footnote w:id="2">{}{}</w:footnote></w:footnotes>"#,
            pm(
                "",
                &["del"],
                &format!("{}{}", r("Source"), ins(&r(" updated")))
            ),
            p("", &r("page 4")),
        );
        assert_eq!(
            md(&docx(&body, &[("word/footnotes.xml", &footnotes)])),
            "Claim[^1]\n\n[^1]: Source{++ updated++}{>>Ana (2026-01-02T03:04:00Z)<<}{-- --}{>>Ana (2026-01-02T03:04:00Z)<<}page 4\n"
        );
    }

    /// `word/comments.xml` with one `w:comment` per entry: (id, author, date,
    /// paragraphs). An empty author or date leaves the attribute out.
    fn comments(entries: &[(&str, &str, &str, &str)]) -> String {
        let body: String = entries
            .iter()
            .map(|(id, author, date, paragraphs)| {
                let author = if author.is_empty() {
                    String::new()
                } else {
                    format!(r#" w:author="{author}""#)
                };
                let date = if date.is_empty() {
                    String::new()
                } else {
                    format!(r#" w:date="{date}""#)
                };
                format!(r#"<w:comment w:id="{id}"{author}{date} w:initials="X">{paragraphs}</w:comment>"#)
            })
            .collect();
        format!(r#"<w:comments {W}>{body}</w:comments>"#)
    }

    fn start(id: &str) -> String {
        format!(r#"<w:commentRangeStart w:id="{id}"/>"#)
    }

    fn end(id: &str) -> String {
        format!(r#"<w:commentRangeEnd w:id="{id}"/>"#)
    }

    /// The reference run Word writes right after a comment's range end.
    fn reference(id: &str) -> String {
        format!(
            r#"<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="{id}"/></w:r>"#
        )
    }

    fn with_comments(body: &str, xml: &str) -> String {
        md(&docx(body, &[("word/comments.xml", xml)]))
    }

    const BILL: (&str, &str) = ("Bill Winter", "2024-04-08T10:32:00Z");

    #[test]
    fn comment_highlights_its_range_and_names_author_and_date_as_word_stores_them() {
        let body = p(
            "",
            &format!(
                "{}{}{}{}{}{}",
                r("Truth is "),
                start("0"),
                r("stranger than fiction"),
                end("0"),
                reference("0"),
                r(".")
            ),
        );
        let xml = comments(&[("0", BILL.0, BILL.1, &p("", &r("true")))]);
        assert_eq!(
            with_comments(&body, &xml),
            "Truth is {==stranger than fiction==}{>>Bill Winter (2024-04-08T10:32:00Z): true<<}.\n"
        );
    }

    #[test]
    fn comment_date_is_copied_verbatim_whatever_its_form() {
        // Fractional seconds and offsets are valid xsd:dateTime; they are kept.
        for date in [
            "2024-04-08T10:32:17.123Z",
            "2024-04-08T10:32:00+02:00",
            "2024-04-08T10:32:00",
        ] {
            let body = p("", &format!("{}{}", r("a"), reference("1")));
            let xml = comments(&[("1", "Ana", date, &p("", &r("n")))]);
            assert_eq!(
                with_comments(&body, &xml),
                format!("a{{>>Ana ({date}): n<<}}\n")
            );
        }
    }

    #[test]
    fn comment_note_leaves_out_what_is_missing() {
        assert_eq!(
            comment_note(Some("Ana"), Some("2026-01-02T03:04:00Z"), "hi"),
            "Ana (2026-01-02T03:04:00Z): hi"
        );
        assert_eq!(comment_note(Some("Ana"), None, "hi"), "Ana: hi");
        assert_eq!(
            comment_note(None, Some("2026-01-02T03:04:00Z"), "hi"),
            "(2026-01-02T03:04:00Z): hi"
        );
        assert_eq!(comment_note(Some("  "), Some(" "), "hi"), "hi");
        assert_eq!(
            comment_note(Some("Ana"), Some("2026-01-02T03:04:00Z"), ""),
            "Ana (2026-01-02T03:04:00Z)"
        );
        assert_eq!(comment_note(None, None, ""), "");
        assert_eq!(comment_note(Some("A <<} B"), None, "x"), "A <<\\} B: x");
    }

    #[test]
    fn comment_without_a_range_is_a_plain_comment_at_its_reference() {
        let body = p(
            "",
            &format!(
                "{}{}{}",
                r("Lorem ipsum dolor sit amet."),
                reference("3"),
                r(" Next")
            ),
        );
        let xml = comments(&[("3", "Ana", "", &p("", &r("This is a comment")))]);
        assert_eq!(
            with_comments(&body, &xml),
            "Lorem ipsum dolor sit amet.{>>Ana: This is a comment<<} Next\n"
        );
    }

    #[test]
    fn replies_and_overlapping_ranges_share_one_highlight() {
        // Word writes a reply as its own comment over the same range.
        let body = p(
            "",
            &[
                start("0"),
                r("a"),
                start("1"),
                start("2"),
                r("b"),
                end("0"),
                reference("0"),
                r("c"),
                end("1"),
                reference("1"),
                end("2"),
                reference("2"),
            ]
            .concat(),
        );
        let xml = comments(&[
            ("0", "Ana", "2026-01-01T09:00:00Z", &p("", &r("first"))),
            ("1", "Bo", "2026-01-01T10:00:00Z", &p("", &r("second"))),
            ("2", "Ana", "2026-01-01T11:00:00Z", &p("", &r("reply"))),
        ]);
        assert_eq!(
            with_comments(&body, &xml),
            "{==abc==}{>>Ana (2026-01-01T09:00:00Z): first<<}{>>Bo (2026-01-01T10:00:00Z): second<<}{>>Ana (2026-01-01T11:00:00Z): reply<<}\n"
        );
    }

    #[test]
    fn comment_range_across_paragraphs_highlights_each_paragraph() {
        let body = [
            p("", &format!("{}{}{}", r("one "), start("5"), r("two"))),
            p("Heading1", &r("three")),
            p(
                "",
                &format!("{}{}{}{}", r("four"), end("5"), reference("5"), r(" five")),
            ),
        ]
        .concat();
        let xml = comments(&[("5", "Ana", "", &p("", &r("long")))]);
        assert_eq!(
            with_comments(&body, &xml),
            "one {==two==}\n\n# {==three==}\n\n{==four==}{>>Ana: long<<} five\n"
        );
    }

    #[test]
    fn comment_ranges_at_block_level_and_in_tables() {
        let body = format!(
            "{}{}<w:tbl><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>{}",
            start("1"),
            p("", &r("para")),
            p("", &r("cell")),
            end("1"),
            p("", &format!("{}{}", r("after"), reference("1"))),
        );
        let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
        assert_eq!(
            with_comments(&body, &xml),
            "{==para==}\n\n|{==cell==}|\n|-|\n\nafter{>>Ana: n<<}\n"
        );
    }

    #[test]
    fn comment_without_a_reference_gets_its_note_at_the_range_end() {
        let body = p(
            "",
            &format!("{}{}{}{}", start("4"), r("x"), end("4"), r("y")),
        );
        let xml = comments(&[("4", "Ana", "", &p("", &r("n")))]);
        assert_eq!(with_comments(&body, &xml), "{==x==}{>>Ana: n<<}y\n");
    }

    #[test]
    fn unknown_or_malformed_comments_are_ignored() {
        let body = p(
            "",
            &format!(
                "{}{}{}{}{}{}{}",
                start("9"),
                r("a"),
                end("9"),
                reference("9"),
                end("8"),
                r("b"),
                r#"<w:commentRangeStart/><w:r><w:commentReference/></w:r>"#,
            ),
        );
        // Without a comments part every anchor is ignored.
        assert_eq!(md(&docx(&body, &[])), "ab\n");
        // A comment without an id cannot be referenced; one never closed stays a
        // highlight to the end of its paragraph only.
        let xml = format!(
            r#"<w:comments {W}><w:comment w:author="Ana">{}</w:comment><w:comment w:id="8" w:author="Ana">{}</w:comment></w:comments>"#,
            p("", &r("x")),
            p("", &r("y"))
        );
        let open = [
            p("", &format!("{}{}", start("8"), r("open"))),
            p("", &r("still")),
        ]
        .concat();
        assert_eq!(with_comments(&body, &xml), "ab\n");
        assert_eq!(with_comments(&open, &xml), "{==open==}\n\n{==still==}\n");
        // A range start seen twice opens once.
        let twice = p(
            "",
            &format!(
                "{}{}{}{}{}",
                start("8"),
                start("8"),
                r("x"),
                end("8"),
                reference("8")
            ),
        );
        assert_eq!(with_comments(&twice, &xml), "{==x==}{>>Ana: y<<}\n");
    }

    #[test]
    fn comment_text_keeps_formatting_joins_paragraphs_and_escapes_delimiters() {
        let body = p("", &format!("{}{}", r("a"), reference("1")));
        let text = [
            p("", r#"<w:r><w:rPr><w:b/></w:rPr><w:t>Bold</w:t></w:r><w:r><w:br/><w:t>line &lt;&lt;} end</w:t></w:r>"#),
            p("", ""),
            pm("", &["del"], &r("gone")),
            p("", &format!("{}{}", r("kept"), ins(&r(" new")))),
        ]
        .concat();
        let xml = comments(&[("1", "Ana", "", &text)]);
        assert_eq!(
            with_comments(&body, &xml),
            "a{>>Ana: **Bold** line <<\\} end gone{-- --}kept{++ new++}<<}\n"
        );
    }

    #[test]
    fn comments_meet_tracked_changes_without_breaking_either() {
        // Word wraps the reference run of a comment added with tracking on in
        // w:ins; the note must not become an insertion.
        let tracked = p(
            "",
            &format!(
                "{}{}{}{}",
                start("1"),
                r("text"),
                end("1"),
                ins(&reference("1"))
            ),
        );
        // A comment on deleted text, and a range that ends inside an insertion.
        let deleted = p(
            "",
            &format!(
                "{}{}{}{}",
                start("1"),
                del(&dr("old")),
                end("1"),
                reference("1")
            ),
        );
        let crossing = p(
            "",
            &format!(
                "{}{}{}{}",
                start("1"),
                r("a"),
                ins(&format!("{}{}{}", r("b"), end("1"), r("c"))),
                reference("1")
            ),
        );
        let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
        assert_eq!(with_comments(&tracked, &xml), "{==text==}{>>Ana: n<<}\n");
        assert_eq!(
            with_comments(&deleted, &xml),
            "{=={--old--}{>>Ana (2026-01-02T03:04:00Z)<<}==}{>>Ana: n<<}\n"
        );
        assert_eq!(
            with_comments(&crossing, &xml),
            "{==a{++b++}{>>Ana (2026-01-02T03:04:00Z)<<}==}{++c++}{>>Ana (2026-01-02T03:04:00Z)<<}{>>Ana: n<<}\n"
        );
    }

    #[test]
    fn comment_range_around_a_text_box_does_not_cover_the_box() {
        let body = p(
            "",
            &format!(
                "{}{}{}{}{}",
                start("1"),
                r("anchor"),
                text_box(&p("", &r("inside"))),
                end("1"),
                reference("1")
            ),
        );
        let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
        assert_eq!(
            with_comments(&body, &xml),
            "{==anchor==}{>>Ana: n<<}\n\ninside\n"
        );
    }

    #[test]
    fn comment_part_is_found_through_the_document_relationships() {
        let body = p("", &format!("{}{}", r("a"), reference("1")));
        let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="notes/remarks.xml"/></Relationships>"#;
        let bytes = docx(
            &body,
            &[
                ("word/_rels/document.xml.rels", rels),
                ("word/notes/remarks.xml", &xml),
            ],
        );
        assert_eq!(md(&bytes), "a{>>Ana: n<<}\n");
    }

    #[test]
    fn comments_in_footnotes_and_ranges_left_open_before_them() {
        let body = p(
            "",
            &format!(
                "{}{}{}",
                start("1"),
                r("Claim"),
                r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#
            ),
        );
        let footnotes = format!(
            r#"<w:footnotes {W}><w:footnote w:id="2">{}</w:footnote></w:footnotes>"#,
            p(
                "",
                &format!(
                    "{}{}{}{}",
                    start("2"),
                    r("Source"),
                    end("2"),
                    reference("2")
                )
            ),
        );
        let xml = comments(&[
            ("1", "Ana", "", &p("", &r("never closed"))),
            ("2", "Bo", "", &p("", &r("check"))),
        ]);
        assert_eq!(
            md(&docx(
                &body,
                &[
                    ("word/comments.xml", &xml),
                    ("word/footnotes.xml", &footnotes)
                ]
            )),
            "{==Claim[^1]==}\n\n[^1]: {==Source==}{>>Bo: check<<}\n"
        );
    }

    #[test]
    fn substitutions_merges_and_empty_changes() {
        let body = [
            // Replacement written insertion first still reads old ~> new.
            p("", &format!("{}{}{}", r("I really love "), ins(&r("font-styles")), del(&dr("fonts")))),
            // Word splits one insertion over several w:ins; they join.
            p("", &format!("{}{}{}", ins(&r("one ")), ins(&r("insertion")), r("."))),
            // Empty change elements vanish; an inserted space stays.
            p("", &format!("{}{}{}{}", r("a"), ins(""), del(&r("")), ins(&r(" ")))),
            // Inserted by one author and deleted by another.
            p("", &format!("{}{}", r("x"), ins(&del(&dr("y"))))),
            // Moves are an insertion and a deletion; next to each other they read
            // as a substitution.
            p("", r#"<w:moveFrom w:id="3"><w:r><w:delText>here</w:delText></w:r></w:moveFrom><w:moveTo w:id="4"><w:r><w:t>there</w:t></w:r></w:moveTo>"#),
        ]
        .concat();
        assert_eq!(
            md(&docx(&body, &[])),
            "I really love {~~fonts~>font-styles~~}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{++one insertion++}{>>Ana (2026-01-02T03:04:00Z)<<}.\n\na{++ ++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\nx{++{--y--}{>>Ana (2026-01-02T03:04:00Z)<<}++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{~~here~>there~~}\n"
        );
    }

    #[test]
    fn delimiters_in_document_text_do_not_open_spans() {
        // With a tracked change in the document, delimiter-like text is escaped.
        let body = p("", &r("x {++ y ++} ~> z")) + &p("", &ins(&r("new")));
        assert_eq!(
            md(&docx(&body, &[])),
            "x {\\++ y ++\\} ~\\> z\n\n{++new++}{>>Ana (2026-01-02T03:04:00Z)<<}\n"
        );
        // Without one, nothing is marked up, so the text is kept as is.
        let body = p("", &r("x {++ y ++} ~> z"));
        assert_eq!(md(&docx(&body, &[])), "x {++ y ++} ~> z\n");
    }

    #[test]
    fn tracked_changes_with_links_fields_and_hidden_runs() {
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://x.test" TargetMode="External"/></Relationships>"#;
        let body = [
            p("", &format!(r#"<w:hyperlink r:id="rId9">{}{}</w:hyperlink>"#, r("keep "), ins(&r("new")))),
            p("", &ins(r#"<w:hyperlink r:id="rId9"><w:r><w:t>added link</w:t></w:r></w:hyperlink>"#)),
            // A deleted hyperlink field keeps its target through w:delInstrText.
            p("", &del(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:delInstrText> HYPERLINK "https://f.test" </w:delInstrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:delText>old link</w:delText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#)),
            // Hidden text is not shown in Word, tracked or not.
            p("", &format!("{}{}", r("seen"), ins(r#"<w:r><w:rPr><w:vanish/></w:rPr><w:t>hidden</w:t></w:r>"#))),
        ]
        .concat();
        assert_eq!(
            md(&docx(&body, &[("word/_rels/document.xml.rels", rels)])),
            "[keep](https://x.test) {++[new](https://x.test)++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{++[added link](https://x.test)++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{--[old link](https://f.test)--}{>>Ana (2026-01-02T03:04:00Z)<<}\n\nseen\n"
        );
    }

    #[test]
    fn tracked_breaks_keep_headings_lists_and_blocks_valid() {
        let numbering = format!(
            r#"<w:numbering {W}><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="5"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
        );
        let item = |marks: &[&str], runs: &str| {
            let marks: String = marks.iter().map(|m| format!("<w:{m}/>")).collect();
            format!(
                r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="5"/></w:numPr><w:rPr>{marks}</w:rPr></w:pPr>{runs}</w:p>"#
            )
        };
        let body = [
            // Before a heading the break closes after `## `.
            pm("", &["ins"], &r("A")),
            p("Heading2", &r("B")),
            // Before a list item it closes after the marker; a deleted item keeps
            // its number, as Word shows it.
            item(&["moveFrom"], &r("one")),
            item(&[], &del(&dr("two"))),
            item(&["moveTo"], &r("three")),
            item(&[], &r("four")),
            // A blank paragraph keeps its own break; a table cannot join a paragraph.
            pm("", &["del"], &r("C")),
            p("", ""),
            pm("", &["ins", "del"], &r("D")),
            p("", &r("E")),
            pm("", &["del"], &r("F")),
            format!(
                "<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
                p("", &r("G"))
            ),
            // A paragraph holding a text box gives its break to the box.
            pm(
                "",
                &["del"],
                &format!("{}{}", r("H"), text_box(&p("", &r("box")))),
            ),
            p("", &r("I")),
            // The last paragraph's mark has no break after it.
            pm("", &["del"], &r("J")),
        ]
        .concat();
        assert_eq!(
            md(&docx(&body, &[("word/numbering.xml", &numbering)])),
            "A{++\n\n## ++}{>>Ana (2026-01-02T03:04:00Z)<<}B\n\n1. one{--\n2. two--}{>>Ana (2026-01-02T03:04:00Z)<<}\n3. three{++\n4. ++}four\n\nC\n\nD{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}E\n\nF\n\n|G|\n|-|\n\nH\n\nbox\n\nI\n\nJ\n"
        );
    }

    #[test]
    fn tracked_cells_rows_and_nested_tables() {
        let body = format!(
            r#"<w:tbl><w:tr><w:tc>{}{}{}</w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:ins/></w:trPr><w:tc><w:tbl><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel/></w:tcPr>{}</w:tc></w:tr></w:tbl></w:tc><w:tc>{}</w:tc></w:tr></w:tbl>"#,
            // Paragraph marks inside a cell.
            pm("", &["del"], &r("a")),
            pm("", &["ins"], &r("b")),
            pm("", &["del"], &r("c")),
            ins(&p("", &r("block ins"))),
            p("", &r("n1")),
            p("", &r("n2")),
            del(&p("", &r("wrapped"))),
        );
        assert_eq!(
            md(&docx(&body, &[])),
            "|a{--<br>--}{>>Ana (2026-01-02T03:04:00Z)<<}b{++<br>++}{>>Ana (2026-01-02T03:04:00Z)<<}c|{++block ins++}{>>Ana (2026-01-02T03:04:00Z)<<}|\n|-|-|\n|{++{--n1--}; {--n2--}++}|{++{--wrapped--}{>>Ana (2026-01-02T03:04:00Z)<<}++}|\n"
        );
    }

    #[test]
    fn tracked_blocks_inside_content_controls() {
        let body = ins(&format!(
            "<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt><w:customXml>{}</w:customXml>",
            p("", &r("in control")),
            p("", &r("in custom xml")),
        ));
        assert_eq!(
            md(&docx(&body, &[])),
            "{++in control++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{++in custom xml++}{>>Ana (2026-01-02T03:04:00Z)<<}\n"
        );
    }

    #[test]
    fn tracked_changes_inside_inline_wrappers_and_styled_breaks() {
        let body = [
            // Smart tags and inline content controls wrap tracked runs too.
            p(
                "",
                &format!(
                    "<w:smartTag>{}</w:smartTag><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>",
                    ins(&r("tagged")),
                    del(&dr(" boxed"))
                ),
            ),
            // A heading whose own mark is deleted joins the next paragraph.
            pm("Heading1", &["del"], &r("Title")),
            p("", &r("Body")),
            // A note reference without an id has nothing to point at.
            p(
                "",
                &format!("{}{}", r("x"), ins(r#"<w:r><w:footnoteReference/></w:r>"#)),
            ),
        ]
        .concat();
        assert_eq!(
            md(&docx(&body, &[])),
            "{~~ boxed~>tagged~~}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n# Title{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}Body\n\nx\n"
        );
    }

    #[test]
    fn comment_without_an_author_shows_only_its_date() {
        let body = p("", &format!("{}{}", r("a"), reference("1")));
        let xml = comments(&[("1", "", "2026-01-02T03:04:00Z", &p("", &r("anonymous")))]);
        assert_eq!(
            with_comments(&body, &xml),
            "a{>>(2026-01-02T03:04:00Z): anonymous<<}\n"
        );
    }

    #[test]
    fn text_boxes_and_content_controls_inside_tracked_cells() {
        let body = format!(
            r#"<w:tbl><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt></w:tc></w:tr></w:tbl>"#,
            // The box's own paragraph mark cannot join the box text.
            pm(
                "",
                &["del"],
                &format!("{}{}", r("cell"), text_box(&p("", &r("box"))))
            ),
            p("", &r("control")),
        );
        assert_eq!(
            md(&docx(&body, &[])),
            "|{--cell<br>box--}|{--control--}|\n|-|-|\n"
        );
    }

    #[test]
    fn deleted_paragraph_mark_preserves_text_and_marks_only_the_separator() {
        let body = r#"<w:p><w:pPr><w:rPr><w:del w:id="1"/></w:rPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
<w:p><w:r><w:t>Second</w:t></w:r></w:p>
<w:p><w:r><w:t>Third</w:t></w:r></w:p>"#;
        assert_eq!(md(&docx(body, &[])), "First{--\n\n--}Second\n\nThird\n");
    }

    #[test]
    fn deleted_text_boxes_keep_block_structure_without_an_empty_paragraph() {
        let content = r#"<w:txbxContent>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr><w:del><w:r><w:delText>Item</w:delText></w:r></w:del></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:txbxContent>"#;
        let numbering = format!(
            r#"<w:numbering {W}><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
        );
        let expected = "## {--Heading--}\n\n- {--Item--}\n\n|{--Cell--}|\n|-|\n";
        for drawing in ["drawing", "pict", "object"] {
            let run = format!("<w:r><w:{drawing}>{content}</w:{drawing}></w:r>");
            for body in [
                p("", &format!("<w:del>{run}</w:del>")),
                format!("<w:del>{}</w:del>", p("", &run)),
                p(
                    "",
                    &format!(
                        r#"<w:del><w:hyperlink w:anchor="bookmark"><w:sdt><w:sdtContent>{run}</w:sdtContent></w:sdt></w:hyperlink></w:del>"#
                    ),
                ),
            ] {
                assert_eq!(
                    md(&docx(&body, &[("word/numbering.xml", &numbering)])),
                    expected
                );
            }
        }
        let body = p("", &format!("{}<w:del><w:r><w:delText>Old</w:delText><w:drawing>{content}</w:drawing></w:r></w:del>", r("Keep ")));
        assert_eq!(
            md(&docx(&body, &[("word/numbering.xml", &numbering)])),
            format!("Keep {{--Old--}}\n\n{expected}")
        );
    }

    #[test]
    fn deleted_row_with_inline_deletion_has_no_nested_delimiters() {
        let body = r#"<w:tbl>
<w:tr><w:tc><w:p><w:r><w:t>Header</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:del w:id="1"/></w:trPr><w:tc><w:p>
<w:r><w:t xml:space="preserve">Before </w:t></w:r>
<w:del w:id="2"><w:r><w:rPr><w:b/></w:rPr><w:delText>old</w:delText></w:r></w:del>
<w:r><w:t xml:space="preserve"> after</w:t></w:r>
</w:p></w:tc></w:tr></w:tbl>"#;
        assert_eq!(
            md(&docx(body, &[])),
            "|Header|\n|-|\n|{--Before **old** after--}|\n"
        );
    }

    /// A revision element with an explicit author and date (either may be empty
    /// to leave the attribute out).
    fn by(tag: &str, author: &str, date: &str, content: &str) -> String {
        let author = if author.is_empty() {
            String::new()
        } else {
            format!(r#" w:author="{author}""#)
        };
        let date = if date.is_empty() {
            String::new()
        } else {
            format!(r#" w:date="{date}""#)
        };
        format!(r#"<w:{tag} w:id="7"{author}{date}>{content}</w:{tag}>"#)
    }

    #[test]
    fn every_tracked_change_names_its_author_and_date_as_word_stores_them() {
        let t1 = "2026-09-29T14:05:00Z";
        let t2 = "2026-09-29T15:10:00Z";
        let body = [
            // One person's replacement: one note after the substitution.
            p(
                "",
                &format!(
                    "{}{}",
                    by("del", "Ana Lima", t1, &dr("fonts")),
                    by("ins", "Ana Lima", t1, &r("styles"))
                ),
            ),
            // Two people's: both notes, old side first.
            p(
                "",
                &format!(
                    "{}{}",
                    by("del", "Ana Lima", t1, &dr("old")),
                    by("ins", "Bo Chen", t2, &r("new"))
                ),
            ),
            // The same author at two times stays two changes.
            p(
                "",
                &format!(
                    "{}{}",
                    by("ins", "Ana Lima", t1, &r("a")),
                    by("ins", "Ana Lima", t2, &r("b"))
                ),
            ),
            // Moves carry their own attribution.
            p("", &by("moveTo", "Bo Chen", t2, &r("moved here"))),
            // Missing date, missing author, neither, and a name that needs escaping.
            p(
                "",
                &format!(
                    "{}{}{}{}",
                    by("ins", "Ana Lima", "", &r("w")),
                    by("del", "", t1, &dr("x")),
                    by("ins", "", "", &r("y")),
                    by("ins", "A <<} B", t1, &r("z")),
                ),
            ),
        ]
        .concat();
        assert_eq!(
            md(&docx(&body, &[])),
            "{~~fonts~>styles~~}{>>Ana Lima (2026-09-29T14:05:00Z)<<}\n\n\
             {~~old~>new~~}{>>Ana Lima (2026-09-29T14:05:00Z)<<}{>>Bo Chen (2026-09-29T15:10:00Z)<<}\n\n\
             {++a++}{>>Ana Lima (2026-09-29T14:05:00Z)<<}{++b++}{>>Ana Lima (2026-09-29T15:10:00Z)<<}\n\n\
             {++moved here++}{>>Bo Chen (2026-09-29T15:10:00Z)<<}\n\n\
             {~~x~>w~~}{>>(2026-09-29T14:05:00Z)<<}{>>Ana Lima<<}{++y++}{++z++}{>>A <<\\} B (2026-09-29T14:05:00Z)<<}\n"
        );
    }

    #[test]
    fn paragraph_breaks_rows_and_cells_carry_their_own_attribution() {
        let t1 = "2026-09-29T14:05:00Z";
        let t2 = "2026-09-29T15:10:00Z";
        let mark = |author: &str, date: &str| {
            format!(
                r#"<w:pPr><w:rPr><w:del w:id="8" w:author="{author}" w:date="{date}"/></w:rPr></w:pPr>"#
            )
        };
        let body = [
            // Text deleted by Ana, the break after it by Bo: two changes.
            format!("<w:p>{}{}</w:p>", mark("Bo Chen", t2), by("del", "Ana Lima", t1, &dr("A"))),
            p("", &r("Next")),
            // Text and break both deleted by Ana at t1: one change.
            format!("<w:p>{}{}</w:p>", mark("Ana Lima", t1), by("del", "Ana Lima", t1, &dr("B"))),
            p("", &by("del", "Ana Lima", t1, &dr("C"))),
            format!(
                r#"<w:tbl><w:tr><w:trPr><w:ins w:id="9" w:author="Bo Chen" w:date="{t2}"/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel w:id="10" w:author="Ana Lima" w:date="{t1}"/></w:tcPr>{}</w:tc></w:tr></w:tbl>"#,
                p("", &r("row")),
                p("", &r("cell")),
            ),
        ]
        .concat();
        assert_eq!(
            md(&docx(&body, &[])),
            "{--A--}{>>Ana Lima (2026-09-29T14:05:00Z)<<}{--\n\n--}{>>Bo Chen (2026-09-29T15:10:00Z)<<}Next\n\n\
             {--B\n\nC--}{>>Ana Lima (2026-09-29T14:05:00Z)<<}\n\n\
             |{++row++}{>>Bo Chen (2026-09-29T15:10:00Z)<<}|{++{--cell--}{>>Ana Lima (2026-09-29T14:05:00Z)<<}++}{>>Bo Chen (2026-09-29T15:10:00Z)<<}|\n|-|-|\n"
        );
    }

    #[test]
    fn footnotes_in_comment_text_are_numbered_where_the_comment_is() {
        let body = [
            p(
                "",
                r#"<w:r><w:t>B</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r>"#,
            ),
            p("", &format!("{}{}", r("A"), reference("1"))),
        ]
        .concat();
        let footnotes = format!(
            r#"<w:footnotes {W}><w:footnote w:id="2">{}</w:footnote><w:footnote w:id="3">{}</w:footnote></w:footnotes>"#,
            p("", &r("body note")),
            p("", &r("comment note")),
        );
        let xml = comments(&[(
            "1",
            "Ana",
            "",
            &p(
                "",
                r#"<w:r><w:t>see</w:t></w:r><w:r><w:footnoteReference w:id="3"/></w:r>"#,
            ),
        )]);
        assert_eq!(
            md(&docx(
                &body,
                &[
                    ("word/comments.xml", &xml),
                    ("word/footnotes.xml", &footnotes)
                ]
            )),
            "B[^1]\n\nA{>>Ana: see[^2]<<}\n\n[^1]: body note\n[^2]: comment note\n"
        );
    }

    #[test]
    fn a_range_ending_between_paragraphs_keeps_its_note() {
        let xml = comments(&[
            ("1", "Ana", "", &p("", &r("first"))),
            ("2", "Bo", "", &p("", &r("last"))),
        ]);
        let body = [
            p("", &format!("{}{}", start("1"), r("a"))),
            end("1"),
            p("", &r("b")),
            p("", &format!("{}{}", start("2"), r("c"))),
            end("2"),
        ]
        .concat();
        assert_eq!(
            with_comments(&body, &xml),
            "{==a==}\n\n{>>Ana: first<<}b\n\n{==c==}\n\n{>>Bo: last<<}\n"
        );
    }

    #[test]
    fn a_note_between_paragraph_and_table_stays_out_of_the_table() {
        let xml = comments(&[("1", "Ana", "", &p("", &r("first")))]);
        let table = |cell: &str| format!("<w:tbl><w:tr><w:tc>{cell}</w:tc></w:tr></w:tbl>");
        let body = [
            p("", &format!("{}{}", start("1"), r("a"))),
            end("1"),
            table(&p("", &r("cell"))),
        ]
        .concat();
        assert_eq!(
            with_comments(&body, &xml),
            "{==a==}\n\n{>>Ana: first<<}\n\n|cell|\n|-|\n"
        );
        // A deleted paragraph mark before the note keeps its markup.
        let body = [
            pm("", &["del"], &format!("{}{}", start("1"), r("a"))),
            end("1"),
            table(&p("", &r("cell"))),
        ]
        .concat();
        assert_eq!(
            with_comments(&body, &xml),
            "{==a==}{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}{>>Ana: first<<}\n\n|cell|\n|-|\n"
        );
    }

    #[test]
    fn comment_text_keeps_text_boxes_without_nesting_comments() {
        let inner = [
            p(
                "",
                &by("ins", "Bo Chen", "2026-09-29T15:10:00Z", &r("boxed")),
            ),
            p("", &format!("{}{}", r("with a note"), reference("2"))),
        ]
        .concat();
        let text = p(
            "",
            &format!("{}{}{}", r("see "), text_box(&inner), reference("2")),
        );
        let xml = comments(&[
            ("1", "Ana", "", &text),
            ("2", "Bo", "", &p("", &r("inner"))),
        ]);
        let body = p(
            "",
            &format!("{}{}{}{}", start("1"), r("x"), end("1"), reference("1")),
        );
        assert_eq!(
            with_comments(&body, &xml),
            "{==x==}{>>Ana: see {++boxed++} with a note<<}\n"
        );
    }

    #[test]
    fn a_comment_used_twice_is_written_once_and_its_footnote_counted_once() {
        let body = [
            p("", &format!("{}{}", r("A"), reference("1"))),
            p("", &format!("{}{}", r("B"), reference("1"))),
        ]
        .concat();
        let footnotes = format!(
            r#"<w:footnotes {W}><w:footnote w:id="3">{}</w:footnote></w:footnotes>"#,
            p("", &r("source")),
        );
        let xml = comments(&[(
            "1",
            "Ana",
            "",
            &p(
                "",
                r#"<w:r><w:t>see</w:t></w:r><w:r><w:footnoteReference w:id="3"/></w:r>"#,
            ),
        )]);
        assert_eq!(
            md(&docx(
                &body,
                &[
                    ("word/comments.xml", &xml),
                    ("word/footnotes.xml", &footnotes)
                ]
            )),
            "A{>>Ana: see[^1]<<}\n\nB{>>Ana: see[^1]<<}\n\n[^1]: source\n"
        );
    }

    fn resolved(bytes: &[u8], revisions: Revisions) -> String {
        let options = Options {
            revisions,
            ..Options::default()
        };
        convert(bytes, &options).unwrap().sections[0]
            .markdown
            .clone()
    }

    #[test]
    fn accept_and_reject_resolve_paragraphs_breaks_and_moves() {
        let body = [
            pm("", &["del"], &del(&r("Gone"))),
            p("", &r("Next")),
            p("", &format!("{}{}", ins(&r("New ")), r("text"))),
            p("", &by("moveFrom", "Bo", "", &r("Moved away"))),
            p("", &by("moveTo", "Bo", "", &r("Moved here"))),
            pm("", &["ins"], &r("One")),
            p("", &r("two")),
        ]
        .concat();
        let bytes = docx(&body, &[]);
        assert_eq!(
            resolved(&bytes, Revisions::Accept),
            "Next\n\nNew text\n\nMoved here\n\nOne\n\ntwo\n"
        );
        assert_eq!(
            resolved(&bytes, Revisions::Reject),
            "Gone\n\nNext\n\ntext\n\nMoved away\n\nOnetwo\n"
        );
    }

    #[test]
    fn a_joined_paragraph_takes_the_later_paragraphs_properties() {
        // Word keeps paragraph properties on the mark that ends the paragraph,
        // so deleting a break gives the joined text the second one's format.
        let heading =
            r#"<w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>"#;
        let body = format!("{}{heading}", pm("", &["del"], &r("Intro ")));
        let bytes = docx(&body, &[]);
        assert_eq!(resolved(&bytes, Revisions::Accept), "# Intro Title\n");
        assert_eq!(resolved(&bytes, Revisions::Reject), "Intro\n\n# Title\n");
    }

    #[test]
    fn accept_and_reject_resolve_rows_cells_and_nested_tables() {
        let body = format!(
            r#"<w:tbl><w:tr><w:tc>{}{}{}</w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:ins/></w:trPr><w:tc><w:tbl><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel/></w:tcPr>{}</w:tc></w:tr></w:tbl></w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel/></w:tcPr>{}</w:tc><w:tc><w:tcPr><w:cellIns/></w:tcPr>{}</w:tc></w:tr></w:tbl>"#,
            pm("", &["del"], &r("a")),
            pm("", &["ins"], &r("b")),
            pm("", &["del"], &r("c")),
            ins(&p("", &r("block ins"))),
            p("", &r("n1")),
            p("", &r("n2")),
            del(&p("", &r("wrapped"))),
            p("", &r("x")),
            p("", &r("gone")),
            p("", &r("added")),
        );
        let bytes = docx(&body, &[]);
        assert_eq!(
            resolved(&bytes, Revisions::Accept),
            "|ab<br>c|block ins|\n|-|-|\n|x|added|\n"
        );
        assert_eq!(
            resolved(&bytes, Revisions::Reject),
            "|a<br>bc||\n|-|-|\n|x|gone|\n"
        );
    }

    #[test]
    fn accept_and_reject_resolve_text_boxes_and_footnotes() {
        let body = [
            p("", &del(&text_box(&p("", &r("Deleted box"))))),
            p(
                "",
                &format!(
                    "{}{}{}",
                    r("Keep"),
                    ins(&text_box(&p("", &r("Added box")))),
                    r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#
                ),
            ),
        ]
        .concat();
        let footnotes = format!(
            r#"<w:footnotes {W}><w:footnote w:id="2">{}{}</w:footnote></w:footnotes>"#,
            pm(
                "",
                &["del"],
                &format!("{}{}", r("Source"), ins(&r(" updated")))
            ),
            p("", &r("page 4")),
        );
        let bytes = docx(&body, &[("word/footnotes.xml", &footnotes)]);
        assert_eq!(
            resolved(&bytes, Revisions::Accept),
            "Keep[^1]\n\nAdded box\n\n[^1]: Source updatedpage 4\n"
        );
        assert_eq!(
            resolved(&bytes, Revisions::Reject),
            "Deleted box\n\nKeep[^1]\n\n[^1]: Source page 4\n"
        );
    }

    #[test]
    fn reject_restores_formatting_that_a_tracked_change_replaced() {
        let run = r#"<w:r><w:rPr><w:b/><w:rPrChange w:id="1" w:author="Ana"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>styled</w:t></w:r>"#;
        let bytes = docx(&p("", run), &[]);
        // A formatting change alone is not shown as markup.
        assert_eq!(md(&bytes), "**styled**\n");
        assert_eq!(resolved(&bytes, Revisions::Accept), "**styled**\n");
        assert_eq!(resolved(&bytes, Revisions::Reject), "_styled_\n");
        // LibreOffice records a change from plain text with no earlier `w:rPr`
        // inside: rejecting it leaves the text plain.
        let run = r#"<w:r><w:rPr><w:b/><w:rPrChange w:id="1" w:author="Ana"></w:rPrChange></w:rPr><w:t>styled</w:t></w:r>"#;
        let bytes = docx(&p("", run), &[]);
        assert_eq!(resolved(&bytes, Revisions::Accept), "**styled**\n");
        assert_eq!(resolved(&bytes, Revisions::Reject), "styled\n");
    }

    #[test]
    fn accept_and_reject_leave_comments_out() {
        let body = p(
            "",
            &format!("{}{}{}{}", start("1"), r("noted"), end("1"), reference("1")),
        );
        let xml = comments(&[("1", "Ana", "", &p("", &r("why?")))]);
        let bytes = docx(&body, &[("word/comments.xml", &xml)]);
        assert!(md(&bytes).contains("{==noted==}"), "{}", md(&bytes));
        assert_eq!(resolved(&bytes, Revisions::Accept), "noted\n");
        assert_eq!(resolved(&bytes, Revisions::Reject), "noted\n");
    }

    #[test]
    fn a_note_whose_reference_is_changed_is_changed_too() {
        let reference = |id: &str| format!(r#"<w:r><w:footnoteReference w:id="{id}"/></w:r>"#);
        let body = [
            p("", &(r("Kept") + &reference("2"))),
            p("", &ins(&(r("Added") + &reference("3")))),
            p("", &del(&(dr("Gone") + &reference("4")))),
        ]
        .concat();
        let footnotes = format!(
            r#"<w:footnotes {W}><w:footnote w:id="2">{}</w:footnote><w:footnote w:id="3">{}</w:footnote><w:footnote w:id="4">{}</w:footnote></w:footnotes>"#,
            p("", &r("Plain note.")),
            p("", &r("Added note.")),
            p("", &format!("{}{}", r("Gone "), ins(&r("note")))),
        );
        let bytes = docx(&body, &[("word/footnotes.xml", &footnotes)]);
        let by = "{>>Ana (2026-01-02T03:04:00Z)<<}";
        assert_eq!(
            md(&bytes),
            format!(
                "Kept[^1]\n\n{{++Added[^2]++}}{by}\n\n{{--Gone[^3]--}}{by}\n\n[^1]: Plain note.\n{{++[^2]: Added note.++}}{by}\n{{--[^3]: Gone {{++note++}}{by}--}}{by}\n"
            )
        );
        // Resolved first, the notes are numbered as they are left.
        assert_eq!(
            resolved(&bytes, Revisions::Reject),
            "Kept[^1]\n\nGone[^2]\n\n[^1]: Plain note.\n[^2]: Gone\n"
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
