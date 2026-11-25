use lopdf::Document;

use crate::cos_document::EncryptionFacts;

mod outline;
mod outline_destination;
mod page_labels;
mod walker;

use outline::extract_outline;
pub(crate) use outline::OutlineItem;
use page_labels::extract_page_labels;
pub(crate) use walker::MarkInfo;
use walker::{extract_mark_info, extract_permissions, Walker};

#[cfg(test)]
use lopdf::{Dictionary, Object};
#[cfg(test)]
use walker::MAX_ENTRIES;

#[derive(Default)]
pub(crate) struct CatalogSignals {
    pub page_labels: Option<Vec<String>>,
    pub mark_info: Option<MarkInfo>,
    pub permissions: Option<Vec<String>>,
    pub outline: Option<Vec<OutlineItem>>,
}

pub(crate) struct CatalogSignalRequest {
    pub page_labels: bool,
    pub permissions: bool,
    pub outline: bool,
}

pub(crate) fn extract_catalog_signals(
    document: &Document,
    encryption_facts: Option<EncryptionFacts>,
    num_pages: u32,
    request: CatalogSignalRequest,
) -> CatalogSignals {
    let mut output = CatalogSignals::default();
    let Ok(catalog) = document.catalog().cloned() else {
        return output;
    };
    if request.page_labels {
        let mut walker = Walker::new(document);
        output.page_labels = extract_page_labels(&mut walker, &catalog, num_pages);
    }
    if request.permissions {
        output.permissions = extract_permissions(encryption_facts);
        let mut walker = Walker::new(document);
        output.mark_info = extract_mark_info(&mut walker, &catalog);
    }
    if request.outline {
        let mut walker = Walker::new(document);
        output.outline = extract_outline(&mut walker, &catalog);
    }
    output
}

#[cfg(test)]
mod tests;
