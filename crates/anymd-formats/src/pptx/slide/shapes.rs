use std::cell::RefCell;
use std::collections::HashMap;

use crate::ooxml::{Blocks, Element, Media, Rels};
use crate::ConvertError;

use super::{offset, placeholder, Positions};

pub(super) struct Context<'a> {
    pub(super) failure: RefCell<Option<ConvertError>>,
    pub(super) rels: &'a Rels,
    pub(super) positions: &'a Positions,
    pub(super) charts: &'a HashMap<String, Element>,
    pub(super) media: &'a Media,
    pub(super) slide: usize,
}

/// A shape to render, with its reading position when known.
struct Item<'e> {
    element: &'e Element,
    title: bool,
    position: Option<(i64, i64)>,
}

impl Context<'_> {
    pub(super) fn tree(&self, tree: &Element, blocks: &mut Blocks) {
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
            "sp" => self.text_shape(shape, title, blocks),
            "graphicFrame" => self.graphic_frame(shape, blocks),
            "pic" => self.picture(shape, blocks),
            _ => {}
        }
    }
}
