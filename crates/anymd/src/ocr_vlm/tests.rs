use super::*;

#[test]
fn booleans_inherit_the_env_but_named_modes_override_it() {
    assert_eq!(
        resolve_engine(Some(OcrSelection::Enabled(true)), Some("tesseract")).unwrap(),
        OcrEngine::Tesseract
    );
    assert_eq!(resolve_engine(None, Some("vlm")).unwrap(), OcrEngine::Vlm);
    assert_eq!(
        resolve_engine(Some(OcrSelection::Engine(OcrEngine::Auto)), Some("vlm")).unwrap(),
        OcrEngine::Auto
    );
    assert_eq!(
        resolve_engine(Some(OcrSelection::Enabled(false)), Some("invalid")).unwrap(),
        OcrEngine::Auto
    );
    assert!(resolve_engine(None, Some("invalid")).is_err());
}

#[test]
fn selections_preserve_boolean_contract() {
    assert_eq!(
        serde_json::from_str::<OcrSelection>("false").unwrap(),
        OcrSelection::Enabled(false)
    );
    assert_eq!(
        serde_json::from_str::<OcrSelection>("\"vlm\"")
            .unwrap()
            .engine(),
        OcrEngine::Vlm
    );
    assert!(serde_json::from_str::<OcrSelection>("\"remote\"").is_err());
}
#[test]
fn manifest_lookup_needs_the_exact_asset_and_a_full_hash() {
    let hash = "a".repeat(64);
    let manifest = format!(
        "{hash}  anymd-ocr-vlm-linux-x64-gnu\n{} *anymd-ocr-vlm-win32-x64-msvc.exe\nshort  anymd-ocr-vlm-darwin-x64\n",
        "B".repeat(64)
    );
    assert_eq!(
        manifest_hash(&manifest, "anymd-ocr-vlm-linux-x64-gnu"),
        Some(hash)
    );
    assert_eq!(
        manifest_hash(&manifest, "anymd-ocr-vlm-win32-x64-msvc.exe"),
        Some("b".repeat(64))
    );
    assert_eq!(manifest_hash(&manifest, "anymd-ocr-vlm-darwin-x64"), None);
    assert_eq!(manifest_hash(&manifest, "anymd-ocr-vlm-linux-x64"), None);
    assert_eq!(
        companion_asset("win32-x64-msvc"),
        "anymd-ocr-vlm-win32-x64-msvc.exe"
    );
    assert_eq!(
        companion_asset("darwin-arm64"),
        "anymd-ocr-vlm-darwin-arm64"
    );
}
