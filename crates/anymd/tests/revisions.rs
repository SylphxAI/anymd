//! `revisions: markup | accept | reject` on `read` and the CLI, for Word
//! tracked changes. The fixtures are in `anymd-formats` (see its
//! `tests/critic_markup.rs`).

use std::path::Path;

use anymd::lean::{read_text, ReadRender};
use anymd::schema::ReadArgs;
use anymd::source_access::SourceAccessPolicy;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../anymd-formats/tests/fixtures/critic")
        .join(name)
        .display()
        .to_string()
}

fn args(name: &str, revisions: Option<&str>) -> ReadArgs {
    ReadArgs {
        source: fixture(name),
        pages: None,
        max_tokens: None,
        cursor: None,
        ocr: Some(anymd::ocr_vlm::OcrSelection::Enabled(false)),
        transcript: None,
        download_asr_model: None,
        images: Some("none".into()),
        revisions: revisions.map(str::to_string),
        node: None,
    }
}

fn read(name: &str, revisions: Option<&str>) -> String {
    let render = ReadRender {
        front_matter: false,
        unlimited: true,
    };
    read_text(
        &args(name, revisions),
        &SourceAccessPolicy::unrestricted(),
        &render,
    )
    .unwrap()
    .0
}

#[test]
fn each_choice_is_read_and_cached_on_its_own() {
    // Read the same file under every choice twice, in an order where a cache
    // keyed only on the file would hand one choice's text to another.
    for _ in 0..2 {
        let markup = read("tracked-changes.docx", None);
        assert!(
            markup.contains("{~~fonts~>font-styles~~}"),
            "markup: {markup}"
        );
        assert_eq!(read("tracked-changes.docx", Some("markup")), markup);

        let accepted = read("tracked-changes.docx", Some("accept"));
        assert!(
            accepted.contains("I really love font-styles."),
            "{accepted}"
        );
        assert!(!accepted.contains('{'), "accept: {accepted}");

        let rejected = read("tracked-changes.docx", Some("reject"));
        assert!(rejected.contains("I really love fonts."), "{rejected}");
        assert!(!rejected.contains('{'), "reject: {rejected}");
    }
}

#[test]
fn a_document_without_tracked_changes_reads_the_same_under_every_choice() {
    let markup = read("no-tracked-changes.docx", None);
    assert!(markup.contains("a {++ b ++} ~> c --}"), "{markup}");
    for revisions in ["markup", "accept", "reject"] {
        assert_eq!(read("no-tracked-changes.docx", Some(revisions)), markup);
    }
}

#[test]
fn an_unknown_choice_is_refused() {
    let error = args("tracked-changes.docx", Some("all"))
        .validate()
        .unwrap_err();
    assert!(error.contains("revisions"), "{error}");
    assert!(args("tracked-changes.docx", Some("accept"))
        .validate()
        .is_ok());
}
