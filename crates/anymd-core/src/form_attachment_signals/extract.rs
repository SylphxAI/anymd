use lopdf::{Document, ObjectId};

use super::{
    attachments::extract_attachments, forms::extract_forms, walker::Walker, FormAttachmentSignals,
};

pub(crate) fn extract_form_attachment_signals(
    document: &Document,
    pages: &[(u32, ObjectId)],
    want_forms: bool,
    want_attachments: bool,
) -> FormAttachmentSignals {
    let mut output = FormAttachmentSignals::default();
    let Ok(root) = document.trailer.get(b"Root") else {
        return output;
    };
    if want_forms {
        let mut walker = Walker::new(document);
        output.form_fields = walker
            .dict(root)
            .and_then(|catalog| extract_forms(&mut walker, catalog, pages));
        #[cfg(test)]
        {
            output.form_materialized_array_items = walker.materialized_array_items;
            output.form_annotation_materialized_array_items =
                walker.annotation_materialized_array_items;
        }
        if walker.limited {
            output.form_fields = None;
            output.warnings.push("include_form_fields: COS traversal exceeded the bounded form-field limit; the surface was omitted.".into());
        }
    }
    if want_attachments {
        let mut walker = Walker::new(document);
        output.attachments = walker
            .dict(root)
            .and_then(|catalog| extract_attachments(&mut walker, catalog));
        #[cfg(test)]
        {
            output.attachment_materialized_array_items = walker.materialized_array_items;
        }
        if walker.limited {
            output.attachments = None;
            output.warnings.push("include_attachments: COS traversal exceeded the bounded embedded-file limit; the surface was omitted.".into());
        }
    }
    output
}
