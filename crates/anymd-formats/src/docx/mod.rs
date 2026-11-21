//! DOCX → Markdown: headings, emphasis, links, lists, tables, footnotes, and
//! tracked changes and comments as CriticMarkup.

use std::collections::{HashMap, HashSet};

use crate::critic::Change;
use crate::ooxml::{self, Blocks, Element, ListIndent, Media, Package, Rels};
use crate::revise;
use crate::{ConvertError, Converted, Options, Revisions, Section};

use self::numbering::Numbering;
use self::review::has_markup;
use self::styles::Styles;

mod blocks;
mod inline;
mod math;
mod notes;
mod numbering;
mod review;
mod styles;
mod tables;

#[cfg(test)]
mod tests;

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    convert_with_output_limit(bytes, options, crate::limits::MAX_OUTPUT_BYTES)
}

fn convert_with_output_limit(
    bytes: &[u8],
    options: &Options,
    output_limit: usize,
) -> Result<Converted, ConvertError> {
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
    package.limits.check()?;
    let mut writer = Writer {
        failure: None,
        rels: &rels,
        media,
        styles,
        numbering,
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
    // Reserve the final newline before any body or definition is retained.
    let mut blocks = Blocks::with_limit(output_limit.saturating_sub(1));
    let mut list = ListIndent::default();
    writer.blocks(body, &mut blocks, &mut list);
    // Notes of ranges that ended after the last paragraph.
    let trailing = writer.take_notes();
    blocks.push(&trailing, false);

    if let Some(error) = writer.failure.take() {
        return Err(error);
    }
    let mut markdown = blocks.finish()?;
    writer.append_definitions(&mut markdown, &notes, output_limit)?;
    if !markdown.is_empty() {
        markdown.push('\n');
    }

    package.limits.output(&markdown)?;
    Ok(Converted {
        outline: Vec::new(),
        format: "docx".into(),
        title,
        sections: vec![Section {
            label: "document".into(),
            markdown,
        }],
        metadata,
    })
}

struct Writer<'a> {
    failure: Option<ConvertError>,
    rels: &'a Rels,
    media: Media,
    styles: Styles,
    numbering: Numbering,
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
