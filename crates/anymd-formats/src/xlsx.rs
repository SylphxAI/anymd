//! XLSX/XLS/XLSB/ODS → Markdown: one table per sheet via calamine.

mod input;
mod render;
mod workbook;

#[cfg(test)]
mod tests;

use crate::limits::Limits;
use crate::{ConvertError, Converted, Options};

pub fn convert(bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
    workbook::convert_with_limits(bytes, Limits::default())
}
