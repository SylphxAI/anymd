use std::path::PathBuf;

use crate::url_fetch::url_source_to_temp_file;

use super::{SearchPdfError, SearchPdfSource};

pub(super) fn validate_source(source: &SearchPdfSource) -> Result<(), SearchPdfError> {
    let has_path = source.path.as_ref().is_some_and(|value| !value.is_empty());
    let has_url = source.url.as_ref().is_some_and(|value| !value.is_empty());
    match (has_path, has_url) {
        (true, false) | (false, true) => Ok(()),
        (false, false) => Err(SearchPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
        (true, true) => Err(SearchPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
    }
}

pub(super) fn source_label(source: &SearchPdfSource) -> String {
    source
        .path
        .clone()
        .or_else(|| source.url.clone())
        .unwrap_or_else(|| "unknown".into())
}

pub(super) fn resolve_source(
    source: &SearchPdfSource,
    temps: &mut Vec<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(path) = source.path.as_ref().filter(|value| !value.is_empty()) {
        Ok(PathBuf::from(path))
    } else if let Some(url) = source.url.as_ref().filter(|value| !value.is_empty()) {
        match url_source_to_temp_file(url) {
            Ok(temp) => {
                temps.push(temp.clone());
                Ok(temp)
            }
            Err(message) => Err(message),
        }
    } else {
        Err("Provide exactly one of path or url for each PDF source.".into())
    }
}
