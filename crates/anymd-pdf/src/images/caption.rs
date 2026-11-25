//! Match figure captions using text and page geometry.

use crate::extract::Glyph;
use crate::rows::{row_text, rows_of, segments_of_row};

use super::is_caption_start;

/// A caption sits within this many points of its image.
const CAPTION_REACH: f64 = 36.0;
const CAPTION_LINES: usize = 4;
const CAPTION_CHARS: usize = 200;

struct Line {
    x0: f64,
    x1: f64,
    top: f64,
    bottom: f64,
    size: f64,
    text: String,
}

/// The caption of the image at `bbox`: the nearest line directly below or
/// above it that starts like a caption, with the lines that continue it.
pub(super) fn caption_near(glyphs: &[Glyph], bbox: [f64; 4]) -> Option<String> {
    let mut lines: Vec<Line> = Vec::new();
    for row in rows_of(glyphs.to_vec()) {
        let segments = segments_of_row(row);
        let text = row_text(&segments);
        if segments.is_empty() || text.trim().is_empty() {
            continue;
        }
        lines.push(Line {
            x0: segments.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min),
            x1: segments
                .iter()
                .map(|s| s.x1)
                .fold(f64::NEG_INFINITY, f64::max),
            top: segments
                .iter()
                .map(|s| s.top)
                .fold(f64::NEG_INFINITY, f64::max),
            bottom: segments
                .iter()
                .map(|s| s.bottom)
                .fold(f64::INFINITY, f64::min),
            size: segments.iter().map(|s| s.size).fold(0.0, f64::max),
            text,
        });
    }
    let [ix0, iy0, ix1, iy1] = bbox;
    let reach = -2.0..=CAPTION_REACH;
    let mut best: Option<(f64, String)> = None;
    for (index, line) in lines.iter().enumerate() {
        if !is_caption_start(&line.text) || !(line.x1 > ix0 && line.x0 < ix1) {
            continue;
        }
        // Lines directly beneath with the same type size continue the caption.
        let mut block = vec![index];
        let mut bottom = line.bottom;
        while block.len() < CAPTION_LINES {
            let next = lines
                .iter()
                .enumerate()
                .filter(|(i, l)| {
                    !block.contains(i)
                        && l.top <= bottom + 1.0
                        && bottom - l.top <= 0.7 * line.size
                        && (l.size - line.size).abs() <= 1.0
                        && l.x1 > line.x0 - line.size
                        && l.x0 < line.x1 + line.size
                        && !is_caption_start(&l.text)
                })
                .max_by(|a, b| a.1.top.total_cmp(&b.1.top));
            let Some((i, l)) = next else {
                break;
            };
            block.push(i);
            bottom = l.bottom;
        }
        let below = iy0 - line.top;
        let above = bottom - iy1;
        let gap = if reach.contains(&below) {
            below
        } else if reach.contains(&above) {
            above
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(best_gap, _)| gap < *best_gap) {
            let text: Vec<&str> = block.iter().map(|i| lines[*i].text.as_str()).collect();
            best = Some((gap, text.join(" ")));
        }
    }
    let (_, text) = best?;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > CAPTION_CHARS {
        let cut: String = text.chars().take(CAPTION_CHARS).collect();
        return Some(format!("{}…", cut.trim_end()));
    }
    Some(text)
}
