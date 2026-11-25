use std::collections::{HashMap, HashSet};

use crate::images::{self, Embed, ImageStore};

use super::super::package::{Package, Rels};
use super::collapse_ws;

/// Markdown for an image with alt text; images without alt text are dropped.
pub(crate) fn image_markdown(alt: &str, target: Option<&str>) -> Option<String> {
    let alt = collapse_ws(alt);
    if alt.is_empty() {
        return None;
    }
    let name = target
        .and_then(|t| t.rsplit('/').next())
        .filter(|n| !n.is_empty())
        .unwrap_or("image");
    Some(format!(
        "![{}]({})",
        alt.replace('[', "\\[").replace(']', "\\]"),
        // Characters a URI may not hold, encoded; `{`, `}`, `<` and `>` would
        // otherwise form CriticMarkup delimiters.
        name.replace(' ', "%20")
            .replace('{', "%7B")
            .replace('}', "%7D")
            .replace('<', "%3C")
            .replace('>', "%3E")
    ))
}

/// Most image parts read from one part's relationships.
const MAX_MEDIA_PARTS: usize = 512;
/// Most image bytes read from one part's relationships.
const MAX_MEDIA_BYTES: usize = 256 * 1024 * 1024;

/// The image parts a part's relationships list, read up front so rendering
/// needs no package access. Empty (alt text only) without an image store.
#[derive(Default)]
pub(crate) struct Media {
    store: Option<ImageStore>,
    files: HashMap<String, Vec<u8>>,
    repeated: HashSet<u64>,
}

impl Media {
    pub(crate) fn load(
        package: &mut Package<'_>,
        rels: &Rels,
        store: Option<&ImageStore>,
        repeated: &HashSet<u64>,
    ) -> Self {
        let Some(store) = store else {
            return Self::default();
        };
        let mut files = HashMap::new();
        let mut total = 0usize;
        for rel in rels.ordered() {
            if rel.external || !rel.kind.ends_with("/image") || files.contains_key(&rel.target) {
                continue;
            }
            if files.len() >= MAX_MEDIA_PARTS {
                break;
            }
            if let Ok(Some(bytes)) = package.read(&rel.target) {
                total += bytes.len();
                if total > MAX_MEDIA_BYTES {
                    break;
                }
                files.insert(rel.target.clone(), bytes);
            }
        }
        Self {
            store: Some(store.clone()),
            files,
            repeated: repeated.clone(),
        }
    }

    /// Markdown for a picture: the exported file when there is a store and the
    /// picture is a raster that matters, nothing for decoration, else the
    /// alt-text form.
    pub(crate) fn markdown(
        &self,
        alt: &str,
        target: Option<&str>,
        place: Option<&str>,
    ) -> Option<String> {
        let found = self
            .store
            .as_ref()
            .zip(target)
            .and_then(|(store, target)| Some((store, self.files.get(target)?)));
        match found {
            Some((store, bytes)) => match images::embed(store, bytes, alt, place, &self.repeated) {
                Embed::Markdown(markdown) => Some(markdown),
                Embed::Skip => None,
                Embed::Fallback => image_markdown(alt, target),
            },
            None => image_markdown(alt, target),
        }
    }
}
