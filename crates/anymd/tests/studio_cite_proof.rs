//! The Studio fixture records native cite-check evidence, not illustrative numbers.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn studio_pdf_fixture_agrees_with_native_cite_check() {
    let studio = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/web");
    let proof: Value = serde_json::from_slice(
        &std::fs::read(studio.join("src/components/studio/sample-cite-proof.json")).unwrap(),
    )
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let request = tmp.path().join("request.json");
    let payload = json!({
        "args": {
            "operation": "cite_check",
            "normalization": "none",
            "sources": [{"path": studio.join("public/samples/sample.pdf")}],
            "citations": [{"id": "studio-preview", "page": proof["page"], "quote": proof["quote"]}]
        },
        "ocr_admitted": false
    });
    std::fs::write(&request, payload.to_string()).unwrap();
    // Exercise the real evidence worker directly. The public entitlement gate
    // stays intact and this regression never needs a Pro token.
    let mut child = Command::new(env!("CARGO_BIN_EXE_anymd"))
        .arg("__cite-check-worker")
        .arg(&request)
        .arg("--supervised=10000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = String::new();
    // Keep the supervision pipe open until the worker exits.
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(
        child.wait().unwrap().success(),
        "cite-check worker: {output}"
    );
    let native: Value = serde_json::from_str(&output).unwrap();
    let citation = &native["results"][0];
    let location = &citation["locations"][0];
    assert_eq!(citation["verdict"], "verified_exact");
    assert_eq!(citation["evidence_layer"]["kind"], "native_text");
    assert_eq!(citation["locations"].as_array().unwrap().len(), 1);
    assert_eq!(location["boxes"].as_array().unwrap().len(), 1);
    assert_eq!(proof["page"], citation["page"]);
    assert_eq!(proof["quote"], location["observed_text"]);
    assert_eq!(proof["sha256"], native["source_hash"]);
    assert_eq!(proof["verdict"], citation["verdict"]);
    assert_eq!(proof["geometry_level"], location["geometry_level"]);
    for edge in ["left", "bottom", "right", "top"] {
        let recorded = proof["bbox"][edge].as_f64().unwrap();
        let extracted = location["boxes"][0][edge].as_f64().unwrap();
        assert!(
            (recorded - extracted).abs() < 0.001,
            "{edge}: {recorded} != {extracted}"
        );
    }
}
