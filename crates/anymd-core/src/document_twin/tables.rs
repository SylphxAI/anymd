//! Selectable-text table detection, admission limits, and continuation linking.

mod continuation;
mod grid;
mod render;
#[cfg(test)]
mod tests;

use serde_json::{json, Value};

use self::continuation::link_table_continuations;
use self::grid::{detect_page_grid, GridLimitExceeded, PageGrid};
use self::render::render_table;
use super::PageText;

fn table_round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(crate) fn build_tables_with_admission(
    pages: &[PageText],
    page_content_geometry: bool,
) -> (Value, Vec<String>) {
    let mut tables = Vec::new();
    let mut warnings = Vec::new();
    for page in pages {
        match detect_page_grid(page, page_content_geometry) {
            Ok(Some(PageGrid {
                boundaries,
                regions,
            })) => {
                for (table_index, region) in regions.into_iter().enumerate() {
                    if let Some(table) = render_table(page.page, table_index, &region, &boundaries) {
                        tables.push(table);
                    }
                }
            }
            Ok(None) => {}
            Err(GridLimitExceeded) => warnings.push(format!(
                "Selectable table extraction skipped page {}: spatial grid exceeds the Rust admission limit.",
                page.page
            )),
        }
    }
    tables.sort_by_key(|table| {
        (
            table.get("page").and_then(Value::as_u64).unwrap_or(0),
            table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0),
        )
    });
    link_table_continuations(&mut tables);
    (json!(tables), warnings)
}

#[cfg(test)]
pub(crate) fn build_tables(pages: &[PageText]) -> Value {
    build_tables_with_admission(pages, false).0
}
