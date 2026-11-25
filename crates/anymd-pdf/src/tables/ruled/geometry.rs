//! Line merging, connected components, and coverage.

/// Distance within which two lines count as touching or as one line.
pub(super) const SNAP: f64 = 2.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct Line {
    pub(super) at: f64,
    pub(super) from: f64,
    pub(super) to: f64,
    /// Every piece is an edge of a shaded box.
    pub(super) soft: bool,
}

/// Merge rules of one orientation: the same position within [`SNAP`], and
/// pieces that overlap or nearly touch along it.
pub(super) fn merge(mut rules: Vec<Line>) -> Vec<Line> {
    rules.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.from.total_cmp(&b.from)));
    let mut clusters: Vec<Vec<Line>> = Vec::new();
    for rule in rules {
        match clusters.last_mut() {
            Some(cluster) if (rule.at - cluster[0].at).abs() <= SNAP / 2.0 => cluster.push(rule),
            _ => clusters.push(vec![rule]),
        }
    }
    let mut out = Vec::new();
    for mut cluster in clusters {
        let at = cluster.iter().map(|l| l.at).sum::<f64>() / cluster.len() as f64;
        cluster.sort_by(|a, b| a.from.total_cmp(&b.from));
        let mut current: Option<Line> = None;
        for line in cluster {
            match current.as_mut() {
                Some(run) if line.from <= run.to + SNAP => {
                    run.to = run.to.max(line.to);
                    run.soft &= line.soft;
                }
                _ => {
                    out.extend(current.take());
                    current = Some(Line { at, ..line });
                }
            }
        }
        out.extend(current);
    }
    out.retain(|line| line.to - line.from >= SNAP * 2.0);
    out
}

pub(super) fn find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

pub(super) fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
}

/// Positions within [`SNAP`] of each other become one (their mean).
pub(super) fn cluster_positions(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    let mut out: Vec<Vec<f64>> = Vec::new();
    for value in values {
        match out.last_mut() {
            Some(group) if value - group[group.len() - 1] <= SNAP => group.push(value),
            _ => out.push(vec![value]),
        }
    }
    out.into_iter()
        .map(|group| group.iter().sum::<f64>() / group.len() as f64)
        .collect()
}

/// Whether some line at `at` covers most of `from..to`.
pub(super) fn covered(lines: &[&Line], at: f64, from: f64, to: f64) -> bool {
    let need = (to - from) * 0.6;
    lines
        .iter()
        .any(|line| (line.at - at).abs() <= SNAP && line.to.min(to) - line.from.max(from) >= need)
}

type Component<'a> = (Vec<&'a Line>, Vec<&'a Line>);

/// Group touching lines, with horizontals first and then verticals.
pub(super) fn components<'a>(
    horizontal: &'a [Line],
    vertical: &'a [Line],
) -> impl Iterator<Item = Component<'a>> {
    let count = horizontal.len() + vertical.len();
    let mut parent: Vec<usize> = (0..count).collect();
    for (h, hl) in horizontal.iter().enumerate() {
        for (v, vl) in vertical.iter().enumerate() {
            if vl.at >= hl.from - SNAP
                && vl.at <= hl.to + SNAP
                && hl.at >= vl.from - SNAP
                && hl.at <= vl.to + SNAP
            {
                union(&mut parent, h, horizontal.len() + v);
            }
        }
    }
    let mut components: std::collections::BTreeMap<usize, Component<'_>> =
        std::collections::BTreeMap::new();
    for index in 0..count {
        let root = find(&mut parent, index);
        let entry = components.entry(root).or_default();
        if index < horizontal.len() {
            entry.0.push(&horizontal[index]);
        } else {
            entry.1.push(&vertical[index - horizontal.len()]);
        }
    }
    components.into_values()
}
