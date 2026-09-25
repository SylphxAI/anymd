use crate::schema::PageSpecifier;

pub(crate) const MAX_SELECTED_PAGES: usize = 10_001;
const MAX_RANGE_DELTA: u32 = 10_000;

pub(crate) fn selected_pages(spec: &Option<PageSpecifier>) -> Result<Option<Vec<u32>>, String> {
    let Some(spec) = spec else {
        return Ok(None);
    };
    let pages = match spec {
        PageSpecifier::Pages(values) => {
            if values.is_empty() {
                return Err("Page specification resulted in an empty set of pages.".into());
            }
            if values.len() > MAX_SELECTED_PAGES {
                return Err("Page specification exceeds 10001 selected pages.".into());
            }
            values.iter().map(|page| page.0).collect::<Vec<_>>()
        }
        PageSpecifier::Range(value) => parse_range(value)?,
    };
    let mut pages = pages;
    pages.sort_unstable();
    pages.dedup();
    if pages.is_empty() {
        return Err("Page specification resulted in an empty set of pages.".into());
    }
    Ok(Some(pages))
}

fn parse_range(value: &str) -> Result<Vec<u32>, String> {
    let mut admitted = 0usize;
    for raw in value.split(',') {
        let (_, _, count) = parse_part(raw.trim())?;
        admitted = admitted
            .checked_add(count)
            .filter(|count| *count <= MAX_SELECTED_PAGES)
            .ok_or_else(|| "Page specification exceeds 10001 selected pages.".to_string())?;
    }
    if admitted == 0 {
        return Err("Page specification resulted in an empty set of pages.".into());
    }

    let mut pages = Vec::with_capacity(admitted);
    for raw in value.split(',') {
        let (start, end, _) = parse_part(raw.trim())?;
        pages.extend(start..=end);
    }
    Ok(pages)
}

fn parse_part(part: &str) -> Result<(u32, u32, usize), String> {
    if part.contains('-') {
        let mut segments = part.split('-');
        let start_text = segments.next().unwrap_or_default();
        let end_text = segments.next().unwrap_or_default();
        let start = parse_ts_positive_page(start_text);
        let end = if end_text.is_empty() {
            start.map(|page| page.saturating_add(MAX_RANGE_DELTA))
        } else {
            parse_ts_positive_page(end_text)
        };
        let (Some(start), Some(end)) = (start, end) else {
            return Err(format!("Invalid page range values: {part}"));
        };
        if start > end {
            return Err(format!("Invalid page range values: {part}"));
        }
        let end = end.min(start.saturating_add(MAX_RANGE_DELTA));
        let count = u64::from(end) - u64::from(start) + 1;
        Ok((start, end, count as usize))
    } else {
        let Some(page) = parse_ts_positive_page(part) else {
            return Err(format!("Invalid page number: {part}"));
        };
        Ok((page, page, 1))
    }
}

fn parse_ts_positive_page(value: &str) -> Option<u32> {
    let value = value.trim_start();
    let value = value.strip_prefix('+').unwrap_or(value);
    let mut parsed = 0u32;
    let mut found = false;
    for byte in value.bytes().take_while(u8::is_ascii_digit) {
        found = true;
        parsed = parsed
            .checked_mul(10)?
            .checked_add(u32::from(byte - b'0'))?;
    }
    (found && parsed > 0).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ts_prefix_sort_dedupe_and_exact_range_cap() {
        assert_eq!(
            selected_pages(&Some(PageSpecifier::Range("3,1,1,2x".into()))).unwrap(),
            Some(vec![1, 2, 3])
        );
        let exact = selected_pages(&Some(PageSpecifier::Range("1-4294967295".into())))
            .unwrap()
            .unwrap();
        assert_eq!(exact.len(), MAX_SELECTED_PAGES);
        assert_eq!(exact.first(), Some(&1));
        assert_eq!(exact.last(), Some(&10_001));
    }

    #[test]
    fn raw_admission_rejects_max_plus_one_before_materialization() {
        let exact = (0..MAX_SELECTED_PAGES)
            .map(|_| "1")
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            selected_pages(&Some(PageSpecifier::Range(exact))).unwrap(),
            Some(vec![1])
        );
        let oversized = (0..=MAX_SELECTED_PAGES)
            .map(|_| "1")
            .collect::<Vec<_>>()
            .join(",");
        assert!(selected_pages(&Some(PageSpecifier::Range(oversized)))
            .unwrap_err()
            .contains("10001"));
    }

    #[test]
    fn rejects_empty_invalid_descending_and_overflow() {
        for value in ["", "0", "abc", "-5", "5-3", "1-   x", "4294967296"] {
            assert!(selected_pages(&Some(PageSpecifier::Range(value.into()))).is_err());
        }
        assert!(selected_pages(&Some(PageSpecifier::Pages(Vec::new()))).is_err());
    }

    #[test]
    fn uses_only_the_exact_second_hyphen_segment_like_ts_parse_range_part() {
        let open = selected_pages(&Some(PageSpecifier::Range("1--5".into())))
            .unwrap()
            .unwrap();
        assert_eq!(open.len(), MAX_SELECTED_PAGES);
        assert_eq!(open.first(), Some(&1));
        assert_eq!(open.last(), Some(&10_001));
        assert_eq!(
            selected_pages(&Some(PageSpecifier::Range("1-   ".into()))).unwrap(),
            Some((1..=10_001).collect())
        );
        assert_eq!(
            selected_pages(&Some(PageSpecifier::Range("1-2-999".into()))).unwrap(),
            Some(vec![1, 2])
        );
    }
}
