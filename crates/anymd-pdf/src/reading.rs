//! Reading order: bands, column gutters and an XY cut.

use crate::rows::Segment;

pub(crate) fn gaps(intervals: &mut [(f64, f64)], min_gap: f64) -> Vec<(f64, f64)> {
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

pub(crate) fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// A column gutter: a vertical strip with running text on both sides. A few
/// segments may cross it (figure labels, a spanning caption); they are
/// handled by the caller. Returns the gutter's (start, end).
pub(crate) fn column_cut(segments: &[Segment], body: f64, depth: usize) -> Option<(f64, f64)> {
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
            // Inside a column, a further split needs lines of running text,
            // not the short cells of a two-column table.
            let min_chars = if depth == 0 { 18.0 } else { 30.0 };
            side_width >= width * 0.2 && median(&mut chars) >= min_chars && median(&mut fill) >= 0.55
        };
        // A side that is itself two or more text columns also counts (three-
        // column layouts).
        let columns_ok = |side: &[&Segment]| {
            side_ok(side) || {
                let owned: Vec<Segment> = side.iter().map(|s| (*s).clone()).collect();
                owned.len() < segments.len() && column_cut(&owned, body, depth + 1).is_some()
            }
        };
        if columns_ok(&left) && columns_ok(&right) {
            let score = end - start;
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some(((start, end), score));
            }
        }
    }
    best.map(|(gutter, _)| gutter)
}

/// Order segments into reading-order regions (each region is top-to-bottom).
pub(crate) fn reading_regions(segments: Vec<Segment>, body: f64, depth: usize) -> Vec<Vec<Segment>> {
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
    // A band too small or too table-like to show its columns takes the
    // page's column gutter when no line crosses it and running text sits
    // beside it (a table or figure inside one column of a two-column page).
    let cuts: Vec<Option<(f64, f64)>> = bands.iter().map(|band| column_cut(band, body, depth)).collect();
    let template = cuts
        .iter()
        .zip(&bands)
        .filter_map(|(cut, band)| cut.map(|gutter| (gutter, band.len())))
        .max_by_key(|(_, count)| *count)
        .map(|(gutter, _)| gutter)
        .filter(|_| depth == 0);
    let fits = |band: &[Segment], (start, end): (f64, f64)| {
        let crosses = band.iter().any(|s| s.x0 < start - 0.5 && s.x1 > end + 0.5);
        let left = band.iter().filter(|s| s.x1 <= start + 0.5).collect::<Vec<_>>();
        let right = band.iter().filter(|s| s.x0 >= end - 0.5).collect::<Vec<_>>();
        let prose = |side: &[&Segment]| side.iter().any(|s| s.chars() >= 25);
        !crosses && !left.is_empty() && !right.is_empty() && (prose(&left) || prose(&right))
    };
    let mut cuts: Vec<Option<(f64, f64)>> = cuts
        .into_iter()
        .zip(&bands)
        .map(|(cut, band)| cut.or_else(|| template.filter(|gutter| fits(band, *gutter))))
        .collect();
    // A band between two bands split at the page's gutter, with nothing
    // crossing it, is split there too (a table row beside a chart).
    if let Some((start, end)) = template {
        for index in 1..cuts.len().saturating_sub(1) {
            let clear = !bands[index].iter().any(|s| s.x0 < start - 0.5 && s.x1 > end + 0.5);
            if cuts[index].is_none() && clear && cuts[index - 1].is_some() && cuts[index + 1].is_some() {
                cuts[index] = template;
            }
        }
    }
    // Merge consecutive bands that share the same column gutter.
    let mut groups: Vec<(Option<(f64, f64)>, Vec<Segment>)> = Vec::new();
    for (band, cut) in bands.into_iter().zip(cuts) {
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
        match column_cut(&group, body, depth).or(cut) {
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
pub(crate) fn split_columns(group: Vec<Segment>, gutter: (f64, f64), body: f64, depth: usize) -> Vec<Vec<Segment>> {
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
