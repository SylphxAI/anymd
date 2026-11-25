use std::{cell::RefCell, collections::HashMap};

use crate::ooxml::{Blocks, Element, Inline, Media, Package, Rels};
use crate::ConvertError;

use super::extras::{load_charts, notes_text};
use super::{layout_positions, Positions, SlideImages};

mod embeds;
mod shapes;
mod text;

use shapes::Context;

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

pub(super) fn paragraph(p: &Element, rels: &Rels) -> Inline {
    let mut inline = Inline::default();
    text::runs(p, &mut inline, rels);
    inline
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
