//! Accepts or rejects every Word tracked change in a parsed part, so it
//! converts as the text Word shows after Accept All or Reject All.
//!
//! Content changes (`w:ins`, `w:del`, `w:moveTo`, `w:moveFrom`) are kept
//! without their wrapper or dropped. A changed paragraph mark that goes away
//! joins its paragraph with the next one, which keeps the next one's
//! properties, as in Word. Changed table rows and cells are kept or dropped.
//! On reject, formatting changes (`w:rPrChange`, `w:pPrChange`...) restore the
//! earlier properties.

use crate::ooxml::{Element, Node};

/// The part with every tracked change accepted (`accept`) or rejected.
pub(crate) fn resolve(element: &Element, accept: bool) -> Element {
    let mut out = element.clone();
    resolve_in(&mut out, accept);
    out
}

/// Content that goes away: deletions on accept, insertions on reject.
fn removed(local: &str, accept: bool) -> bool {
    if accept {
        matches!(local, "del" | "moveFrom" | "cellDel")
    } else {
        matches!(local, "ins" | "moveTo" | "cellIns")
    }
}

/// Content that stays, without its wrapper.
fn kept(local: &str, accept: bool) -> bool {
    if accept {
        matches!(local, "ins" | "moveTo")
    } else {
        matches!(local, "del" | "moveFrom")
    }
}

/// A revision marker inside a properties element (`trPr`, `tcPr`, a
/// paragraph mark's `rPr`), or a record of a formatting change.
fn is_marker(local: &str) -> bool {
    matches!(
        local,
        "ins" | "del" | "moveTo" | "moveFrom" | "cellIns" | "cellDel" | "cellMerge"
    ) || local.ends_with("PrChange")
}

/// Whether a properties element marks its row, cell or paragraph break as
/// going away.
fn marked_removed(properties: Option<&Element>, accept: bool) -> bool {
    properties.is_some_and(|p| p.elements().any(|m| removed(m.local(), accept)))
}

fn resolve_in(element: &mut Element, accept: bool) {
    let properties = element.local().ends_with("Pr");
    if properties && !accept {
        restore_earlier(element);
    }
    let mut children = Vec::with_capacity(element.children.len());
    for child in std::mem::take(&mut element.children) {
        let Node::Element(mut child) = child else {
            children.push(child);
            continue;
        };
        let local = child.local().to_string();
        if properties && is_marker(&local) {
            continue;
        }
        let gone = match local.as_str() {
            "tr" => marked_removed(child.child("trPr"), accept),
            "tc" => marked_removed(child.child("tcPr"), accept),
            _ => removed(&local, accept),
        };
        if gone {
            continue;
        }
        if local == "p" && marked_removed(child.path(&["pPr", "rPr"]), accept) {
            child.attrs.push((BREAK_GONE.to_string(), String::new()));
        }
        resolve_in(&mut child, accept);
        if kept(&local, accept) {
            children.extend(child.children);
        } else {
            children.push(Node::Element(child));
        }
    }
    element.children = join_paragraphs(children);
}

/// On reject, puts back the properties a formatting change replaced. A
/// paragraph keeps its mark's run properties and its section, which
/// `w:pPrChange` does not record.
fn restore_earlier(properties: &mut Element) {
    let change = format!("{}Change", properties.local());
    let Some(earlier) = properties
        .child(&change)
        .and_then(|c| c.elements().find(|e| e.local() == properties.local()))
        .cloned()
    else {
        return;
    };
    let current = std::mem::take(&mut properties.children);
    properties.children = earlier.children;
    properties.children.extend(current.into_iter().filter(
        |node| matches!(node, Node::Element(e) if matches!(e.local(), "rPr" | "sectPr")),
    ));
}

/// Joins each paragraph whose break goes away with the paragraph after it.
/// The break's marker is gone once the paragraph is resolved, so it was
/// noted beforehand as the [`BREAK_GONE`] attribute.
fn join_paragraphs(children: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(children.len());
    // The paragraph the next one joins, as an index into `out`.
    let mut joining: Option<usize> = None;
    for node in children {
        let mut p = match node {
            Node::Element(p) if p.is("p") => p,
            Node::Element(other) => {
                joining = None;
                out.push(Node::Element(other));
                continue;
            }
            text => {
                out.push(text);
                continue;
            }
        };
        let gone = p.attrs.iter().any(|(name, _)| name == BREAK_GONE);
        p.attrs.retain(|(name, _)| name != BREAK_GONE);
        let at = match joining.and_then(|at| match &mut out[at] {
            Node::Element(before) => Some((at, before)),
            Node::Text(_) => None,
        }) {
            Some((at, before)) => {
                // The joined paragraph takes the later paragraph's properties.
                let (properties, rest): (Vec<Node>, Vec<Node>) = p
                    .children
                    .into_iter()
                    .partition(|n| matches!(n, Node::Element(e) if e.is("pPr")));
                let content = std::mem::take(&mut before.children)
                    .into_iter()
                    .filter(|n| !matches!(n, Node::Element(e) if e.is("pPr")));
                before.children = properties.into_iter().chain(content).chain(rest).collect();
                at
            }
            None => {
                out.push(Node::Element(p));
                out.len() - 1
            }
        };
        joining = gone.then_some(at);
    }
    out
}

/// Set on a paragraph, before it is resolved, when its break goes away.
const BREAK_GONE: &str = "anymd:break-gone";
