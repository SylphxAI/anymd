use crate::ooxml::{Blocks, Element, Inline, ListIndent, Rels};

use super::{paragraph, placeholder, Context};

impl Context<'_> {
    pub(super) fn text_shape(&self, shape: &Element, title: bool, blocks: &mut Blocks) {
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
}

pub(super) fn runs(container: &Element, inline: &mut Inline, rels: &Rels) {
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
