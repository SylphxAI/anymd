use anymd_core::text_index::cite_check::{check_cite_items, CiteNormalization};
use anymd_core::text_index::{extract_cite_pages, TextBoundingBox};

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-selectable-table-v1.pdf")
}

#[test]
fn cite_check_selected_pages_support_real_pdf_and_wrong_page() {
    let pages = extract_cite_pages(&fixture(), 256 * 1024 * 1024, &[1, 999]).unwrap();
    assert_eq!(pages.len(), 2);
    assert!(pages[&999].is_err());
    let page = pages[&1].as_ref().unwrap();
    let item = page
        .positioned_items
        .iter()
        .find(|i| !i.text.trim().is_empty() && i.bounding_box.is_some())
        .unwrap();
    let location = item.bounding_box.unwrap();
    let result = check_cite_items(
        &page.positioned_items,
        &item.text,
        location,
        CiteNormalization::None,
        true,
        "text_item",
    );
    assert_eq!(result.verdict, "verified_exact");
    assert_eq!(result.locations[0].geometry_level, "char_estimated");
    let wrong = TextBoundingBox {
        left: 0.,
        bottom: 0.,
        right: 1.,
        top: 1.,
    };
    assert_eq!(
        check_cite_items(
            &page.positioned_items,
            &item.text,
            wrong,
            CiteNormalization::None,
            true,
            "text_item"
        )
        .verdict,
        "unmatched"
    );
}

#[test]
fn cite_check_scan_without_ocr_is_insufficient() {
    let scan = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/scanned-page.pdf");
    let pages = extract_cite_pages(&scan, 256 * 1024 * 1024, &[1]).unwrap();
    let page = pages[&1].as_ref().unwrap();
    assert!(page.text.trim().is_empty());
    let b = TextBoundingBox {
        left: 0.,
        bottom: 0.,
        right: 1000.,
        top: 1000.,
    };
    assert_eq!(
        check_cite_items(
            &page.positioned_items,
            "quote",
            b,
            CiteNormalization::None,
            true,
            "text_item"
        )
        .verdict,
        "insufficient_evidence"
    );
}

#[test]
fn cite_check_rejects_page_and_source_budgets_and_malformed_pdf() {
    assert!(extract_cite_pages(&fixture(), 1, &[1]).is_err());
    assert!(
        extract_cite_pages(&fixture(), 256 * 1024 * 1024, &(1..=21).collect::<Vec<_>>()).is_err()
    );
    assert!(extract_cite_pages(&fixture(), 256 * 1024 * 1024, &[0]).is_err());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("malformed.pdf");
    std::fs::write(&path, b"%PDF-1.4 malformed").unwrap();
    assert!(extract_cite_pages(&path, 1024, &[1]).is_err());
}

#[test]
fn cite_check_password_encrypted_pdf_fails_closed() {
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};
    let mut doc = lopdf::Document::load(fixture()).unwrap();
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "fixture-owner",
        user_password: "fixture-reader",
        key_length: 128,
        permissions: Permissions::COPYABLE,
    })
    .unwrap();
    doc.encrypt(&state).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("encrypted.pdf");
    doc.save(&path).unwrap();
    assert!(extract_cite_pages(&path, 256 * 1024 * 1024, &[1]).is_err());
}
