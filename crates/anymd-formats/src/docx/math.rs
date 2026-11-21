//! Office Math (OMML) expressions as LaTeX.

use crate::ooxml::{self, Element};

/// Office Math (OMML) to LaTeX, covering the structures Word's equation editor emits.
pub(super) fn math(element: &Element) -> String {
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
