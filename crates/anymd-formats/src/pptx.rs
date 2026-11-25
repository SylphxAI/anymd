//! PPTX → Markdown: one section per slide in presentation order, with titles,
//! bullet levels, tables, chart data, image alt text, and speaker notes.

use std::collections::{HashMap, HashSet};

use crate::images::{self, ImageStore};
use crate::ooxml::{self, Element, Package, Rels};
use crate::{ConvertError, Converted, Options, Section};

mod extras;
mod slide;
#[cfg(test)]
mod tests;

use slide::{offset, placeholder, render_slide};

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    convert_with_limits(bytes, options, crate::limits::Limits::default())
}

fn convert_with_limits(
    bytes: &[u8],
    options: &Options,
    limits: crate::limits::Limits,
) -> Result<Converted, ConvertError> {
    let mut package = Package::open(bytes, "PPTX")?;
    package.limits = limits;
    let main = package.main_part("ppt/presentation.xml")?;
    let presentation = package
        .xml(&main)?
        .ok_or_else(|| ooxml::invalid("PPTX has no ppt/presentation.xml part"))?;
    let rels = package.rels(&main)?;

    let mut slides: Vec<String> = presentation
        .path(&["sldIdLst"])
        .map(|list| {
            list.children_named("sldId")
                .filter_map(|id| id.rel_attr("id"))
                .filter_map(|id| rels.get(id))
                .map(|r| r.target.clone())
                .collect()
        })
        .unwrap_or_default();
    if slides.is_empty() {
        // No slide list: fall back to ppt/slides/slideN.xml in numeric order.
        let mut numbered: Vec<(u32, String)> = (1..=ooxml::MAX_ENTRIES as u32)
            .map(|n| (n, format!("ppt/slides/slide{n}.xml")))
            .take_while(|(_, name)| package.has(name))
            .collect();
        numbered.sort();
        slides = numbered.into_iter().map(|(_, name)| name).collect();
    }

    let (title, metadata) = ooxml::core_properties(&mut package);
    let store = options.images.as_ref();
    let repeated = match store {
        Some(_) => repeated_pictures(&mut package, &slides),
        None => HashSet::new(),
    };
    let mut sections = Vec::with_capacity(slides.len());
    let mut layouts: HashMap<String, Positions> = HashMap::new();
    for (index, path) in slides.iter().enumerate() {
        package.limits.check()?;
        let markdown = match package.xml(path)? {
            Some(slide) => {
                let images = SlideImages {
                    store,
                    repeated: &repeated,
                    number: index + 1,
                };
                render_slide(&mut package, path, &slide, &mut layouts, &images)?
            }
            None => String::new(),
        };
        package.limits.output(&markdown)?;
        sections.push(Section {
            label: format!("slide {}", index + 1),
            markdown,
        });
    }
    package.limits.check()?;
    Ok(Converted {
        outline: Vec::new(),
        format: "pptx".into(),
        title,
        sections,
        metadata,
    })
}

/// The picture export settings for one slide.
struct SlideImages<'a> {
    store: Option<&'a ImageStore>,
    repeated: &'a HashSet<u64>,
    number: usize,
}

/// Pictures that sit on three or more slides: logos and running ornaments.
fn repeated_pictures(package: &mut Package<'_>, slides: &[String]) -> HashSet<u64> {
    let mut keys: HashMap<String, Option<u64>> = HashMap::new();
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for path in slides {
        if package.limits.check().is_err() {
            break;
        }
        let Some(slide) = package.xml(path).ok().flatten() else {
            continue;
        };
        let Ok(rels) = package.rels(path) else {
            continue;
        };
        let mut blips = Vec::new();
        slide.find_all("blip", &mut blips);
        let mut on_slide = HashSet::new();
        for blip in blips {
            let Some(target) = blip
                .rel_attr("embed")
                .and_then(|id| rels.get(id))
                .filter(|rel| !rel.external)
                .map(|rel| rel.target.clone())
            else {
                continue;
            };
            let key = *keys.entry(target.clone()).or_insert_with(|| {
                let bytes = package.read(&target).ok().flatten()?;
                Some(images::content_key(&bytes))
            });
            if let Some(key) = key {
                on_slide.insert(key);
            }
        }
        for key in on_slide {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, slides)| *slides >= images::REPEAT_UNITS)
        .map(|(key, _)| key)
        .collect()
}

/// Placeholder positions inherited from the slide layout and master.
#[derive(Default, Clone)]
struct Positions {
    by_idx: HashMap<String, (i64, i64)>,
    by_type: HashMap<String, (i64, i64)>,
}

impl Positions {
    fn collect(&mut self, tree: &Element) {
        for shape in tree.elements() {
            if shape.is("grpSp") {
                self.collect(shape);
                continue;
            }
            let (Some(ph), Some(pos)) = (placeholder(shape), offset(shape)) else {
                continue;
            };
            if let Some(idx) = ph.attr("idx") {
                self.by_idx.entry(idx.to_string()).or_insert(pos);
            }
            self.by_type
                .entry(ph.attr("type").unwrap_or("body").to_string())
                .or_insert(pos);
        }
    }

    fn lookup(&self, ph: &Element) -> Option<(i64, i64)> {
        ph.attr("idx")
            .and_then(|idx| self.by_idx.get(idx))
            .or_else(|| self.by_type.get(ph.attr("type").unwrap_or("body")))
            .copied()
    }
}

fn layout_positions(
    package: &mut Package<'_>,
    slide_rels: &Rels,
) -> Result<Positions, ConvertError> {
    let mut positions = Positions::default();
    let Some(layout) = slide_rels
        .first_of_type("/slideLayout")
        .map(|r| r.target.clone())
    else {
        return Ok(positions);
    };
    if let Some(root) = package.xml(&layout).ok().flatten() {
        if let Some(tree) = root.path(&["cSld", "spTree"]) {
            positions.collect(tree);
        }
    }
    let layout_rels = package.rels(&layout)?;
    if let Some(master) = layout_rels
        .first_of_type("/slideMaster")
        .map(|r| r.target.clone())
    {
        if let Some(root) = package.xml(&master).ok().flatten() {
            if let Some(tree) = root.path(&["cSld", "spTree"]) {
                positions.collect(tree);
            }
        }
    }
    Ok(positions)
}
