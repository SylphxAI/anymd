use std::path::Path;

use anymd_core::markdown_layout::{load_document, SPARSE_PAGE_CHARS};

use crate::source_access::SourceAccessPolicy;

use super::cache::cache_key;
use super::ocr::{ocr_candidates, visible_chars};
use super::{OpenOptions, Opened, Unit};

#[test]
fn margin_scan_candidates_ignore_emitted_image_reference_length() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/scanned-page.pdf");
    let mut doc = load_document(&fixture).unwrap();
    let page = doc.get_pages()[&1];
    let media_box = doc
        .get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .get_mut(b"MediaBox")
        .unwrap()
        .as_array_mut()
        .unwrap();
    media_box[2] = 1200.into();
    media_box[3] = 1600.into();
    // Enlarging the page puts the painted scan below the 80% whole-page
    // image threshold; exporting it adds more than 24 visible path chars.
    let reference = Unit {
        number: 1,
        label: "page 1".into(),
        markdown: "![image](/cache/images/01234567890123456789012345678901234567.png)".into(),
    };
    let plain = Unit {
        markdown: String::new(),
        ..reference.clone()
    };
    assert!(visible_chars(&reference.markdown) > SPARSE_PAGE_CHARS);
    assert_eq!(
        ocr_candidates(std::slice::from_ref(&reference), &doc, true),
        vec![0]
    );
    assert_eq!(ocr_candidates(&[plain], &doc, true), vec![0]);
    assert!(ocr_candidates(&[reference], &doc, false).is_empty());
    // Page 2 of the fixture has a native text line over its raster image.
    let native = Unit {
        number: 2,
        label: "page 2".into(),
        markdown: "Title".into(),
    };
    assert!(ocr_candidates(&[native], &doc, true).is_empty());
}

#[test]
fn disabled_ocr_has_a_distinct_cache_key_without_tesseract() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let automatic = OpenOptions::default();
    let disabled = OpenOptions {
        ocr: Some(false),
        ..Default::default()
    };
    assert_ne!(
        cache_key(file.path(), false, &automatic),
        cache_key(file.path(), false, &disabled)
    );
}

#[test]
fn opens_csv_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.csv");
    std::fs::write(&path, "a,b\n1,2\n").unwrap();
    let opened = Opened::open(
        path.to_str().unwrap(),
        &SourceAccessPolicy::unrestricted(),
        &OpenOptions::default(),
    )
    .unwrap();
    assert_eq!(opened.format, "csv");
    let units = opened.units(&[1]).unwrap();
    assert!(units[0].markdown.contains("|a|b|"), "{}", units[0].markdown);
}
