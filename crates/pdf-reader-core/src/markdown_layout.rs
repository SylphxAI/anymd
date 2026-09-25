//! Clean Markdown from PDF glyph geometry.
//!
//! The pipeline is: glyphs (from `pdf-extract`) → rows by baseline → segments
//! split on large horizontal gaps, with inter-word spaces inferred from glyph
//! gaps → reading order by a column-aware XY cut → paragraphs, headings, list
//! items, and pipe tables → one Markdown string per page.
//!
//! Pages are extracted in parallel and each page is isolated: a page that
//! fails to parse becomes a marker comment instead of failing the document.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use pdf_extract::{
    output_doc_page, ColorSpace, Document, MediaBox, Object, OutputDev, OutputError,
    Path as PdfPath, Transform,
};

use crate::text_index::TextIndexError;

const MAX_GLYPHS_PER_PAGE: usize = 400_000;
const MAX_WORKERS: usize = 8;

/// A parsed PDF (lopdf document).
pub type PdfDocument = Document;

/// One page of converted Markdown.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownPage {
    /// 1-based page number.
    pub number: u32,
    pub markdown: String,
}

/// A converted PDF (or the requested subset of its pages).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownDocument {
    pub title: Option<String>,
    pub page_count: u32,
    pub pages: Vec<MarkdownPage>,
    /// Bookmarks as (depth, title, page), depth 0 = top level.
    pub outline: Vec<(usize, String, Option<u32>)>,
}

/// Open a PDF from disk (decrypting with the empty password when needed).
pub fn load_document(path: &Path) -> Result<Document, TextIndexError> {
    let mut doc = Document::load(path)
        .map_err(|err| TextIndexError::extraction_failed(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            TextIndexError::extraction_failed(format!(
                "PDF is encrypted and needs a password: {err}"
            ))
        })?;
    }
    Ok(doc)
}

/// Open a PDF from memory (decrypting with the empty password when needed).
pub fn load_document_bytes(bytes: &[u8]) -> Result<Document, TextIndexError> {
    let mut doc = Document::load_mem(bytes)
        .map_err(|err| TextIndexError::extraction_failed(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            TextIndexError::extraction_failed(format!(
                "PDF is encrypted and needs a password: {err}"
            ))
        })?;
    }
    Ok(doc)
}

/// Page count without extracting text.
pub fn page_count(doc: &Document) -> u32 {
    u32::try_from(doc.get_pages().len()).unwrap_or(u32::MAX)
}

/// Convert the selected pages (1-based; `None` = all) of a PDF to Markdown.
pub fn pdf_to_markdown(
    doc: &Document,
    pages: Option<&[u32]>,
) -> Result<MarkdownDocument, TextIndexError> {
    let page_map = doc.get_pages();
    let total = u32::try_from(page_map.len()).unwrap_or(u32::MAX);
    let selected: Vec<u32> = match pages {
        Some(list) => list
            .iter()
            .copied()
            .filter(|page| page_map.contains_key(page))
            .collect(),
        None => page_map.keys().copied().collect(),
    };
    let raw = extract_pages(doc, &selected);
    let body_size = body_font_size(&raw);
    let repeated = repeated_margin_lines(&raw);
    let mut heading_sizes = Vec::<f64>::new();
    let mut laid_out = Vec::with_capacity(raw.len());
    for page in &raw {
        let blocks = match &page.glyphs {
            Ok(glyphs) => layout_page(glyphs, page, body_size, &repeated),
            Err(message) => vec![Block::Comment(message.clone())],
        };
        for block in &blocks {
            if let Block::Paragraph { size, text, .. } = block {
                if is_size_heading(*size, body_size, text) {
                    heading_sizes.push(*size);
                }
            }
        }
        laid_out.push((page.number, blocks));
    }
    let levels = heading_levels(&heading_sizes);
    let mut first_heading = None;
    let pages = laid_out
        .into_iter()
        .map(|(number, blocks)| MarkdownPage {
            number,
            markdown: {
                let mut heading = None;
                let markdown = render_blocks(&blocks, body_size, &levels, &mut heading);
                if number == 1 {
                    first_heading = heading;
                }
                markdown
            },
        })
        .collect();
    let title = info_title(doc).or(first_heading);
    Ok(MarkdownDocument {
        title,
        page_count: total,
        pages,
        outline: outline(doc),
    })
}

// ---------------------------------------------------------------------------
// Glyph collection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Glyph {
    /// Start and end along the text direction.
    x0: f64,
    x1: f64,
    /// Baseline position across the text direction (larger = higher on page).
    base: f64,
    size: f64,
    text: String,
    space: bool,
}

struct RawPage {
    number: u32,
    bottom: f64,
    top: f64,
    glyphs: Result<Vec<Glyph>, String>,
    rotated: Vec<Glyph>,
}

#[derive(Default)]
struct Collector {
    media: Option<MediaBox>,
    glyphs: Vec<Glyph>,
    rotated: Vec<Glyph>,
}

impl OutputDev for Collector {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.media = Some(*media_box);
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        character: &str,
    ) -> Result<(), OutputError> {
        if self.glyphs.len() + self.rotated.len() >= MAX_GLYPHS_PER_PAGE {
            return Err(OutputError::IoError(std::io::Error::other(
                "page has too many glyphs",
            )));
        }
        let values = [
            trm.m11, trm.m12, trm.m21, trm.m22, trm.m31, trm.m32, width, font_size,
        ];
        if !values.iter().all(|value| value.is_finite()) {
            return Ok(());
        }
        let text = normalize_glyph_text(character);
        if text.is_empty() {
            return Ok(());
        }
        let size = (trm.m21.hypot(trm.m22) * font_size).abs();
        if size <= 0.1 || size > 2000.0 {
            return Ok(());
        }
        let scale = trm.m11.hypot(trm.m12);
        if scale <= 0.0 {
            return Ok(());
        }
        let (dx, dy) = (trm.m11 / scale, trm.m12 / scale);
        let space = text.chars().all(char::is_whitespace);
        // Character spacing (Tc) is part of the advance; word spacing is not.
        let tracking = if space { 0.0 } else { spacing };
        let mut advance = ((width * font_size + tracking) * scale).abs();
        if advance < size * 0.05 {
            // Fonts without widths would otherwise glue every glyph together.
            advance = size * 0.5;
        }
        let upright = dx > 0.9 && dy.abs() < 0.2;
        let (along, across) = if upright {
            (trm.m31, trm.m32)
        } else {
            // Project onto the rotated text direction.
            (trm.m31 * dx + trm.m32 * dy, -trm.m31 * dy + trm.m32 * dx)
        };
        let glyph = Glyph {
            x0: along,
            x1: along + advance,
            base: across,
            size,
            text: if space { " ".into() } else { text },
            space,
        };
        if upright {
            self.glyphs.push(glyph);
        } else {
            self.rotated.push(glyph);
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn stroke(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &PdfPath,
    ) -> Result<(), OutputError> {
        Ok(())
    }

    fn fill(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &PdfPath,
    ) -> Result<(), OutputError> {
        Ok(())
    }
}

fn normalize_glyph_text(character: &str) -> String {
    let mut out = String::with_capacity(character.len());
    for ch in character.chars() {
        match ch {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{3000}' | '\t' => out.push(' '),
            '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{FEFF}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn extract_pages(doc: &Document, selected: &[u32]) -> Vec<RawPage> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS)
        .min(selected.len().max(1));
    let extract_one = |number: u32| -> RawPage {
        let mut collector = Collector::default();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            output_doc_page(doc, &mut collector, number)
        }));
        let glyphs = match outcome {
            Ok(Ok(())) => Ok(std::mem::take(&mut collector.glyphs)),
            Ok(Err(err)) => Err(format!("page {number}: text extraction failed ({err})")),
            Err(_) => Err(format!(
                "page {number}: text extraction failed (malformed font or content)"
            )),
        };
        let (bottom, top) = collector
            .media
            .map(|media| (media.lly.min(media.ury), media.lly.max(media.ury)))
            .unwrap_or((0.0, 792.0));
        RawPage {
            number,
            bottom,
            top,
            glyphs,
            rotated: collector.rotated,
        }
    };
    if workers <= 1 {
        return selected.iter().map(|&number| extract_one(number)).collect();
    }
    let mut results: Vec<Option<RawPage>> = (0..selected.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let extract_one = &extract_one;
                scope.spawn(move || {
                    selected
                        .iter()
                        .enumerate()
                        .skip(worker)
                        .step_by(workers)
                        .map(|(index, &number)| (index, extract_one(number)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(pages) = handle.join() {
                for (index, page) in pages {
                    results[index] = Some(page);
                }
            }
        }
    });
    results
        .into_iter()
        .zip(selected)
        .map(|(page, &number)| {
            page.unwrap_or(RawPage {
                number,
                bottom: 0.0,
                top: 792.0,
                glyphs: Err(format!("page {number}: text extraction failed")),
                rotated: Vec::new(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rows and segments
// ---------------------------------------------------------------------------

/// A horizontal run of text on one baseline with no large gap inside it.
#[derive(Debug, Clone)]
struct Segment {
    x0: f64,
    x1: f64,
    base: f64,
    top: f64,
    bottom: f64,
    size: f64,
    text: String,
}

impl Segment {
    fn chars(&self) -> usize {
        self.text.chars().count()
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3000..=0x30FF | 0x3100..=0x31FF |
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF |
        0xFF00..=0xFFEF | 0x20000..=0x2FA1F)
}

/// Group glyphs into rows by baseline (tolerating super/subscripts).
///
/// Glyphs are first chained into runs in content-stream order, and a run only
/// joins a row when it does not overlap the row's runs horizontally, so two
/// nearby lines of different sizes never interleave letter by letter.
fn rows_of(glyphs: Vec<Glyph>) -> Vec<Vec<Glyph>> {
    struct Run {
        base: f64,
        size: f64,
        x0: f64,
        x1: f64,
        glyphs: Vec<Glyph>,
    }
    let mut runs: Vec<Run> = Vec::new();
    for glyph in glyphs {
        if let Some(run) = runs.last_mut() {
            let same_line = (run.base - glyph.base).abs() <= run.size.max(glyph.size) * 0.1;
            let continues =
                glyph.x0 >= run.x1 - run.size * 0.5 && glyph.x0 <= run.x1 + run.size * 1.5;
            if same_line && continues {
                run.x1 = run.x1.max(glyph.x1);
                if !glyph.space {
                    run.size = run.size.max(glyph.size);
                }
                run.glyphs.push(glyph);
                continue;
            }
        }
        runs.push(Run {
            base: glyph.base,
            size: glyph.size,
            x0: glyph.x0,
            x1: glyph.x1,
            glyphs: vec![glyph],
        });
    }
    runs.sort_by(|a, b| b.base.total_cmp(&a.base).then(a.x0.total_cmp(&b.x0)));
    // (reference baseline, reference size, spans, glyphs)
    let mut rows: Vec<(f64, f64, Vec<(f64, f64)>, Vec<Glyph>)> = Vec::new();
    for run in runs {
        let only_space = run.glyphs.iter().all(|g| g.space);
        if let Some((ref_base, ref_size, spans, row)) = rows.last_mut() {
            let big = ref_size.max(run.size);
            let small = ref_size.min(run.size);
            let tolerance = if small < big * 0.85 {
                0.6 * big
            } else {
                0.45 * big
            };
            let near = (*ref_base - run.base).abs() <= tolerance;
            let overlaps = spans.iter().any(|&(a, b)| {
                let overlap = b.min(run.x1) - a.max(run.x0);
                overlap > (b - a).min(run.x1 - run.x0).max(0.0) * 0.3 && overlap > small * 0.3
            });
            if near && (!overlaps || only_space) {
                if run.size > *ref_size * 1.05 && !only_space {
                    *ref_base = run.base;
                    *ref_size = run.size;
                }
                spans.push((run.x0, run.x1));
                row.extend(run.glyphs);
                continue;
            }
        }
        rows.push((run.base, run.size, vec![(run.x0, run.x1)], run.glyphs));
    }
    rows.into_iter().map(|(_, _, _, row)| row).collect()
}

/// Split a row into segments at large gaps, inferring word spaces.
fn segments_of_row(mut row: Vec<Glyph>) -> Vec<Segment> {
    row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let dominant = dominant_size(row.iter().filter(|g| !g.space).map(|g| (g.size, 1)));
    let mut bases: Vec<f64> = row
        .iter()
        .filter(|g| !g.space && g.size >= dominant * 0.95)
        .map(|g| g.base)
        .collect();
    let ref_base = median(&mut bases);

    // Pass 1: drop overprinted duplicates and split into groups at large gaps.
    // Each entry is (glyph, explicit space before it).
    let mut groups: Vec<Vec<(Glyph, bool)>> = Vec::new();
    let mut end = f64::NEG_INFINITY;
    let mut pending_space = false;
    for glyph in row {
        if glyph.space {
            pending_space = true;
            if end.is_finite() {
                end = end.max(glyph.x1.min(glyph.x0 + glyph.size * 0.6));
            }
            continue;
        }
        if let Some((last, _)) = groups.last().and_then(|group| group.last()) {
            if last.text == glyph.text && (glyph.x0 - last.x0).abs() < glyph.size * 0.12 {
                continue;
            }
        }
        let size_ref = glyph.size.max(dominant * 0.8);
        let gap = glyph.x0 - end;
        if groups.is_empty() || gap > size_ref * 1.2 {
            groups.push(Vec::new());
            pending_space = false;
        }
        end = if gap > size_ref * 1.2 {
            glyph.x1
        } else {
            end.max(glyph.x1)
        };
        groups
            .last_mut()
            .expect("group")
            .push((glyph, std::mem::take(&mut pending_space)));
    }

    // Pass 2: text per group, with a space threshold that adapts to tracking.
    let mut segments = Vec::with_capacity(groups.len());
    for group in groups {
        let mut gaps: Vec<f64> = Vec::with_capacity(group.len());
        let mut reach = f64::NEG_INFINITY;
        for (glyph, _) in &group {
            if reach.is_finite() {
                gaps.push((glyph.x0 - reach) / glyph.size.max(0.1));
            }
            reach = reach.max(glyph.x1);
        }
        let letter_gap = if gaps.len() >= 4 {
            let mut sorted = gaps.clone();
            sorted.sort_by(f64::total_cmp);
            sorted[sorted.len() / 4].max(0.0)
        } else {
            0.0
        };
        let base_threshold = if letter_gap > 0.12 {
            letter_gap + (letter_gap * 0.8).max(0.15)
        } else {
            0.16
        };
        let mut segment: Option<Segment> = None;
        let mut script = 0i8;
        let mut reach = f64::NEG_INFINITY;
        for (glyph, explicit_space) in group {
            let Some(current) = segment.as_mut() else {
                reach = glyph.x1;
                segment = Some(new_segment(&glyph, dominant));
                continue;
            };
            let gap = glyph.x0 - reach;
            let prev_char = current.text.chars().last();
            let next_char = glyph.text.chars().next();
            let cjk_pair = prev_char.is_some_and(is_cjk) && next_char.is_some_and(is_cjk);
            let scale = glyph
                .size
                .max(current.size * 0.7)
                .min(current.size.max(glyph.size));
            let wants_space = if cjk_pair {
                gap > 0.5 * scale
            } else {
                explicit_space && letter_gap <= 0.12 || gap > base_threshold * scale
            };
            if wants_space && !current.text.ends_with(' ') {
                current.text.push(' ');
            }
            let glyph_script = if dominant <= 0.0 || glyph.size > dominant * 0.85 {
                0
            } else if glyph.base - ref_base > dominant * 0.2 {
                1
            } else if glyph.base - ref_base < -dominant * 0.12 {
                -1
            } else {
                0
            };
            if glyph_script != script {
                let marks = next_char.is_some_and(char::is_alphanumeric);
                if glyph_script != 0 && marks && !wants_space && prev_char.is_some() {
                    current.text.push(if glyph_script > 0 { '^' } else { '_' });
                    script = glyph_script;
                } else if glyph_script == 0 || !marks {
                    script = 0;
                }
            }
            current.text.push_str(&glyph.text);
            current.x1 = current.x1.max(glyph.x1);
            current.top = current.top.max(glyph.base + glyph.size * 0.8);
            current.bottom = current.bottom.min(glyph.base - glyph.size * 0.2);
            reach = reach.max(glyph.x1);
        }
        if let Some(mut segment) = segment {
            let trimmed = segment.text.trim();
            if trimmed.len() != segment.text.len() {
                segment.text = trimmed.to_string();
            }
            if !segment.text.is_empty() {
                segments.push(segment);
            }
        }
    }
    segments
}

fn new_segment(glyph: &Glyph, dominant: f64) -> Segment {
    Segment {
        x0: glyph.x0,
        x1: glyph.x1,
        base: glyph.base,
        top: glyph.base + glyph.size * 0.8,
        bottom: glyph.base - glyph.size * 0.2,
        size: if dominant > 0.0 { dominant } else { glyph.size },
        text: glyph.text.clone(),
    }
}

fn dominant_size(sizes: impl Iterator<Item = (f64, usize)>) -> f64 {
    let mut buckets: HashMap<i64, (usize, f64)> = HashMap::new();
    for (size, weight) in sizes {
        let entry = buckets
            .entry((size * 2.0).round() as i64)
            .or_insert((0, size));
        entry.0 += weight;
    }
    buckets
        .into_values()
        .max_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(_, size)| size)
        .unwrap_or(0.0)
}

fn body_font_size(pages: &[RawPage]) -> f64 {
    let size = dominant_size(
        pages
            .iter()
            .filter_map(|page| page.glyphs.as_ref().ok())
            .flatten()
            .filter(|glyph| !glyph.space)
            .map(|glyph| (glyph.size, 1)),
    );
    if size > 0.0 {
        size
    } else {
        10.0
    }
}

// ---------------------------------------------------------------------------
// Running headers and footers
// ---------------------------------------------------------------------------

fn margin_key(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            if c.is_ascii_digit() {
                '#'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

fn in_margin(segment: &Segment, page: &RawPage) -> bool {
    let height = (page.top - page.bottom).max(1.0);
    segment.bottom > page.top - height * 0.08 || segment.top < page.bottom + height * 0.08
}

/// Lines in the top/bottom margin that repeat on at least half the pages.
fn repeated_margin_lines(pages: &[RawPage]) -> HashSet<String> {
    let usable = pages.iter().filter(|page| page.glyphs.is_ok()).count();
    if usable < 3 {
        return HashSet::new();
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for page in pages {
        let Ok(glyphs) = &page.glyphs else { continue };
        let mut seen = HashSet::new();
        for row in rows_of(glyphs.clone()) {
            for segment in segments_of_row(row) {
                if in_margin(&segment, page) {
                    let key = margin_key(&segment.text);
                    if !key.is_empty() && seen.insert(key.clone()) {
                        *counts.entry(key).or_default() += 1;
                    }
                }
            }
        }
    }
    let needed = usable.div_ceil(2).max(3);
    counts
        .into_iter()
        .filter(|(_, count)| *count >= needed)
        .map(|(key, _)| key)
        .collect()
}

fn is_page_number(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let lower = lower.trim_matches(|c: char| c == '-' || c == '–' || c == '—' || c.is_whitespace());
    let lower = lower.strip_prefix("page").map(str::trim).unwrap_or(lower);
    if lower.is_empty() || lower.len() > 16 {
        return false;
    }
    let mut parts = lower
        .split(|c: char| c == '/' || c.is_whitespace())
        .filter(|p| !p.is_empty() && *p != "of");
    let first = parts.next().unwrap_or("");
    let numeric = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    let roman = |p: &str| !p.is_empty() && p.len() <= 6 && p.chars().all(|c| "ivxlc".contains(c));
    (numeric(first) || roman(first)) && parts.all(numeric)
}

// ---------------------------------------------------------------------------
// Reading order
// ---------------------------------------------------------------------------

fn gaps(intervals: &mut [(f64, f64)], min_gap: f64) -> Vec<(f64, f64)> {
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for &(start, end) in intervals.iter() {
        if reach.is_finite() && start - reach >= min_gap {
            out.push((reach, start));
        }
        reach = reach.max(end);
    }
    out
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// A column gutter: a vertical strip with running text on both sides. A few
/// segments may cross it (figure labels, a spanning caption); they are
/// handled by the caller. Returns the gutter's (start, end).
fn column_cut(segments: &[Segment], body: f64) -> Option<(f64, f64)> {
    if segments.len() < 6 {
        return None;
    }
    let left_edge = segments.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min);
    let right_edge = segments.iter().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max);
    let width = right_edge - left_edge;
    if width <= body * 4.0 {
        return None;
    }
    // Sweep: x ranges covered by at most `allowed` segments.
    let allowed = segments.len() / 12;
    let mut events: Vec<(f64, i32)> = Vec::with_capacity(segments.len() * 2);
    for segment in segments {
        events.push((segment.x0, 1));
        events.push((segment.x1, -1));
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut candidates = Vec::new();
    let mut active = 0i32;
    let mut open: Option<f64> = None;
    for (x, delta) in events {
        let before = active;
        active += delta;
        if before as usize > allowed && active as usize <= allowed {
            open = Some(x);
        } else if before as usize <= allowed && active as usize > allowed {
            if let Some(start) = open.take() {
                if x - start >= body * 0.9 {
                    candidates.push((start, x));
                }
            }
        }
    }
    let mut best: Option<((f64, f64), f64)> = None;
    for (start, end) in candidates {
        let mid = (start + end) / 2.0;
        if mid < left_edge + width * 0.2 || mid > right_edge - width * 0.2 {
            continue;
        }
        let left: Vec<&Segment> = segments.iter().filter(|s| s.x1 <= start + 0.5).collect();
        let right: Vec<&Segment> = segments.iter().filter(|s| s.x0 >= end - 0.5).collect();
        // A side is a text column when its real lines (ignoring short figure
        // labels) are long and mostly fill the column width.
        let side_ok = |side: &[&Segment]| {
            let lines: Vec<&&Segment> = side.iter().filter(|s| s.chars() >= 10).collect();
            if lines.len() < 3 || lines.len() * 3 < side.len() {
                return false;
            }
            let lo = lines.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min);
            let hi = lines.iter().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max);
            let side_width = hi - lo;
            let mut chars: Vec<f64> = lines.iter().map(|s| s.chars() as f64).collect();
            let mut fill: Vec<f64> = lines
                .iter()
                .map(|s| (s.x1 - s.x0) / side_width.max(1.0))
                .collect();
            side_width >= width * 0.2 && median(&mut chars) >= 18.0 && median(&mut fill) >= 0.55
        };
        if side_ok(&left) && side_ok(&right) {
            let score = end - start;
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some(((start, end), score));
            }
        }
    }
    best.map(|(gutter, _)| gutter)
}

/// Order segments into reading-order regions (each region is top-to-bottom).
fn reading_regions(segments: Vec<Segment>, body: f64, depth: usize) -> Vec<Vec<Segment>> {
    if segments.len() <= 1 || depth > 8 {
        return vec![segments];
    }
    // Bands separated by clear vertical whitespace.
    let mut spans: Vec<(f64, f64)> = segments.iter().map(|s| (-s.top, -s.bottom)).collect();
    let cuts: Vec<f64> = gaps(&mut spans, body * 0.55)
        .into_iter()
        .map(|(a, b)| -(a + b) / 2.0)
        .collect();
    let mut bands: Vec<Vec<Segment>> = vec![Vec::new(); cuts.len() + 1];
    for segment in segments {
        let mid = (segment.top + segment.bottom) / 2.0;
        let index = cuts.iter().take_while(|cut| mid < **cut).count();
        bands[index].push(segment);
    }
    bands.retain(|band| !band.is_empty());
    // Merge consecutive bands that share the same column gutter.
    let mut groups: Vec<(Option<(f64, f64)>, Vec<Segment>)> = Vec::new();
    for band in bands {
        let cut = column_cut(&band, body);
        match (groups.last_mut(), cut) {
            (Some((Some(prev), group)), Some(gutter))
                if (((prev.0 + prev.1) - (gutter.0 + gutter.1)) / 2.0).abs() < body * 2.0 =>
            {
                group.extend(band);
            }
            _ => groups.push((cut, band)),
        }
    }
    // Consecutive bands without columns form one region, so tables and
    // paragraphs with generous row spacing stay together.
    let mut out = Vec::new();
    let mut plain: Vec<Segment> = Vec::new();
    for (cut, group) in groups {
        // Re-check the gutter on the merged group (a band alone may be too small).
        match column_cut(&group, body).or(cut) {
            Some(gutter) => {
                if !plain.is_empty() {
                    out.push(std::mem::take(&mut plain));
                }
                out.extend(split_columns(group, gutter, body, depth));
            }
            None => plain.extend(group),
        }
    }
    if !plain.is_empty() {
        out.push(plain);
    }
    out
}

/// Split a group at a gutter. Wide segments that cross the gutter (a caption
/// or table spanning both columns) divide the columns into vertical zones;
/// narrow ones (figure labels) join the side of their midpoint.
fn split_columns(group: Vec<Segment>, gutter: (f64, f64), body: f64, depth: usize) -> Vec<Vec<Segment>> {
    let x = (gutter.0 + gutter.1) / 2.0;
    let lo = group.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min);
    let hi = group.iter().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max);
    let (mut barriers, rest): (Vec<Segment>, Vec<Segment>) = group.into_iter().partition(|s| {
        s.x0 < gutter.0 - 0.5 && s.x1 > gutter.1 + 0.5 && s.x1 - s.x0 >= (hi - lo) * 0.5
    });
    barriers.sort_by(|a, b| b.top.total_cmp(&a.top));
    // Merge vertically overlapping barriers into bands.
    let mut barrier_bands: Vec<Vec<Segment>> = Vec::new();
    for barrier in barriers {
        match barrier_bands.last_mut() {
            Some(band)
                if band.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min)
                    <= barrier.top + body * 0.3 =>
            {
                band.push(barrier)
            }
            _ => barrier_bands.push(vec![barrier]),
        }
    }
    let bottoms: Vec<f64> = barrier_bands
        .iter()
        .map(|band| band.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min))
        .collect();
    let mut zones: Vec<(Vec<Segment>, Vec<Segment>)> =
        (0..=barrier_bands.len()).map(|_| (Vec::new(), Vec::new())).collect();
    for segment in rest {
        let mid = (segment.top + segment.bottom) / 2.0;
        let zone = bottoms.iter().take_while(|bottom| **bottom > mid).count();
        if (segment.x0 + segment.x1) / 2.0 < x {
            zones[zone].0.push(segment);
        } else {
            zones[zone].1.push(segment);
        }
    }
    let mut out = Vec::new();
    let mut barrier_bands = barrier_bands.into_iter();
    for (left, right) in zones {
        if !left.is_empty() {
            out.extend(reading_regions(left, body, depth + 1));
        }
        if !right.is_empty() {
            out.extend(reading_regions(right, body, depth + 1));
        }
        if let Some(band) = barrier_bands.next() {
            out.push(band);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Block {
    Paragraph {
        text: String,
        size: f64,
        lines: usize,
    },
    ListItem(String),
    Table(Vec<Vec<String>>),
    Comment(String),
}

fn group_rows(mut segments: Vec<Segment>) -> Vec<Vec<Segment>> {
    segments.sort_by(|a, b| b.base.total_cmp(&a.base).then(a.x0.total_cmp(&b.x0)));
    let mut rows: Vec<Vec<Segment>> = Vec::new();
    for segment in segments {
        if let Some(row) = rows.last_mut() {
            let anchor = &row[0];
            if (anchor.base - segment.base).abs() <= 0.45 * anchor.size.max(segment.size) {
                row.push(segment);
                continue;
            }
        }
        rows.push(vec![segment]);
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    rows
}

fn is_equation_number(text: &str) -> bool {
    let t = text.trim();
    t.len() <= 8
        && t.starts_with('(')
        && t.ends_with(')')
        && t[1..t.len() - 1]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
}

fn bullet_body(text: &str) -> Option<&str> {
    for bullet in [
        "•", "◦", "▪", "‣", "●", "○", "■", "□", "–", "—", "-", "*", "·", "\u{F0B7}", "➢", "✓",
    ] {
        if let Some(rest) = text.strip_prefix(bullet) {
            if rest.starts_with(' ')
                || (bullet != "-"
                    && bullet != "*"
                    && bullet != "–"
                    && bullet != "—"
                    && !rest.is_empty())
            {
                let body = rest.trim_start();
                if !body.is_empty() {
                    return Some(body);
                }
            }
        }
    }
    None
}

fn starts_enumerated(text: &str) -> bool {
    let mut chars = text.chars();
    let first = chars.next();
    match first {
        Some('(') => {
            let inner: String = chars.by_ref().take_while(|c| *c != ')').collect();
            !inner.is_empty()
                && inner.len() <= 4
                && inner.chars().all(|c| c.is_ascii_alphanumeric())
        }
        Some(c) if c.is_ascii_digit() || c.is_ascii_lowercase() => {
            let head: String = text
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let rest = &text[head.len()..];
            head.len() <= 3
                && (head.chars().all(|c| c.is_ascii_digit()) || head.len() == 1)
                && (rest.starts_with(". ") || rest.starts_with(") "))
        }
        _ => false,
    }
}

/// Section headings like "3.2 Attention" or "4 Why Self-Attention".
fn numbered_heading_level(text: &str) -> Option<usize> {
    let text = text.trim();
    let (number, rest) = text.split_once(' ')?;
    let number = number.trim_end_matches('.');
    if number.is_empty() || number.len() > 8 {
        return None;
    }
    let parts: Vec<&str> = number.split('.').collect();
    let numeric = parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let lettered =
        parts.len() == 1 && parts[0].len() == 1 && parts[0].chars().all(|c| c.is_ascii_uppercase());
    if !(numeric || lettered) || parts.len() > 4 {
        return None;
    }
    if numeric && parts[0].parse::<u32>().ok()? > 30 {
        return None;
    }
    let rest = rest.trim();
    let words = rest.split_whitespace().count();
    let first = rest.chars().next()?;
    if !first.is_uppercase() || words == 0 || words > 12 || rest.len() > 90 {
        return None;
    }
    if rest.ends_with('.') || rest.ends_with(',') || rest.ends_with(':') && words > 6 {
        return None;
    }
    // Headings are mostly letters, not numbers or math.
    let letters = rest.chars().filter(|c| c.is_alphabetic()).count();
    if letters * 10 < rest.chars().filter(|c| !c.is_whitespace()).count() * 7 {
        return None;
    }
    Some((parts.len() + 1).min(6))
}

fn named_heading(text: &str) -> bool {
    const NAMES: &[&str] = &[
        "abstract",
        "introduction",
        "background",
        "related work",
        "method",
        "methods",
        "methodology",
        "results",
        "discussion",
        "conclusion",
        "conclusions",
        "references",
        "bibliography",
        "acknowledgments",
        "acknowledgements",
        "appendix",
        "summary",
        "contents",
        "table of contents",
        "preface",
        "foreword",
        "index",
        "glossary",
    ];
    let lower = text.trim().trim_end_matches(':').to_lowercase();
    NAMES.contains(&lower.as_str())
}

fn join_line(paragraph: &mut String, line: &str) {
    if paragraph.is_empty() {
        paragraph.push_str(line);
        return;
    }
    let prev_last = paragraph.chars().last();
    let next_first = line.chars().next();
    if paragraph.ends_with('-') {
        let mut chars = paragraph.chars().rev();
        chars.next();
        let before = chars.next();
        let next_word = line.split_whitespace().next().unwrap_or("");
        let prev_word = paragraph.split_whitespace().next_back().unwrap_or("");
        let compound = prev_word[..prev_word.len() - 1].contains('-') || next_word.contains('-');
        if before.is_some_and(char::is_alphabetic) && next_first.is_some_and(char::is_lowercase) {
            if !compound {
                // A word broken across lines: "transduc-" + "tion".
                paragraph.pop();
            }
            // Compounds keep the hyphen: "left-to-" + "right".
            paragraph.push_str(line);
            return;
        }
    }
    if !(prev_last.is_some_and(is_cjk) && next_first.is_some_and(is_cjk)) {
        paragraph.push(' ');
    }
    paragraph.push_str(line);
}

fn row_text(row: &[Segment]) -> String {
    let mut text = String::new();
    for segment in row {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&segment.text);
    }
    text
}

/// Build a pipe table from rows that each contain several aligned segments.
fn build_table(rows: &[Vec<Segment>]) -> Option<Vec<Vec<String>>> {
    let max_cells = rows.iter().map(Vec::len).max()?;
    if max_cells < 2 {
        return None;
    }
    // Column slots come from the rows with the most cells.
    let mut spans: Vec<(f64, f64)> = rows
        .iter()
        .filter(|row| row.len() == max_cells)
        .flat_map(|row| row.iter().map(|s| (s.x0, s.x1)))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut columns: Vec<(f64, f64)> = Vec::new();
    for (start, end) in spans {
        match columns.last_mut() {
            Some(column) if start <= column.1 => column.1 = column.1.max(end),
            _ => columns.push((start, end)),
        }
    }
    if columns.len() < 2 {
        return None;
    }
    let column_of = |segment: &Segment| -> usize {
        let mid = (segment.x0 + segment.x1) / 2.0;
        columns
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = if mid < a.0 {
                    a.0 - mid
                } else if mid > a.1 {
                    mid - a.1
                } else {
                    0.0
                };
                let db = if mid < b.0 {
                    b.0 - mid
                } else if mid > b.1 {
                    mid - b.1
                } else {
                    0.0
                };
                da.total_cmp(&db)
            })
            .map(|(index, _)| index)
            .unwrap_or(0)
    };
    let mut table = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells = vec![String::new(); columns.len()];
        for segment in row {
            let cell = &mut cells[column_of(segment)];
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(&segment.text);
        }
        table.push(cells);
    }
    Some(table)
}

fn layout_page(
    glyphs: &[Glyph],
    page: &RawPage,
    body: f64,
    repeated: &HashSet<String>,
) -> Vec<Block> {
    let mut segments = Vec::new();
    for row in rows_of(glyphs.to_vec()) {
        segments.extend(segments_of_row(row));
    }
    segments.retain(|segment| {
        !(in_margin(segment, page)
            && (repeated.contains(&margin_key(&segment.text)) || is_page_number(&segment.text)))
    });
    let mut blocks = Vec::new();
    for region in reading_regions(segments, body, 0) {
        region_blocks(region, body, &mut blocks);
    }
    if !page.rotated.is_empty() {
        let mut lines = Vec::new();
        for row in rows_of(page.rotated.clone()) {
            let text = row_text(&segments_of_row(row));
            if !text.is_empty() {
                lines.push(text);
            }
        }
        if !lines.is_empty() {
            blocks.push(Block::Paragraph {
                text: lines.join(" "),
                size: 0.0,
                lines: 1,
            });
        }
    }
    blocks
}

fn region_blocks(region: Vec<Segment>, body: f64, blocks: &mut Vec<Block>) {
    let rows = group_rows(region);
    if rows.is_empty() {
        return;
    }
    let left = rows.iter().map(|r| r[0].x0).fold(f64::INFINITY, f64::min);
    let right = rows
        .iter()
        .map(|r| r.last().map_or(f64::NEG_INFINITY, |s| s.x1))
        .fold(f64::NEG_INFINITY, f64::max);
    let is_multi = |row: &Vec<Segment>| {
        row.len() >= 2 && !(row.len() == 2 && is_equation_number(&row[1].text))
    };

    let mut index = 0;
    let mut paragraph = String::new();
    let mut paragraph_size = 0.0;
    let mut paragraph_lines = 0usize;
    let mut in_list = false;
    let mut prev: Option<(f64, f64, f64)> = None; // (bottom, x1, size) of previous line
    let mut continuation_x0 = f64::NAN;
    let mut para_right = f64::NEG_INFINITY;
    let mut prev_x0 = f64::NAN;

    let flush =
        |blocks: &mut Vec<Block>, paragraph: &mut String, size: f64, lines: usize, list: bool| {
            if paragraph.is_empty() {
                return;
            }
            let text = std::mem::take(paragraph);
            blocks.push(if list {
                Block::ListItem(text)
            } else {
                Block::Paragraph { text, size, lines }
            });
        };

    while index < rows.len() {
        // Tables: runs of rows with several aligned cells.
        if is_multi(&rows[index]) {
            let mut end = index;
            let mut multi = 0;
            // Column anchors (cell starts and ends) seen so far in this run.
            let mut anchors: Vec<f64> = Vec::new();
            let aligned = |row: &Vec<Segment>, anchors: &[f64]| {
                if anchors.is_empty() {
                    return true;
                }
                let tolerance = row[0].size * 0.8;
                let hits = row
                    .iter()
                    .filter(|s| {
                        anchors
                            .iter()
                            .any(|a| (a - s.x0).abs() <= tolerance || (a - s.x1).abs() <= tolerance)
                    })
                    .count();
                hits * 2 >= row.len()
            };
            while end < rows.len() {
                if is_multi(&rows[end]) {
                    if multi >= 2 && !aligned(&rows[end], &anchors) {
                        // A different grid starts here (e.g. a second author block).
                        break;
                    }
                    anchors.extend(rows[end].iter().flat_map(|s| [s.x0, s.x1]));
                    multi += 1;
                    end += 1;
                } else if end + 1 < rows.len()
                    && is_multi(&rows[end + 1])
                    && multi > 0
                    && rows[end][0].chars() <= 40
                {
                    end += 1;
                } else {
                    break;
                }
            }
            if multi >= 2 {
                if let Some(table) = build_table(&rows[index..end]) {
                    flush(
                        blocks,
                        &mut paragraph,
                        paragraph_size,
                        paragraph_lines,
                        in_list,
                    );
                    in_list = false;
                    paragraph_lines = 0;
                    blocks.push(Block::Table(table));
                    let last = &rows[end - 1];
                    prev = Some((
                        last.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min),
                        last.last().map_or(0.0, |s| s.x1),
                        last[0].size,
                    ));
                    index = end;
                    continue;
                }
            }
        }
        let row = &rows[index];
        let text = row_text(row);
        let size = row.iter().map(|s| s.size).fold(0.0, f64::max);
        let top = row.iter().map(|s| s.top).fold(f64::NEG_INFINITY, f64::max);
        let bottom = row.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min);
        let x0 = row[0].x0;
        let x1 = row.last().map_or(x0, |s| s.x1);
        let bullet = bullet_body(&text);
        let enumerated = starts_enumerated(&text);
        let heading_like = numbered_heading_level(&text).is_some() || named_heading(&text);

        let new_block = match prev {
            None => true,
            Some((prev_bottom, prev_x1, prev_size)) => {
                let gap = prev_bottom - top;
                let size_change = (size - prev_size).abs() > prev_size.max(size) * 0.12;
                let heading_continues =
                    size >= body * 1.15 && !size_change && gap <= size * 0.6 && paragraph_lines < 3;
                // Short against its own paragraph (or the next line), or a short
                // stand-alone line in a wide region (key: value rows).
                let prev_width = prev_x1 - prev_x0;
                let prev_short = !heading_continues
                    && (prev_x1 < para_right.max(x1) - prev_size * 2.5
                        || (prev_x1 < right - prev_size * 2.5 && prev_width < (right - left) * 0.6));
                let indented = x0 > continuation_x0 + size * 0.8 && !in_list;
                let outdented = paragraph_lines >= 2 && x0 < continuation_x0 - size * 0.8;
                // Centered lines (title blocks, letterheads) stay separate.
                let region_width = right - left;
                let centered = !heading_continues
                    && prev_width < region_width * 0.8
                    && x1 - x0 < region_width * 0.8
                    && ((prev_x0 + prev_x1) / 2.0 - (x0 + x1) / 2.0).abs() < size * 0.6
                    && (prev_x0 - x0).abs() > size * 0.8
                    && x0.min(prev_x0) > left + size;
                gap > size.max(prev_size) * 0.55
                    || centered
                    || outdented
                    || size_change
                    || prev_short
                    || indented
                    || bullet.is_some()
                    || enumerated
                    || heading_like
            }
        };
        if new_block {
            flush(
                blocks,
                &mut paragraph,
                paragraph_size,
                paragraph_lines,
                in_list,
            );
            paragraph_lines = 0;
            paragraph_size = size;
            in_list = bullet.is_some();
            // A list item's continuation lines are indented under the bullet.
        } else if in_list && x0 <= left + size * 0.3 && bullet.is_none() {
            // Back at the margin: the list item ended.
            flush(
                blocks,
                &mut paragraph,
                paragraph_size,
                paragraph_lines,
                true,
            );
            paragraph_lines = 0;
            paragraph_size = size;
            in_list = false;
        }
        if paragraph_lines == 0 {
            para_right = f64::NEG_INFINITY;
        }
        join_line(&mut paragraph, bullet.unwrap_or(&text));
        paragraph_lines += 1;
        para_right = para_right.max(x1);
        prev_x0 = x0;
        if paragraph_lines == 2 {
            continuation_x0 = x0;
        } else if paragraph_lines == 1 {
            continuation_x0 = f64::NAN;
        }
        prev = Some((bottom, x1, size));
        // A heading line stands alone.
        if heading_like && !in_list {
            flush(
                blocks,
                &mut paragraph,
                paragraph_size,
                paragraph_lines,
                false,
            );
            paragraph_lines = 0;
            prev = Some((bottom, f64::NEG_INFINITY, size));
        }
        index += 1;
    }
    flush(
        blocks,
        &mut paragraph,
        paragraph_size,
        paragraph_lines,
        in_list,
    );
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn is_size_heading(size: f64, body: f64, text: &str) -> bool {
    size >= body * 1.15
        && text.chars().count() <= 120
        && text.chars().next().is_some_and(|c| !c.is_lowercase())
        && text.chars().any(char::is_alphabetic)
        && !text.ends_with(',')
}

/// Map distinct heading font sizes (largest first) to Markdown levels.
fn heading_levels(sizes: &[f64]) -> Vec<f64> {
    let mut distinct: Vec<f64> = Vec::new();
    let mut sorted = sizes.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    for size in sorted {
        if distinct.last().is_none_or(|last| (last - size).abs() > 0.6) {
            distinct.push(size);
        }
    }
    distinct
}

fn level_for(size: f64, levels: &[f64]) -> usize {
    levels
        .iter()
        .position(|level| (level - size).abs() <= 0.6)
        .map_or(3, |index| (index + 1).min(4))
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn render_blocks(
    blocks: &[Block],
    body: f64,
    levels: &[f64],
    first_heading: &mut Option<String>,
) -> String {
    let mut out = String::new();
    for block in blocks {
        let piece = match block {
            Block::Paragraph { text, size, lines } => {
                let numbered = if *lines == 1 {
                    numbered_heading_level(text).or_else(|| named_heading(text).then_some(2))
                } else {
                    None
                };
                let level = numbered.or_else(|| {
                    (is_size_heading(*size, body, text) && *lines <= 3)
                        .then(|| level_for(*size, levels))
                });
                match level {
                    Some(level) => {
                        if first_heading.is_none() && level == 1 {
                            *first_heading = Some(text.clone());
                        }
                        format!("{} {}", "#".repeat(level), text)
                    }
                    None => text.clone(),
                }
            }
            Block::ListItem(text) => format!("- {text}"),
            Block::Table(rows) => {
                let mut table = String::new();
                for (index, row) in rows.iter().enumerate() {
                    // Compact pipe tables: padding spaces cost ~20% more tokens.
                    table.push('|');
                    for cell in row {
                        table.push_str(&escape_cell(cell));
                        table.push('|');
                    }
                    table.push('\n');
                    if index == 0 {
                        table.push('|');
                        for _ in row {
                            table.push_str("-|");
                        }
                        table.push('\n');
                    }
                }
                table.trim_end().to_string()
            }
            Block::Comment(message) => format!("<!-- {message} -->"),
        };
        if piece.is_empty() {
            continue;
        }
        if !out.is_empty() {
            // Consecutive list items stay in one list.
            let list_continues = matches!(block, Block::ListItem(_)) && out.ends_with_list_item();
            out.push_str(if list_continues { "\n" } else { "\n\n" });
        }
        out.push_str(&piece);
    }
    out
}

trait EndsWithListItem {
    fn ends_with_list_item(&self) -> bool;
}

impl EndsWithListItem for String {
    fn ends_with_list_item(&self) -> bool {
        self.rsplit("\n\n")
            .next()
            .and_then(|last| last.lines().last())
            .is_some_and(|line| line.starts_with("- "))
    }
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

fn decode_pdf_string(object: &Object) -> Option<String> {
    let bytes = match object {
        Object::String(bytes, _) => bytes,
        _ => return None,
    };
    let text = if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

pub fn info_title(doc: &Document) -> Option<String> {
    let info = resolve(doc, doc.trailer.get(b"Info").ok()?)?
        .as_dict()
        .ok()?;
    let title = decode_pdf_string(resolve(doc, info.get(b"Title").ok()?)?)?;
    let lower = title.to_ascii_lowercase();
    let junk = lower.starts_with("untitled")
        || lower.starts_with("microsoft word")
        || [
            ".doc", ".docx", ".dvi", ".tex", ".pdf", ".indd", ".qxd", ".ps",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext));
    (!junk && title.chars().count() <= 300).then_some(title)
}

/// Bookmarks with resolved page numbers (bounded walk, cycle-safe).
pub fn outline(doc: &Document) -> Vec<(usize, String, Option<u32>)> {
    let page_of: BTreeMap<(u32, u16), u32> = doc
        .get_pages()
        .into_iter()
        .map(|(number, id)| (id, number))
        .collect();
    let Some(root) = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"Outlines").ok())
        .and_then(|object| resolve(doc, object))
        .and_then(|object| object.as_dict().ok())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    let mut stack: Vec<(usize, Option<&Object>)> = vec![(0, root.get(b"First").ok())];
    while let Some((depth, next)) = stack.pop() {
        let Some(Object::Reference(id)) = next else {
            continue;
        };
        if out.len() >= 2000 || depth > 16 || !visited.insert(*id) {
            continue;
        }
        let Ok(item) = doc.get_object(*id).and_then(Object::as_dict) else {
            continue;
        };
        stack.push((depth, item.get(b"Next").ok()));
        stack.push((depth + 1, item.get(b"First").ok()));
        let Some(title) = item
            .get(b"Title")
            .ok()
            .and_then(|object| resolve(doc, object))
            .and_then(decode_pdf_string)
        else {
            continue;
        };
        let dest = item
            .get(b"Dest")
            .ok()
            .or_else(|| {
                item.get(b"A")
                    .ok()
                    .and_then(|action| resolve(doc, action))
                    .and_then(|action| action.as_dict().ok())
                    .and_then(|action| action.get(b"D").ok())
            })
            .and_then(|dest| resolve(doc, dest));
        let page = match dest {
            Some(Object::Array(parts)) => match parts.first() {
                Some(Object::Reference(page_id)) => page_of.get(page_id).copied(),
                _ => None,
            },
            _ => None,
        };
        out.push((depth, title, page));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(line: &str, x: f64, base: f64, size: f64, advance: f64, word_gap: f64) -> Vec<Glyph> {
        let mut out = Vec::new();
        let mut cursor = x;
        for ch in line.chars() {
            if ch == ' ' {
                cursor += word_gap;
                continue;
            }
            out.push(Glyph {
                x0: cursor,
                x1: cursor + advance,
                base,
                size,
                text: ch.to_string(),
                space: false,
            });
            cursor += advance;
        }
        out
    }

    #[test]
    fn infers_word_spaces_from_gaps_without_space_glyphs() {
        let row = glyphs("The dominant sequence", 72.0, 700.0, 10.0, 5.0, 3.3);
        let segments = segments_of_row(row);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "The dominant sequence");
    }

    #[test]
    fn tight_kerning_does_not_split_words() {
        let row = glyphs("Transformer", 72.0, 700.0, 10.0, 5.0, 0.0)
            .into_iter()
            .enumerate()
            .map(|(i, mut g)| {
                let shift = if i % 2 == 0 { -0.4 } else { 0.6 };
                g.x0 += shift;
                g.x1 += shift;
                g
            })
            .collect();
        assert_eq!(segments_of_row(row)[0].text, "Transformer");
    }

    #[test]
    fn cjk_glyphs_do_not_get_spaces() {
        let row = glyphs("注意力机制", 72.0, 700.0, 10.0, 10.0, 0.0)
            .into_iter()
            .enumerate()
            .map(|(i, mut g)| {
                g.x0 += i as f64 * 0.8;
                g.x1 += i as f64 * 0.8;
                g
            })
            .collect();
        assert_eq!(segments_of_row(row)[0].text, "注意力机制");
    }

    #[test]
    fn large_gaps_split_segments() {
        let mut row = glyphs("Model", 72.0, 700.0, 10.0, 5.0, 3.0);
        row.extend(glyphs("BLEU", 200.0, 700.0, 10.0, 5.0, 3.0));
        let segments = segments_of_row(row);
        assert_eq!(
            segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["Model", "BLEU"]
        );
    }

    #[test]
    fn superscripts_stay_on_their_row() {
        let mut glyph_list = glyphs("word", 72.0, 700.0, 10.0, 5.0, 3.0);
        glyph_list.extend(glyphs("2", 92.5, 703.5, 7.0, 3.5, 0.0));
        let rows = rows_of(glyph_list);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn dehyphenates_line_breaks() {
        let mut text = String::from("trans-");
        join_line(&mut text, "duction models");
        assert_eq!(text, "transduction models");
        let mut keep = String::from("state-of-the-");
        join_line(&mut keep, "Art");
        assert_eq!(keep, "state-of-the- Art");
        let mut compound = String::from("a left-to-");
        join_line(&mut compound, "right model");
        assert_eq!(compound, "a left-to-right model");
    }

    #[test]
    fn detects_numbered_headings() {
        assert_eq!(numbered_heading_level("3.2 Attention"), Some(3));
        assert_eq!(numbered_heading_level("1 Introduction"), Some(2));
        assert_eq!(
            numbered_heading_level("3.2.1 Scaled Dot-Product Attention"),
            Some(4)
        );
        assert_eq!(
            numbered_heading_level("2 GPUs were used for training."),
            None
        );
        assert_eq!(numbered_heading_level("100 Epochs"), None);
    }

    #[test]
    fn recognizes_page_numbers() {
        assert!(is_page_number("12"));
        assert!(is_page_number("Page 3 of 10"));
        assert!(is_page_number("- 4 -"));
        assert!(is_page_number("xii"));
        assert!(!is_page_number("Results"));
    }

    fn segment(text: &str, x0: f64, x1: f64, base: f64) -> Segment {
        Segment {
            x0,
            x1,
            base,
            top: base + 8.0,
            bottom: base - 2.0,
            size: 10.0,
            text: text.into(),
        }
    }

    #[test]
    fn two_columns_read_left_then_right() {
        let long = "a line of running text that fills the column width";
        let mut segments = vec![segment("Title Of The Paper", 150.0, 450.0, 760.0)];
        for i in 0..10 {
            let base = 700.0 - i as f64 * 12.0;
            segments.push(segment(&format!("L{i} {long}"), 72.0, 290.0, base));
            segments.push(segment(&format!("R{i} {long}"), 320.0, 540.0, base));
        }
        let regions = reading_regions(segments, 10.0, 0);
        let order: Vec<String> = regions
            .iter()
            .flat_map(|region| group_rows(region.clone()))
            .map(|row| row_text(&row).chars().take(3).collect())
            .collect();
        assert_eq!(order[0], "Tit");
        assert_eq!(order[1], "L0 ");
        assert_eq!(order[10], "L9 ");
        assert_eq!(order[11], "R0 ");
    }

    #[test]
    fn aligned_cells_become_a_table() {
        let rows = vec![
            vec![
                segment("Model", 72.0, 110.0, 700.0),
                segment("BLEU", 200.0, 230.0, 700.0),
                segment("Cost", 300.0, 330.0, 700.0),
            ],
            vec![
                segment("ByteNet", 72.0, 120.0, 688.0),
                segment("23.75", 200.0, 228.0, 688.0),
                segment("1.0", 300.0, 318.0, 688.0),
            ],
            vec![
                segment("GNMT", 72.0, 105.0, 676.0),
                segment("24.6", 202.0, 226.0, 676.0),
                segment("2.3", 300.0, 318.0, 676.0),
            ],
        ];
        let mut blocks = Vec::new();
        region_blocks(rows.into_iter().flatten().collect(), 10.0, &mut blocks);
        match &blocks[..] {
            [Block::Table(table)] => {
                assert_eq!(table[0], ["Model", "BLEU", "Cost"]);
                assert_eq!(table[2], ["GNMT", "24.6", "2.3"]);
            }
            other => panic!("expected one table, got {other:?}"),
        }
    }
}
