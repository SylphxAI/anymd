//! A long, heavily tagged PDF must not cost memory in proportion to the parts
//! of it that text layout never reads (annotations, link actions and the
//! structure tree). The allocator here counts live bytes, which, unlike the
//! process RSS, does not depend on the machine or on other tests.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use pdf_extract::{dictionary, Document, Object, Stream};

static LIVE: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size >= layout.size() {
            LIVE.fetch_add(new_size - layout.size(), Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Relaxed);
        }
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const PAGES: u32 = 400;
const LINKS_PER_PAGE: u32 = 40;
const ELEMENTS_PER_PAGE: u32 = 40;

/// `PAGES` pages of one line of text each, with `LINKS_PER_PAGE` link
/// annotations (each with its own action) and `ELEMENTS_PER_PAGE` structure
/// elements per page.
fn tagged_book() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let struct_root_id = doc.new_object_id();
    let mut kids = Vec::new();
    let mut elements = Vec::new();
    for number in 1..=PAGES {
        let content = format!("BT /F1 12 Tf 72 700 Td (page {number} of the book) Tj ET");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = doc.new_object_id();
        let mut annots = Vec::new();
        for link in 0..LINKS_PER_PAGE {
            let action = doc.add_object(dictionary! {
                "S" => "GoTo",
                "D" => vec![Object::Reference(page_id), "Fit".into()],
            });
            annots.push(Object::Reference(doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Link",
                "Rect" => vec![0.into(), (link as i64).into(), 10.into(), 10.into()],
                "A" => action,
            })));
        }
        for _ in 0..ELEMENTS_PER_PAGE {
            elements.push(Object::Reference(doc.add_object(dictionary! {
                "S" => "P", "P" => struct_root_id, "Pg" => page_id, "K" => 0,
            })));
        }
        doc.objects.insert(
            page_id,
            dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => resources_id, "Annots" => annots,
            }
            .into(),
        );
        kids.push(Object::Reference(page_id));
    }
    doc.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => PAGES as i64 }.into(),
    );
    doc.objects.insert(
        struct_root_id,
        dictionary! { "Type" => "StructTreeRoot", "K" => elements }.into(),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "StructTreeRoot" => struct_root_id,
    });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_tagged_book_is_held_without_its_annotations_and_structure_tree() {
    let bytes = tagged_book();

    let before = LIVE.load(Relaxed);
    let whole = Document::load_mem(&bytes).unwrap();
    let whole_held = LIVE.load(Relaxed) - before;
    drop(whole);

    let before = LIVE.load(Relaxed);
    let doc = anymd_pdf::load_document_bytes(&bytes).unwrap();
    let held = LIVE.load(Relaxed) - before;

    // Pages, one content stream each, the font and the page tree: about three
    // objects a page. Keeping the annotations, actions and structure elements
    // as well takes more than 40 times that.
    assert!(
        doc.objects.len() < PAGES as usize * 4,
        "{} objects held",
        doc.objects.len()
    );
    assert!(
        held * 3 < whole_held,
        "{held} bytes held for {PAGES} pages; the whole document takes {whole_held}"
    );

    // What layout reads is untouched.
    assert_eq!(anymd_pdf::page_count(&doc), PAGES);
    let converted = anymd_pdf::pdf_to_markdown(&doc, Some(&[1, 200, PAGES])).unwrap();
    for (page, number) in converted.pages.iter().zip([1, 200, PAGES]) {
        assert_eq!(page.markdown.trim(), format!("page {number} of the book"));
    }
}
