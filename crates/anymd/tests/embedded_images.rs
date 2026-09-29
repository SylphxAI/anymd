//! Images embedded in PDF, DOCX, PPTX and EPUB files are exported to a
//! content-addressed cache and marked where they sit in the Markdown.
//!
//! Fixtures in `test/fixtures`: `figure-report.pdf` (a captioned figure on
//! page 1, a banner logo on all three pages, a 32 px icon on page 2),
//! `alt-text.docx`, `slides.pptx` (a logo on all three slides) and
//! `field-notes.epub` (an ornament at the end of all three chapters).

use std::path::{Path, PathBuf};

use anymd::document::{OpenOptions, Opened};
use anymd::schema::ReadArgs;
use anymd::source_access::SourceAccessPolicy;
use anymd_formats::images::ImageStore;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name)
        .display()
        .to_string()
}

/// Every unit of a fixture as Markdown, exporting images into `cache` (or
/// leaving them out when `cache` is `None`).
fn read(name: &str, cache: Option<&Path>) -> String {
    let options = OpenOptions {
        ocr: Some(false),
        images: cache.map(ImageStore::at),
        ..Default::default()
    };
    let opened = Opened::open(
        &fixture(name),
        &SourceAccessPolicy::unrestricted(),
        &options,
    )
    .unwrap_or_else(|error| panic!("{name}: {error}"));
    let numbers: Vec<u32> = (1..=opened.total).collect();
    opened
        .units(&numbers)
        .unwrap()
        .iter()
        .map(|unit| format!("<!-- {} -->\n{}", unit.label, unit.markdown))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn files(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

fn refs(markdown: &str) -> usize {
    markdown.matches("![").count()
}

fn at(markdown: &str, needle: &str) -> usize {
    markdown
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in:\n{markdown}"))
}

#[test]
fn pdf_figure_sits_in_reading_order_with_its_caption() {
    let cache = tempfile::tempdir().unwrap();
    let markdown = read("figure-report.pdf", Some(cache.path()));
    let figure = at(&markdown, "![Figure 1: Quarterly revenue by region](");
    assert!(at(&markdown, "Quarterly report") < figure, "{markdown}");
    assert!(
        figure < at(&markdown, "Revenue grew in every region"),
        "{markdown}"
    );
    assert!(
        markdown.contains("<!-- image: 240x150, page 1 -->"),
        "{markdown}"
    );
    // The path is absolute, inside the configured cache, and the file is there.
    let start = figure + markdown[figure..].find("](").unwrap() + 2;
    let end = start + markdown[start..].find(')').unwrap();
    let path = PathBuf::from(&markdown[start..end]);
    assert!(
        path.is_absolute() && path.starts_with(cache.path()),
        "{path:?}"
    );
    assert!(std::fs::read(&path).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn pdf_skips_the_repeated_logo_and_the_small_icon() {
    let cache = tempfile::tempdir().unwrap();
    let markdown = read("figure-report.pdf", Some(cache.path()));
    // The banner is on all three pages and the icon is 32 px: only the figure remains.
    assert_eq!(refs(&markdown), 1, "{markdown}");
    assert_eq!(files(cache.path()).len(), 1);
}

#[test]
fn reading_again_reuses_the_cached_file() {
    let cache = tempfile::tempdir().unwrap();
    let first = read("figure-report.pdf", Some(cache.path()));
    let written = files(cache.path());
    assert_eq!(written.len(), 1);
    let second = read("figure-report.pdf", Some(cache.path()));
    assert_eq!(first, second);
    assert_eq!(files(cache.path()), written);
}

#[test]
fn a_deleted_cached_image_is_exported_again_on_reread() {
    let cache = tempfile::tempdir().unwrap();
    // A DOCX is kept in the in-process document cache, so this re-read is a
    // cache hit whose image file is gone.
    let first = read("alt-text.docx", Some(cache.path()));
    let written = files(cache.path());
    assert_eq!(written.len(), 2);
    std::fs::remove_file(&written[0]).unwrap();
    let second = read("alt-text.docx", Some(cache.path()));
    assert_eq!(first, second);
    assert!(written.iter().all(|path| path.is_file()));
}

#[test]
fn images_none_leaves_images_out_and_writes_nothing() {
    let cache = tempfile::tempdir().unwrap();
    let missing = cache.path().join("never-created");
    for name in [
        "figure-report.pdf",
        "alt-text.docx",
        "slides.pptx",
        "field-notes.epub",
    ] {
        let markdown = read(name, None);
        assert!(!markdown.contains("<!-- image:"), "{name}: {markdown}");
        assert!(!markdown.contains(&missing.display().to_string()));
    }
    assert!(files(&missing).is_empty());
    let mut args: ReadArgs =
        serde_json::from_value(serde_json::json!({"source": "a.pdf"})).unwrap();
    assert!(args.wants_images());
    args.images = Some("none".into());
    assert!(!args.wants_images() && args.validate().is_ok());
    args.images = Some("all".into());
    assert!(args.validate().is_err());
}

#[test]
fn docx_uses_alt_text_as_caption_and_skips_small_icons() {
    let cache = tempfile::tempdir().unwrap();
    let markdown = read("alt-text.docx", Some(cache.path()));
    let chart = at(&markdown, "![A bar chart of sales by quarter](");
    assert!(at(&markdown, "Sales overview") < chart, "{markdown}");
    assert!(chart < at(&markdown, "Between the pictures."), "{markdown}");
    assert!(markdown.contains("<!-- image: 120x80 -->"), "{markdown}");
    // A picture without alt text is captioned "image"; the 16 px icon is gone.
    assert!(at(&markdown, "Between the pictures.") < at(&markdown, "![image]("));
    assert!(!markdown.contains("Tiny icon"), "{markdown}");
    assert_eq!(refs(&markdown), 2, "{markdown}");
    assert_eq!(files(cache.path()).len(), 2);
}

#[test]
fn pptx_pictures_follow_slide_order_and_the_shared_logo_is_skipped() {
    let cache = tempfile::tempdir().unwrap();
    let markdown = read("slides.pptx", Some(cache.path()));
    let org = at(&markdown, "![Org chart](");
    let pipeline = at(&markdown, "![Pipeline diagram](");
    assert!(at(&markdown, "<!-- slide 1 -->") < org, "{markdown}");
    assert!(org < at(&markdown, "<!-- slide 2 -->"), "{markdown}");
    assert!(at(&markdown, "<!-- slide 2 -->") < pipeline, "{markdown}");
    assert!(
        markdown.contains("<!-- image: 100x100, slide 1 -->"),
        "{markdown}"
    );
    assert!(!markdown.contains("Company logo"), "{markdown}");
    assert_eq!(refs(&markdown), 2, "{markdown}");
    assert_eq!(files(cache.path()).len(), 2);
}

#[test]
fn epub_images_use_alt_text_and_repeated_ornaments_are_skipped() {
    let cache = tempfile::tempdir().unwrap();
    let markdown = read("field-notes.epub", Some(cache.path()));
    assert!(markdown.contains("![Map of the valley]("), "{markdown}");
    assert!(
        markdown.contains("<!-- image: 140x100, chapter 1 -->"),
        "{markdown}"
    );
    assert_eq!(refs(&markdown), 1, "{markdown}");
    assert_eq!(files(cache.path()).len(), 1);
}
