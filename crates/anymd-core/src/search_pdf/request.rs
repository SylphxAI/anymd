use super::{source::validate_source, SearchPdfError, SearchPdfInput};

pub(super) fn validate_request(input: &SearchPdfInput) -> Result<(), SearchPdfError> {
    if input.sources.is_empty() {
        return Err(SearchPdfError::invalid_params(
            "sources must include at least one PDF source.",
        ));
    }
    if input.query.is_empty() {
        return Err(SearchPdfError::invalid_params("query must not be empty"));
    }
    if input.include_ocr_text_layer.unwrap_or(false) {
        return Err(SearchPdfError::invalid_request(
            "search_pdf include_ocr_text_layer requires an OCR provider; pure-Rust search uses the embedded text layer only.",
        ));
    }

    if matches!(input.max_pages, Some(0)) {
        return Err(SearchPdfError::invalid_params("max_pages must be >= 1"));
    }
    if matches!(input.max_matches_per_source, Some(0)) {
        return Err(SearchPdfError::invalid_params(
            "max_matches_per_source must be >= 1",
        ));
    }
    if input.max_pages.is_some_and(|value| value > 1000) {
        return Err(SearchPdfError::invalid_params("max_pages must be <= 1000"));
    }
    if input
        .max_matches_per_source
        .is_some_and(|value| value > 500)
    {
        return Err(SearchPdfError::invalid_params(
            "max_matches_per_source must be <= 500",
        ));
    }
    if input.context_chars.is_some_and(|value| value > 1000) {
        return Err(SearchPdfError::invalid_params(
            "context_chars must be <= 1000",
        ));
    }

    for source in &input.sources {
        validate_source(source)?;
        crate::pages::parse_page_spec::<SearchPdfError>(&source.pages, false)?;
    }
    Ok(())
}
