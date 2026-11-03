//! Page selection for read_pdf: even sampling and joining extracted pages; the
//! `pages` specification itself is parsed once in `crate::pages`.

pub(super) fn join_page_text(pages: &[crate::document_twin::PageText]) -> String {
    pages
        .iter()
        .map(|page| page.text.as_str())
        .filter(|page| !page.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn evenly_sample_pages(total_pages: u32, max_samples: u32) -> Vec<u32> {
    let max_samples = max_samples.clamp(1, 20).min(total_pages.max(1));
    if total_pages <= max_samples {
        return (1..=total_pages).collect();
    }
    if max_samples == 1 {
        return vec![1];
    }
    let mut selected = Vec::with_capacity(max_samples as usize);
    for index in 0..max_samples {
        let numerator = u64::from(index) * u64::from(total_pages - 1);
        let denominator = u64::from(max_samples - 1);
        // Math.round for non-negative values.
        let offset = (numerator + denominator / 2) / denominator;
        selected.push(1 + offset as u32);
    }
    selected.sort_unstable();
    selected.dedup();
    selected
}

pub(super) fn select_pages(
    pages: &[crate::text_index::ExtractedPageText],
    requested_pages: Option<&[u32]>,
) -> (Vec<crate::document_twin::PageText>, Vec<u32>) {
    let all: Vec<crate::document_twin::PageText> = pages
        .iter()
        .enumerate()
        .map(|(i, extracted)| crate::document_twin::PageText {
            page: (i + 1) as u32,
            text: extracted.text.clone(),
            positioned_items: extracted.positioned_items.clone(),
        })
        .collect();
    let Some(wanted) = requested_pages else {
        return (all, Vec::new());
    };
    let total_pages = pages.len() as u32;
    let invalid = wanted
        .iter()
        .copied()
        .filter(|page| *page > total_pages)
        .collect();
    let selected = all
        .into_iter()
        .filter(|page| wanted.binary_search(&page.page).is_ok())
        .collect();
    (selected, invalid)
}
