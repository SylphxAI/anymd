//! Page-level spatial grid detection and admission.

use super::PageText;
use crate::text_index::TextBoundingBox;

const TABLE_Y_TOLERANCE: f64 = 5.0;
pub(super) const TABLE_COLUMN_GAP: f64 = 15.0;
pub(super) const MAX_TABLE_ITEMS_PER_PAGE: usize = 4_096;
const MAX_TABLE_BOUNDARIES_PER_PAGE: usize = 256;
const MAX_TABLE_CELLS_PER_PAGE: usize = 16_384;

#[derive(Clone, Copy)]
pub(super) struct TableTextItem<'a> {
    pub(super) text: &'a str,
    pub(super) x: f64,
    y: f64,
    pub(super) bounding_box: TextBoundingBox,
}

#[derive(Clone)]
pub(super) struct TableTextRow<'a> {
    pub(super) y: f64,
    pub(super) items: Vec<TableTextItem<'a>>,
}

pub(super) struct PageGrid<'a> {
    pub(super) boundaries: Vec<f64>,
    pub(super) regions: Vec<Vec<TableTextRow<'a>>>,
}

pub(super) struct GridLimitExceeded;

fn table_text_item(text: &str, bounding_box: Option<TextBoundingBox>) -> Option<TableTextItem<'_>> {
    let text = text.trim();
    let bounding_box = bounding_box?;
    let finite = [
        bounding_box.left,
        bounding_box.bottom,
        bounding_box.right,
        bounding_box.top,
    ]
    .into_iter()
    .all(f64::is_finite);
    (!text.is_empty() && finite).then_some(TableTextItem {
        text,
        x: bounding_box.left,
        y: bounding_box.bottom,
        bounding_box,
    })
}

pub(super) fn detect_page_grid(
    page: &PageText,
    page_content_geometry: bool,
) -> Result<Option<PageGrid<'_>>, GridLimitExceeded> {
    // Text segmentation preserves every source PDF text part as a typed run.
    // Tables consume those source parts, matching the pre-segmentation TS
    // extractor boundary and keeping their independent 4,096-item admission
    // reachable even when nearby selectable text joins into one public item.
    let mut items = Vec::new();
    'admission: for item in &page.positioned_items {
        if item.runs.is_empty() {
            if let Some(item) = table_text_item(&item.text, item.bounding_box) {
                items.push(item);
            }
        } else {
            for run in &item.runs {
                if let Some(item) = table_text_item(&run.text, run.bounding_box) {
                    items.push(item);
                }
                if items.len() > MAX_TABLE_ITEMS_PER_PAGE {
                    break 'admission;
                }
            }
        }
        if items.len() > MAX_TABLE_ITEMS_PER_PAGE {
            break;
        }
    }
    if items.len() > MAX_TABLE_ITEMS_PER_PAGE {
        return Err(GridLimitExceeded);
    }
    items.sort_by(|left, right| right.y.total_cmp(&left.y));
    let mut rows = Vec::<TableTextRow<'_>>::new();
    for item in items {
        if let Some(row) = rows.last_mut() {
            if (row.y - item.y).abs() <= TABLE_Y_TOLERANCE {
                row.items.push(item);
                continue;
            }
        }
        rows.push(TableTextRow {
            y: item.y,
            items: vec![item],
        });
    }
    for row in &mut rows {
        row.items.sort_by(|left, right| left.x.total_cmp(&right.x));
        if page_content_geometry {
            for index in 0..row.items.len().saturating_sub(1) {
                let next_x = row.items[index + 1].x;
                row.items[index].bounding_box.right =
                    row.items[index].bounding_box.right.max(next_x);
            }
        }
    }
    let candidate_rows = rows
        .into_iter()
        .filter(|row| row.items.len() >= 2)
        .collect::<Vec<_>>();
    if candidate_rows.len() < 2 {
        return Ok(None);
    }
    let mut x_positions = candidate_rows
        .iter()
        .flat_map(|row| row.items.iter().map(|item| item.x))
        .collect::<Vec<_>>();
    x_positions.sort_by(f64::total_cmp);
    let mut boundaries = x_positions.first().copied().into_iter().collect::<Vec<_>>();
    for pair in x_positions.windows(2) {
        if pair[1] - pair[0] >= TABLE_COLUMN_GAP {
            boundaries.push(pair[1]);
        }
    }
    if boundaries.len() < 2 {
        return Ok(None);
    }
    if boundaries.len() > MAX_TABLE_BOUNDARIES_PER_PAGE {
        return Err(GridLimitExceeded);
    }

    let mut regions = Vec::<Vec<TableTextRow<'_>>>::new();
    let mut current = Vec::new();
    for row in candidate_rows {
        let aligned = row
            .items
            .iter()
            .filter(|item| {
                boundaries
                    .iter()
                    .any(|boundary| (item.x - boundary).abs() < TABLE_COLUMN_GAP)
            })
            .count();
        if aligned >= boundaries.len() - 1 {
            current.push(row);
        } else if current.len() >= 2 {
            regions.push(std::mem::take(&mut current));
        } else {
            current.clear();
        }
    }
    if current.len() >= 2 {
        regions.push(current);
    }

    let page_cell_count = regions.iter().try_fold(0usize, |count, region| {
        region
            .len()
            .checked_mul(boundaries.len())
            .and_then(|cells| count.checked_add(cells))
    });
    if page_cell_count.is_none_or(|count| count > MAX_TABLE_CELLS_PER_PAGE) {
        return Err(GridLimitExceeded);
    }

    Ok(Some(PageGrid {
        boundaries,
        regions,
    }))
}
