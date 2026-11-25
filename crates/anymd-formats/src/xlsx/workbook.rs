use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;

use calamine::Reader;

use super::{input::AccountedInput, render};
use crate::limits::Limits;
use crate::ooxml::{self, Package};
use crate::{ConvertError, Converted, Section};

pub(super) fn convert_with_limits(
    bytes: &[u8],
    mut limits: Limits,
) -> Result<Converted, ConvertError> {
    let mut entries = Vec::new();
    let (title, metadata) = if zip::ZipArchive::new(Cursor::new(bytes)).is_ok() {
        ooxml::guard_zip(bytes)?;
        let mut package = Package::open(bytes, "spreadsheet")?;
        package.limits = limits;
        entries = package.preflight()?;
        let properties = ooxml::core_properties(&mut package);
        package.limits.check()?;
        limits = package.limits;
        properties
    } else {
        (None, Vec::new())
    };
    let limits = Rc::new(RefCell::new(limits));
    let input = AccountedInput::new(bytes, entries, Rc::clone(&limits));
    // calamine is third-party parsing of hostile input: contain any panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sheets(input, &limits)));
    // Optional parser reads and format probing may swallow I/O errors. A limit
    // remains sticky across every cloned input and wins over parser diagnostics.
    limits.borrow().check()?;
    let sections =
        result.map_err(|_| ooxml::invalid("spreadsheet parser failed on malformed input"))??;
    Ok(Converted {
        outline: Vec::new(),
        format: "xlsx".into(),
        title,
        sections,
        metadata,
    })
}

fn sheets(
    input: AccountedInput<'_>,
    limits: &Rc<RefCell<Limits>>,
) -> Result<Vec<Section>, ConvertError> {
    let mut workbook = calamine::open_workbook_auto_from_rs(input)
        .map_err(|e| ooxml::invalid(format!("not a readable spreadsheet: {e}")))?;
    limits.borrow().check()?;
    let mut sections = Vec::new();
    for name in workbook.sheet_names() {
        let range = workbook.worksheet_range(&name);
        limits.borrow().check()?;
        // Chart sheets and dialog sheets have no cell grid.
        let markdown = render::sheet(&name, range.ok())?;
        limits.borrow_mut().output(&markdown)?;
        sections.push(Section {
            label: format!("sheet {name}"),
            markdown,
        });
    }
    Ok(sections)
}
