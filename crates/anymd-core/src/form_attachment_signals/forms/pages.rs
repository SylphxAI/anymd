use std::collections::HashMap;

use lopdf::{Dictionary, ObjectId};

use super::super::walker::{Walker, MAX_ENTRIES};

pub(super) struct WidgetPages {
    page_by_id: HashMap<ObjectId, u32>,
    annotation_pages: HashMap<ObjectId, u32>,
}

impl WidgetPages {
    pub(super) fn collect(walker: &mut Walker<'_>, pages: &[(u32, ObjectId)]) -> Option<Self> {
        if pages.len() > MAX_ENTRIES {
            walker.limited = true;
            return None;
        }
        let mut page_annotation_budget = MAX_ENTRIES - pages.len();
        let mut admitted_annotations = Vec::new();
        for (page, page_id) in pages {
            let Some(page_value) = walker.document.objects.get(page_id) else {
                continue;
            };
            let Some(page_dict) = walker.dict(page_value) else {
                if walker.failed || walker.limited {
                    return None;
                }
                continue;
            };
            let Ok(annots_value) = page_dict.get(b"Annots") else {
                continue;
            };
            let Some(annots) =
                walker.annotation_array_bounded(annots_value, page_annotation_budget)
            else {
                if walker.failed || walker.limited {
                    return None;
                }
                continue;
            };
            page_annotation_budget -= annots.len();
            admitted_annotations.push((*page, annots));
        }
        let page_by_id = pages
            .iter()
            .map(|(page, id)| (*id, *page))
            .collect::<HashMap<_, _>>();
        let mut annotation_pages = HashMap::new();
        for (page, annots) in admitted_annotations {
            for annot in annots {
                if let Ok(id) = annot.as_reference() {
                    annotation_pages.insert(id, page);
                }
            }
        }
        Some(Self {
            page_by_id,
            annotation_pages,
        })
    }

    pub(super) fn page_of(&self, dict: &Dictionary, widget_id: Option<ObjectId>) -> Option<u32> {
        dict.get(b"P")
            .ok()
            .and_then(|v| v.as_reference().ok())
            .and_then(|id| self.page_by_id.get(&id).copied())
            .or_else(|| widget_id.and_then(|id| self.annotation_pages.get(&id).copied()))
    }
}
