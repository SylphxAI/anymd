//! Tracked changes and comment ranges rendered as CriticMarkup.

use std::collections::HashMap;

use super::Writer;
use crate::critic::{self, Change, Critic, Mark};
use crate::ooxml::Element;

/// Whether a part holds a tracked change, or a comment anchor for a comment
/// that exists. Formatting-only changes (`w:rPrChange`...) do not count: they
/// are not shown.
pub(super) fn has_markup(part: &Element, comments: &HashMap<String, Element>) -> bool {
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
pub(super) fn change_of(element: &Element) -> Change {
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
pub(super) fn revision_marks(properties: &Element) -> Vec<Change> {
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
pub(super) fn paragraph_mark(p: &Element) -> Option<Change> {
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
pub(super) fn comment_note(author: Option<&str>, date: Option<&str>, text: &str) -> String {
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

impl Writer<'_> {
    /// The notes waiting for the next paragraph, as CriticMarkup comments.
    pub(super) fn take_notes(&mut self) -> String {
        std::mem::take(&mut self.pending_notes)
            .iter()
            .map(|note| format!("{{>>{note}<<}}"))
            .collect()
    }

    /// Writes content inside a tracked change.
    pub(super) fn revised(&mut self, change: Change, write: impl FnOnce(&mut Self)) {
        self.revisions.push(change);
        write(self);
        self.revisions.pop();
    }

    /// A comment range start or end. The commented text is highlighted; a range
    /// that spans paragraphs gets one highlight per paragraph, because
    /// CriticMarkup cannot cross a block.
    pub(super) fn comment_range(&mut self, range: &Element, out: Option<&mut Critic>) {
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
                    .then(|| self.render_comment(id))
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

    /// The tracked change on a paragraph's mark, without its author inside a
    /// comment's own text.
    pub(super) fn paragraph_mark(&self, p: &Element) -> Option<Change> {
        paragraph_mark(p).map(|(mark, by)| (mark, by.filter(|_| !self.in_comment)))
    }

    /// A comment's note, rendered the first time it is needed. Its text is
    /// written apart from the document around its anchor: no tracked change
    /// or comment range there applies to it.
    pub(super) fn render_comment(&mut self, id: &str) -> Option<String> {
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
