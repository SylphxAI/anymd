pub(super) fn sample_pages(total: u32, targets: Option<&[u32]>, max: usize) -> Vec<u32> {
    if total == 0 || max == 0 {
        return Vec::new();
    }
    let max = max.min(20);
    let mut values = targets
        .map(|pages| {
            pages
                .iter()
                .copied()
                .filter(|page| (1..=total).contains(page))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if targets.is_none() {
        if u64::from(total) <= max as u64 {
            return (1..=total).collect();
        }
        if max == 1 {
            return vec![1];
        }
        return (0..max)
            .map(|index| {
                let numerator = index as u64 * u64::from(total - 1);
                let denominator = (max - 1) as u64;
                1 + ((numerator + denominator / 2) / denominator) as u32
            })
            .collect();
    }
    values.sort_unstable();
    values.dedup();
    if values.len() <= max {
        return values;
    }
    if max == 1 {
        return values.into_iter().take(1).collect();
    }
    let mut selected = (0..max)
        .filter_map(|index| {
            let numerator = index * (values.len() - 1);
            let denominator = max - 1;
            let position = (numerator + denominator / 2) / denominator;
            values.get(position).copied()
        })
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    selected
}

#[cfg(test)]
mod tests {
    use super::sample_pages;

    #[test]
    fn sampling_matches_frozen_even_spacing_and_target_normalization() {
        assert_eq!(sample_pages(100, None, 5), vec![1, 26, 51, 75, 100]);
        assert_eq!(
            sample_pages(12, Some(&[12, 2, 8, 6, 4, 10, 2, 99]), 3),
            vec![2, 8, 12]
        );
        assert_eq!(sample_pages(12, Some(&[9, 1, 9]), 5), vec![1, 9]);
        assert_eq!(
            sample_pages(u32::MAX, None, 5),
            vec![1, 1_073_741_825, 2_147_483_648, 3_221_225_472, u32::MAX]
        );
        assert_eq!(sample_pages(u32::MAX, None, 1), vec![1]);
        assert!(sample_pages(u32::MAX, None, 0).is_empty());
        let maximally_requested = sample_pages(u32::MAX, None, usize::MAX);
        assert_eq!(maximally_requested.len(), 20);
        assert_eq!(maximally_requested.first(), Some(&1));
        assert_eq!(maximally_requested.last(), Some(&u32::MAX));
    }
}
