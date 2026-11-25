use std::path::Path;

use anymd_core::markdown_layout::{self, load_document};
use anymd_formats::images::{self, ImageStore};

use crate::source_access::SourceAccessPolicy;

/// Write one PDF image to the store and build its Markdown.
pub(super) fn place_image(
    store: &ImageStore,
    image: &markdown_layout::EncodedImage,
    caption: Option<&str>,
    page: u32,
) -> Option<markdown_layout::Placed> {
    let stored = store.put(&image.bytes).ok()?;
    Some(markdown_layout::Placed {
        markdown: images::reference(caption, &stored, Some(&format!("page {page}"))),
        path: stored.path.display().to_string(),
        width: stored.width,
        height: stored.height,
    })
}

/// The embedded images of a local PDF's pages (all pages when `pages` is
/// `None`), exported to `store`: page, bounding box in points (y up), size,
/// caption and cached path.
pub fn pdf_image_listing(
    path: &Path,
    pages: Option<&[u32]>,
    store: &ImageStore,
) -> Result<Vec<serde_json::Value>, String> {
    let doc = load_document(path).map_err(|error| error.message)?;
    let repeated = markdown_layout::repeated_images(&doc);
    let place = |image: &markdown_layout::EncodedImage, caption: Option<&str>, page: u32| {
        place_image(store, image, caption, page)
    };
    let options = markdown_layout::ImageOptions {
        repeated: &repeated,
        place: &place,
    };
    Ok(markdown_layout::pdf_images(&doc, pages, &options)
        .into_iter()
        .map(|image| {
            serde_json::json!({
                "page": image.page,
                "bbox": image.bbox,
                "width": image.width,
                "height": image.height,
                "caption": image.caption,
                "path": image.path,
            })
        })
        .collect())
}

/// Add `embeddedImages` to each local PDF's data in a `structure` result.
pub fn attach_structure_images(
    result: &mut rmcp::model::CallToolResult,
    sources: &[crate::schema::PdfSource],
    policy: &SourceAccessPolicy,
) {
    let Some(store) = ImageStore::default_location() else {
        return;
    };
    let Some(results) = result
        .structured_content
        .as_mut()
        .and_then(|value| value.get_mut("results"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (entry, source) in results.iter_mut().zip(sources) {
        let Some(path) = source.path.as_deref() else {
            continue;
        };
        let Ok(admitted) = policy.admit_path(path) else {
            continue;
        };
        let pages = crate::page_selection::selected_pages(&source.pages)
            .ok()
            .flatten();
        let Some(data) = entry
            .get_mut("data")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        if let Ok(images) = pdf_image_listing(Path::new(&admitted), pages.as_deref(), &store) {
            if !images.is_empty() {
                data.insert("embeddedImages".into(), serde_json::Value::Array(images));
            }
        }
    }
}
