//! Recover candidate grids and score their geometry from OCR words.

use std::collections::BTreeSet;

use crate::fusion_util::round_ratio;
use crate::ocr_fusion::OcrPage;

use super::{cmp_f64, Box2d};

const Y_TOLERANCE: f64 = 5.0;
const COLUMN_GAP_THRESHOLD: f64 = 15.0;
const MIN_ROWS: usize = 2;
const MIN_COLS: usize = 2;
const MIN_ROW_ITEMS: usize = 2;
const MAX_OCR_WORDS_PER_PAGE: usize = 50_000;

#[derive(Clone, Debug)]
pub(super) struct TextItem {
    pub(super) text: String,
    pub(super) x: f64,
    y: f64,
    pub(super) box_: Box2d,
}

#[derive(Clone, Debug)]
pub(super) struct TextRow {
    y: f64,
    pub(super) items: Vec<TextItem>,
}

#[derive(Clone, Debug)]
pub(super) struct Region {
    pub(super) rows: Vec<TextRow>,
    pub(super) columns: Vec<f64>,
}

pub(super) fn regions_for_page(page: &OcrPage) -> Vec<Region> {
    identify_regions(cluster_rows(text_items(page)))
}

fn text_items(page: &OcrPage) -> Vec<TextItem> {
    page.words
        .as_deref()
        .unwrap_or_default()
        .iter()
        .take(MAX_OCR_WORDS_PER_PAGE)
        .filter_map(|word| {
            let text = word.text.trim();
            let box_ = Box2d::from_value(word.bounding_box.as_ref()?)?;
            (!text.is_empty()).then(|| TextItem {
                text: text.to_string(),
                x: box_.left,
                y: box_.bottom,
                box_,
            })
        })
        .collect()
}

fn cluster_rows(mut items: Vec<TextItem>) -> Vec<TextRow> {
    items.sort_by(|left, right| cmp_f64(right.y, left.y));
    let Some(first) = items.first().cloned() else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut current = TextRow {
        y: first.y,
        items: vec![first],
    };
    for item in items.into_iter().skip(1) {
        if (current.y - item.y).abs() <= Y_TOLERANCE {
            current.items.push(item);
        } else {
            current
                .items
                .sort_by(|left, right| cmp_f64(left.x, right.x));
            rows.push(current);
            current = TextRow {
                y: item.y,
                items: vec![item],
            };
        }
    }
    current
        .items
        .sort_by(|left, right| cmp_f64(left.x, right.x));
    rows.push(current);
    rows
}

fn detect_columns(rows: &[TextRow]) -> Vec<f64> {
    let mut positions = rows
        .iter()
        .flat_map(|row| row.items.iter().map(|item| item.x))
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| cmp_f64(*left, *right));
    let Some(first) = positions.first().copied() else {
        return Vec::new();
    };
    let mut columns = vec![first];
    for pair in positions.windows(2) {
        if pair[1] - pair[0] >= COLUMN_GAP_THRESHOLD {
            columns.push(pair[1]);
        }
    }
    columns
}

pub(super) fn column_for(x: f64, columns: &[f64]) -> usize {
    columns
        .iter()
        .rposition(|boundary| x >= *boundary - COLUMN_GAP_THRESHOLD / 2.0)
        .unwrap_or(0)
}

fn identify_regions(rows: Vec<TextRow>) -> Vec<Region> {
    let candidates = rows
        .into_iter()
        .filter(|row| row.items.len() >= MIN_ROW_ITEMS)
        .collect::<Vec<_>>();
    if candidates.len() < MIN_ROWS {
        return Vec::new();
    }
    let columns = detect_columns(&candidates);
    if columns.len() < MIN_COLS {
        return Vec::new();
    }
    let mut regions = Vec::new();
    let mut current = Vec::new();
    for row in candidates {
        let aligned = row
            .items
            .iter()
            .filter(|item| {
                columns
                    .iter()
                    .any(|boundary| (item.x - boundary).abs() < COLUMN_GAP_THRESHOLD)
            })
            .count();
        if aligned >= MIN_COLS - 1 {
            current.push(row);
        } else {
            if current.len() >= MIN_ROWS {
                regions.push(Region {
                    rows: std::mem::take(&mut current),
                    columns: columns.clone(),
                });
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= MIN_ROWS {
        regions.push(Region {
            rows: current,
            columns,
        });
    }
    regions
}

pub(super) fn row_spacing_consistency(rows: &[TextRow]) -> f64 {
    if rows.len() < 3 {
        return if rows.len() >= 2 { 1.0 } else { 0.0 };
    }
    let spacings = rows
        .windows(2)
        .map(|pair| (pair[0].y - pair[1].y).abs())
        .collect::<Vec<_>>();
    let average = spacings.iter().sum::<f64>() / spacings.len() as f64;
    if average <= 0.0 {
        return 0.0;
    }
    let variance = spacings
        .iter()
        .map(|spacing| (spacing - average).powi(2))
        .sum::<f64>()
        / spacings.len() as f64;
    round_ratio((1.0 - variance.sqrt() / average).max(0.0))
}

pub(super) fn row_alignment(rows: &[TextRow], columns: &[f64]) -> f64 {
    if rows.is_empty() || columns.is_empty() {
        return 0.0;
    }
    let total = rows
        .iter()
        .map(|row| {
            let occupied = row
                .items
                .iter()
                .map(|item| column_for(item.x, columns))
                .collect::<BTreeSet<_>>()
                .len();
            (occupied as f64 / columns.len() as f64).min(1.0)
        })
        .sum::<f64>();
    round_ratio(total / rows.len() as f64)
}

pub(super) fn confidence(rows: &[TextRow], columns: &[f64]) -> f64 {
    if rows.len() < MIN_ROWS || columns.len() < MIN_COLS {
        return 0.0;
    }
    let mut score = 0.0;
    let mut checks = 0usize;
    for row in rows {
        score += row
            .items
            .iter()
            .map(|item| column_for(item.x, columns))
            .collect::<BTreeSet<_>>()
            .len() as f64
            / columns.len() as f64;
        checks += 1;
    }
    let spacings = rows
        .windows(2)
        .map(|pair| (pair[0].y - pair[1].y).abs())
        .collect::<Vec<_>>();
    if !spacings.is_empty() {
        let average = spacings.iter().sum::<f64>() / spacings.len() as f64;
        let variance = spacings
            .iter()
            .map(|spacing| (spacing - average).powi(2))
            .sum::<f64>()
            / spacings.len() as f64;
        score += if average > 0.0 {
            (1.0 - variance.sqrt() / average).max(0.0)
        } else {
            0.0
        };
        checks += 1;
    }
    (score / checks as f64).min(1.0)
}

pub(super) fn infer_span(box_: Option<Box2d>, column: usize, columns: &[f64]) -> usize {
    let Some(box_) = box_ else { return 1 };
    let mut span = 1;
    for boundary in columns.iter().skip(column + 1) {
        if box_.right >= *boundary - COLUMN_GAP_THRESHOLD / 2.0 {
            span += 1;
        } else {
            break;
        }
    }
    span.clamp(1, columns.len() - column)
}
