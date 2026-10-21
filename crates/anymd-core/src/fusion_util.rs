//! Small numeric and warning helpers shared by the fusion, table and
//! accessibility passes. Each rule is stated once: a ratio shown to a reader is
//! rounded to two decimals, a UTF-16 length is what the MCP contract counts, a
//! warning is listed once, and removing a stub warning removes the field.

/// A ratio or score as the contract shows it: two decimals.
pub(crate) fn round_ratio(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// The UTF-16 length of `text`, the unit the MCP text contract counts in.
pub(crate) fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Adds `warning` once; an absent list becomes a list of this warning.
pub(crate) fn append_unique_warning(warnings: &mut Option<Vec<String>>, warning: String) {
    let values = warnings.get_or_insert_with(Vec::new);
    if !values.contains(&warning) {
        values.push(warning);
    }
}

/// Drops `stub` from the warnings; an empty list becomes no list at all.
pub(crate) fn remove_stub_warning(warnings: &mut Option<Vec<String>>, stub: &str) {
    if let Some(values) = warnings {
        values.retain(|warning| warning != stub);
        if values.is_empty() {
            *warnings = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ratio_is_rounded_to_two_decimals() {
        assert_eq!(round_ratio(0.123_456), 0.12);
        assert_eq!(round_ratio(0.125), 0.13);
        assert_eq!(round_ratio(1.0), 1.0);
    }

    #[test]
    fn a_utf16_length_counts_surrogate_pairs_as_two() {
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!(utf16_len(""), 0);
    }

    #[test]
    fn a_warning_is_appended_once_and_a_stub_removed_with_its_empty_list() {
        let mut warnings = None;
        append_unique_warning(&mut warnings, "one".into());
        append_unique_warning(&mut warnings, "one".into());
        append_unique_warning(&mut warnings, "two".into());
        assert_eq!(
            warnings.as_deref(),
            Some(&["one".to_string(), "two".to_string()][..])
        );

        remove_stub_warning(&mut warnings, "one");
        remove_stub_warning(&mut warnings, "two");
        assert!(warnings.is_none(), "an emptied list is removed");
    }
}
