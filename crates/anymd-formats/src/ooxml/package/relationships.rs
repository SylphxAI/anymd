use std::collections::HashMap;

use crate::ConvertError;

use super::Package;

impl Package<'_> {
    /// Relationships of a part (`dir/_rels/file.rels`), keyed by id.
    pub(crate) fn rels(&mut self, part: &str) -> Result<Rels, ConvertError> {
        let (dir, file) = split_part(part);
        let rels_name = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let mut rels = Rels::default();
        let Some(root) = self.xml(&rels_name)? else {
            return Ok(rels);
        };
        for rel in root.children_named("Relationship") {
            let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else {
                continue;
            };
            let external = rel
                .attr("TargetMode")
                .is_some_and(|m| m.eq_ignore_ascii_case("External"));
            let target = if external {
                target.to_string()
            } else {
                resolve_target(dir, target)
            };
            rels.by_id.insert(
                id.to_string(),
                Rel {
                    kind: rel.attr("Type").unwrap_or_default().to_string(),
                    target,
                    external,
                },
            );
            rels.order.push(id.to_string());
        }
        Ok(rels)
    }

    /// The main part named by the package-level officeDocument relationship.
    pub(crate) fn main_part(&mut self, fallback: &str) -> Result<String, ConvertError> {
        let rels = self.rels("")?;
        let found = rels
            .first_of_type("/officeDocument")
            .map(|r| r.target.clone());
        Ok(found
            .filter(|t| self.has(t))
            .unwrap_or_else(|| fallback.to_string()))
    }
}

fn split_part(part: &str) -> (&str, &str) {
    let part = part.trim_start_matches('/');
    match part.rfind('/') {
        Some(i) => (&part[..i], &part[i + 1..]),
        None => ("", part),
    }
}

/// Resolve a relationship target relative to the source part's directory.
pub(in crate::ooxml) fn resolve_target(dir: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or_default();
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[derive(Debug, Clone)]
pub(crate) struct Rel {
    pub kind: String,
    pub target: String,
    pub external: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Rels {
    by_id: HashMap<String, Rel>,
    order: Vec<String>,
}

impl Rels {
    pub(crate) fn get(&self, id: &str) -> Option<&Rel> {
        self.by_id.get(id)
    }

    pub(crate) fn first_of_type(&self, suffix: &str) -> Option<&Rel> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .find(|r| r.kind.ends_with(suffix))
    }

    /// Walk relationship IDs in their original order, looking up each current value.
    pub(in crate::ooxml) fn ordered(&self) -> impl Iterator<Item = &Rel> {
        self.order.iter().filter_map(|id| self.by_id.get(id))
    }

    /// A hyperlink target for a relationship id: external URLs only.
    pub(crate) fn link(&self, id: &str) -> Option<String> {
        self.get(id)
            .filter(|r| r.external && !r.target.is_empty())
            .map(|r| r.target.clone())
    }
}
