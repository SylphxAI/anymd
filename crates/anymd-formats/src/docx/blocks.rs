//! Body traversal and Markdown paragraph, heading, and list layout.

use super::numbering::num_pr;
use super::review::change_of;
use super::Writer;
use crate::ooxml::{Blocks, Element, ListIndent};

impl Writer<'_> {
    pub(super) fn blocks(
        &mut self,
        container: &Element,
        blocks: &mut Blocks,
        list: &mut ListIndent,
    ) {
        for child in container.elements() {
            if blocks.is_full() || self.failure.is_some() {
                break;
            }
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
            } else if let Some(marker) = num.and_then(|(id, ilvl)| {
                self.numbering
                    .list_marker(&id, ilvl.min(8))
                    .map(|m| (m, ilvl))
            }) {
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
}
