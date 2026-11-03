//! The `pages` selector shared by `read_pdf` and `search_pdf`: one parser for
//! the range string and the array form, so both tools accept and refuse exactly
//! the same specifications. Each tool supplies its own error type; the code is
//! always `InvalidParams`, and the messages are the title's public contract.

/// The most pages one request may select.
pub(crate) const MAX_SELECTED_PAGES: usize = 10_001;

/// The error a tool returns for a refused page specification.
pub(crate) trait PageSpecError: Sized {
    fn invalid_params(message: impl Into<String>) -> Self;
}

/// `None` selects every page. An array is a page list; a string is a comma
/// separated list of pages and ranges (`"1-3,5"`), where `+N` is accepted, an
/// open range reaches 10 000 pages and a trailing non-digit is ignored, as the
/// JavaScript client reads a page number.
///
/// `empty_string_means_invalid` keeps `read_pdf`'s historical refusal of `""`
/// (it reads it as "Invalid page number: ") while `search_pdf` accepts it as an
/// empty list and refuses it with the empty-set message.
pub(crate) fn parse_page_spec<E: PageSpecError>(
    pages_spec: &Option<serde_json::Value>,
    empty_string_means_invalid: bool,
) -> Result<Option<Vec<u32>>, E> {
    let Some(spec) = pages_spec else {
        return Ok(None);
    };
    let mut wanted = Vec::new();
    if let Some(arr) = spec.as_array() {
        if arr.is_empty() {
            return Err(E::invalid_params(
                "Page specification resulted in an empty set of pages.",
            ));
        }
        for value in arr {
            let page = value
                .as_u64()
                .filter(|page| *page > 0 && *page <= u64::from(u32::MAX));
            let Some(page) = page else {
                return Err(E::invalid_params(
                    "Page numbers in array must be positive integers.",
                ));
            };
            wanted.push(page as u32);
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(E::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else if let Some(ranges) = spec.as_str() {
        if ranges.is_empty() && empty_string_means_invalid {
            return Err(E::invalid_params("Invalid page number: "));
        }
        for raw_part in ranges.split(',') {
            let part = raw_part.trim();
            if let Some((start_text, end_text)) = split_range(part) {
                let start = parse_ts_positive_page(start_text);
                let end = if end_text.trim().is_empty() {
                    start.map(|page| page.saturating_add(10_000))
                } else {
                    parse_ts_positive_page(end_text)
                };
                let (Some(start), Some(end)) = (start, end) else {
                    return Err(E::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                };
                if start > end {
                    return Err(E::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                }
                wanted.extend(start..=end.min(start.saturating_add(10_000)));
            } else {
                let Some(page) = parse_ts_positive_page(part) else {
                    return Err(E::invalid_params(format!("Invalid page number: {part}")));
                };
                wanted.push(page);
            }
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(E::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else {
        return Err(E::invalid_params(
            "Page specification must be a non-empty range string or array of positive integers.",
        ));
    }
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        return Err(E::invalid_params(
            "Page specification resulted in an empty set of pages.",
        ));
    }
    Ok(Some(wanted))
}

/// A range part as its first hyphen ranges it: the segment before the first
/// hyphen and the one after it. `"1--5"` is an open range from 1 (the middle
/// segment is skipped, as the JavaScript client splits it), `"1-2-999"` stops
/// at 2.
fn split_range(part: &str) -> Option<(&str, &str)> {
    let mut segments = part.split('-');
    let start = segments.next()?;
    let end = segments.next()?;
    Some((start, end))
}

fn parse_ts_positive_page(value: &str) -> Option<u32> {
    let value = value.trim_start();
    let value = value.strip_prefix('+').unwrap_or(value);
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
        .filter(|page| *page > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, PartialEq)]
    struct TestError(String);

    impl PageSpecError for TestError {
        fn invalid_params(message: impl Into<String>) -> Self {
            Self(message.into())
        }
    }

    fn parse(spec: serde_json::Value) -> Result<Option<Vec<u32>>, TestError> {
        parse_page_spec(&Some(spec), false)
    }

    #[test]
    fn reads_lists_ranges_open_ranges_and_javascript_page_tokens() {
        assert_eq!(parse(json!([3, 1, 3])), Ok(Some(vec![1, 3])));
        assert_eq!(parse(json!("2-4,7")), Ok(Some(vec![2, 3, 4, 7])));
        assert_eq!(parse(json!("+5")), Ok(Some(vec![5])));
        assert_eq!(parse(json!("1x")), Ok(Some(vec![1])));
        assert_eq!(parse(json!("4-")), Ok(Some((4..=10_004).collect())));
        assert_eq!(
            parse(json!([])),
            Err(TestError(
                "Page specification resulted in an empty set of pages.".into()
            ))
        );
        assert_eq!(
            parse(json!("10-1")),
            Err(TestError("Invalid page range values: 10-1".into()))
        );
        assert_eq!(
            parse(json!("0")),
            Err(TestError("Invalid page number: 0".into()))
        );
        assert_eq!(
            parse(json!(1)),
            Err(TestError(
                "Page specification must be a non-empty range string or array of positive integers."
                    .into()
            ))
        );
    }

    #[test]
    fn an_empty_string_is_refused_by_the_tool_that_cannot_read_it() {
        let refused = |strict| parse_page_spec::<TestError>(&Some(json!("")), strict).is_err();
        assert!(refused(true), "read_pdf refuses an empty range string");
        assert!(refused(false), "search_pdf names no page");
        assert_eq!(parse_page_spec::<TestError>(&None, false), Ok(None));
    }

    #[test]
    fn refuses_more_than_the_selected_page_limit() {
        let too_many: Vec<u32> = (1..=u32::try_from(MAX_SELECTED_PAGES + 1).unwrap()).collect();
        assert_eq!(
            parse(json!(too_many)),
            Err(TestError(
                "Page specification exceeds the maximum of 10001 selected pages.".into()
            ))
        );
    }
}
