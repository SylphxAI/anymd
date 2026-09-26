use crate::blocks::{region_blocks, Block, PageTables};
use crate::extract::{Glyph, Rule};
use crate::rows::{segments_of_row, Segment};
use crate::tables::ruled::{ruled_tables, Ruled};
use crate::tables::{is_numeric, is_year, strip_leaders, Cell, Grid};

const SIZE: f64 = 9.0;

/// Glyphs of a proportional font: narrow i, l and punctuation, wide m and
/// w, half an em for the rest; quarter-em word spaces.
fn proportional(text: &str, x: f64, base: f64) -> Vec<Glyph> {
    let mut out = Vec::new();
    let mut cursor = x;
    for ch in text.chars() {
        let advance = SIZE
            * match ch {
                ' ' => {
                    cursor += SIZE * 0.25;
                    continue;
                }
                'i' | 'l' | '.' | ',' | 'I' => 0.28,
                'm' | 'w' | 'M' | 'W' => 0.8,
                '0'..='9' => 0.5,
                _ => 0.55,
            };
        out.push(Glyph {
            x0: cursor,
            x1: cursor + advance,
            base,
            size: SIZE,
            text: ch.to_string(),
            space: false,
        });
        cursor += advance;
    }
    out
}

/// One printed line of (text, x) cells, split into segments like a real row.
fn line(cells: &[(&str, f64)], base: f64) -> Vec<Segment> {
    let mut row: Vec<Glyph> = Vec::new();
    for (text, x) in cells {
        row.extend(proportional(text, *x, base));
    }
    segments_of_row(row)
}

/// The tables `region_blocks` makes from these lines.
fn tables_of(lines: Vec<Vec<Segment>>) -> Vec<Vec<Vec<String>>> {
    let mut blocks = Vec::new();
    region_blocks(
        lines.into_iter().flatten().collect(),
        SIZE,
        &mut PageTables::none(),
        &mut blocks,
    );
    blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Table(rows) => Some(rows),
            _ => None,
        })
        .collect()
}

#[test]
fn numbers_years_and_leaders() {
    for number in ["$1,234.50", "(12.3)", "-0.5%", "12^a", "–7.7"] {
        assert!(is_numeric(number), "{number}");
    }
    for text in ["118-158", "FY2025", "Decoder", "-", "289.1m"] {
        assert!(!is_numeric(text), "{text}");
    }
    assert!(is_year("2025") && is_year("2026^p") && is_year("FY2024"));
    assert!(!is_year("1234") && !is_year("20255"));
    assert_eq!(
        strip_leaders("Average hourly earnings ........"),
        "Average hourly earnings"
    );
    assert_eq!(strip_leaders("Total . . . . . . 12"), "Total 12");
    assert_eq!(strip_leaders("is a(n) . . ."), "is a(n) . . .");
}

#[test]
fn stacked_header_lines_become_one_header() {
    let tables = tables_of(vec![
        line(&[("July", 200.0), ("May", 260.0)], 700.0),
        line(&[("2025", 200.0), ("2026", 260.0)], 689.0),
        line(
            &[
                ("Hourly earnings", 72.0),
                ("11.32", 200.0),
                ("11.23", 260.0),
            ],
            674.0,
        ),
        line(
            &[("Weekly hours", 72.0), ("34.2", 200.0), ("34.3", 260.0)],
            663.0,
        ),
    ]);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0][0], ["", "July 2025", "May 2026"]);
    assert_eq!(tables[0][1], ["Hourly earnings", "11.32", "11.23"]);
}

#[test]
fn columns_closer_than_a_segment_gap_still_split() {
    // 0.8 em between "14,579" and "9,233": one segment, two cells.
    let gap = 4.0 * SIZE * 0.5 + 2.0 * SIZE * 0.28 + SIZE * 0.8;
    let tables = tables_of(vec![
        line(
            &[("Dataset", 72.0), ("FB15k", 150.0), ("JF17k", 150.0 + gap)],
            700.0,
        ),
        line(
            &[
                ("Entities", 72.0),
                ("14,579", 150.0),
                ("9,233", 150.0 + gap),
            ],
            689.0,
        ),
        line(
            &[("Types", 72.0), ("588", 150.0), ("511", 150.0 + gap)],
            678.0,
        ),
    ]);
    assert_eq!(tables[0][1], ["Entities", "14,579", "9,233"]);
    assert_eq!(tables[0][2], ["Types", "588", "511"]);
}

#[test]
fn wrapped_labels_stay_with_their_row() {
    let tables = tables_of(vec![
        line(
            &[("Title", 72.0), ("FY2025", 200.0), ("FY2026", 260.0)],
            700.0,
        ),
        line(
            &[
                ("Operation and", 72.0),
                ("$290.3", 200.0),
                ("$294.4", 260.0),
            ],
            685.0,
        ),
        line(&[("Maintenance", 72.0)], 675.0),
        line(
            &[("Procurement", 72.0), ("$167.5", 200.0), ("$167.5", 260.0)],
            660.0,
        ),
    ]);
    assert_eq!(
        tables[0][1],
        ["Operation and Maintenance", "$290.3", "$294.4"]
    );
    assert_eq!(tables[0][2], ["Procurement", "$167.5", "$167.5"]);
}

#[test]
fn value_rows_under_a_centred_label_stay_apart() {
    let tables = tables_of(vec![
        line(&[("Name", 72.0), ("Size", 130.0), ("Params", 190.0)], 700.0),
        line(&[("base", 130.0), ("124.4m", 190.0)], 688.0),
        line(&[("GPT-2", 72.0)], 683.0),
        line(&[("medium", 130.0), ("354.8m", 190.0)], 678.0),
        line(&[("ByT5", 72.0), ("small", 130.0), ("300m", 190.0)], 664.0),
    ]);
    assert_eq!(tables[0][1], ["GPT-2", "base", "124.4m"]);
    assert_eq!(tables[0][2], ["", "medium", "354.8m"]);
}

fn rule(horizontal: bool, at: f64, from: f64, to: f64) -> Rule {
    Rule {
        horizontal,
        at,
        from,
        to,
        soft: false,
    }
}

fn text_at(text: &str, x: f64, base: f64) -> Vec<Glyph> {
    proportional(text, x, base)
}

#[test]
fn ruled_grid_with_spanning_header() {
    // Columns at 72 | 150 | 225 | 300; rows at 700 | 688 | 676 | 664 | 652.
    // Row 0: "Group" spans columns 1-2 (no line at x=225); "Name" spans
    // rows 0-1 in column 0 (no line at y=688 there).
    let mut rules = vec![
        rule(true, 700.0, 72.0, 300.0),
        rule(true, 688.0, 150.0, 300.0),
        rule(true, 676.0, 72.0, 300.0),
        rule(true, 664.0, 72.0, 300.0),
        rule(true, 652.0, 72.0, 300.0),
        rule(false, 72.0, 652.0, 700.0),
        rule(false, 150.0, 652.0, 700.0),
        rule(false, 225.0, 652.0, 688.0),
        rule(false, 300.0, 652.0, 700.0),
    ];
    rules.push(rule(true, 700.0, 72.0, 300.0));
    let mut glyphs = Vec::new();
    glyphs.extend(text_at("Name", 80.0, 684.0));
    glyphs.extend(text_at("Group", 200.0, 691.0));
    glyphs.extend(text_at("A", 160.0, 679.0));
    glyphs.extend(text_at("B", 235.0, 679.0));
    glyphs.extend(text_at("x", 80.0, 667.0));
    glyphs.extend(text_at("1.5", 160.0, 667.0));
    glyphs.extend(text_at("2.5", 235.0, 667.0));
    glyphs.extend(text_at("y", 80.0, 655.0));
    glyphs.extend(text_at("3.5", 160.0, 655.0));
    glyphs.extend(text_at("4.5", 235.0, 655.0));
    let outside = text_at("Below the table", 72.0, 600.0);
    let count = outside.len();
    glyphs.extend(outside);
    let (tables, rest) = ruled_tables(&rules, glyphs);
    assert_eq!(tables.len(), 1);
    assert_eq!(rest.len(), count);
    let Ruled::Table { grid, .. } = &tables[0].content else {
        panic!("expected a table");
    };
    let rows = grid.clone().into_rows();
    assert_eq!(rows[0], ["Name", "Group A", "Group B"]);
    assert_eq!(rows[1], ["x", "1.5", "2.5"]);
    assert_eq!(rows[2], ["y", "3.5", "4.5"]);
}

#[test]
fn words_across_gridlines_are_a_chart_not_a_table() {
    let rules = vec![
        rule(true, 700.0, 72.0, 300.0),
        rule(true, 680.0, 72.0, 300.0),
        rule(true, 660.0, 72.0, 300.0),
        rule(false, 72.0, 660.0, 700.0),
        rule(false, 150.0, 660.0, 700.0),
        rule(false, 225.0, 660.0, 700.0),
        rule(false, 300.0, 660.0, 700.0),
    ];
    let mut glyphs = Vec::new();
    // Legend entries drawn over the gridlines.
    glyphs.extend(text_at("ByGPT5 (small)", 130.0, 690.0));
    glyphs.extend(text_at("ByGPT5 (base)", 205.0, 670.0));
    glyphs.extend(text_at("GPT-2", 80.0, 670.0));
    let (tables, rest) = ruled_tables(&rules, glyphs.clone());
    assert!(tables.is_empty());
    assert_eq!(rest.len(), glyphs.len());
}

#[test]
fn rows_banded_between_two_rules_split_by_line() {
    let rules = vec![
        rule(true, 700.0, 72.0, 300.0),
        rule(true, 688.0, 72.0, 300.0),
        rule(true, 650.0, 72.0, 300.0),
        rule(false, 72.0, 650.0, 700.0),
        rule(false, 150.0, 650.0, 700.0),
        rule(false, 225.0, 650.0, 700.0),
        rule(false, 300.0, 650.0, 700.0),
    ];
    let mut glyphs = Vec::new();
    glyphs.extend(text_at("Year", 80.0, 691.0));
    glyphs.extend(text_at("Total", 160.0, 691.0));
    glyphs.extend(text_at("Share", 235.0, 691.0));
    for (i, (year, total, share)) in [
        ("1950", "8320", "4.9"),
        ("1955", "8928", "5.3"),
        ("1960", "9342", "5.7"),
    ]
    .into_iter()
    .enumerate()
    {
        let base = 678.0 - i as f64 * 11.0;
        glyphs.extend(text_at(year, 80.0, base));
        glyphs.extend(text_at(total, 160.0, base));
        glyphs.extend(text_at(share, 235.0, base));
    }
    let (tables, _) = ruled_tables(&rules, glyphs);
    let Ruled::Table { grid, .. } = &tables[0].content else {
        panic!("expected a table");
    };
    let rows = grid.clone().into_rows();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[2], ["1955", "8928", "5.3"]);
}

#[test]
fn long_spanning_headings_are_written_once() {
    let cell = |text: &str, cols: usize| {
        Some(Cell {
            text: text.into(),
            cols,
            rows: 1,
        })
    };
    let grid = Grid {
        cells: vec![
            vec![
                cell("", 1),
                cell("Lower Paying Job Annual Taxable Wage & Salary", 2),
                None,
            ],
            vec![
                cell("", 1),
                cell("$0 - 9,999", 1),
                cell("$10,000 - 19,999", 1),
            ],
            vec![cell("$0 - 9,999", 1), cell("$0", 1), cell("$0", 1)],
        ],
        header_rows: 2,
    };
    let rows = grid.into_rows();
    assert_eq!(
        rows[0],
        [
            "",
            "Lower Paying Job Annual Taxable Wage & Salary $0 - 9,999",
            "$10,000 - 19,999"
        ]
    );
}
