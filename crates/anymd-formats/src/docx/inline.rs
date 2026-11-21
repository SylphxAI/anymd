//! Inline text, fields, links, and drawings within a paragraph.

use super::math::math;
use super::review::change_of;
use super::Writer;
use crate::critic::{Critic, Mark};
use crate::ooxml::{Blocks, Element, ListIndent};

/// An open complex field (`fldChar begin … separate … end`).
struct Field {
    instruction: String,
    in_result: bool,
    link: Option<String>,
}

impl Writer<'_> {
    /// Inline content of a paragraph plus any text-box blocks found inside it.
    /// Tracked changes around the paragraph and comment ranges still open from
    /// earlier paragraphs are opened again at its start.
    pub(super) fn paragraph_inline(&mut self, p: &Element) -> (Critic, Vec<String>) {
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
                    let number = self.reference_note(child.is("endnoteReference"), id);
                    out.raw(&format!("[^{number}]"));
                }
            }
            "commentReference" if !hidden && !self.in_comment => {
                if let Some(note) = child.attr("id").and_then(|id| self.render_comment(id)) {
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
            match blocks.finish() {
                Ok(text) => extra.push(text),
                Err(error) => {
                    self.failure = Some(error);
                    break;
                }
            }
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
