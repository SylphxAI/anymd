//! Route a conversion to its format owner and enforce the shared output limit.

use std::path::Path;

use crate::{csv, docx, encoding, epub, html, image, limits, pptx, video, xlsx};
use crate::{ConvertError, Converted, Format, Options, Section, SourceAccess};

#[derive(Debug)]
pub(super) struct UnrestrictedSourceAccess;

impl SourceAccess for UnrestrictedSourceAccess {
    fn admit_path(&self, path: &Path) -> Result<std::path::PathBuf, String> {
        Ok(path.to_path_buf())
    }
}

/// Convert a non-PDF input. PDF is handled by the PDF engine, not this crate.
pub fn convert(format: Format, bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    convert_with_source_access(format, bytes, options, &UnrestrictedSourceAccess)
}

/// Convert with the caller's admission policy for additional local source files.
/// A refused auxiliary file is skipped without reading its content.
pub fn convert_with_source_access(
    format: Format,
    bytes: &[u8],
    options: &Options,
    source_access: &dyn SourceAccess,
) -> Result<Converted, ConvertError> {
    let converted = match format {
        Format::Docx => docx::convert(bytes, options),
        Format::Pptx => pptx::convert(bytes, options),
        Format::Xlsx => xlsx::convert(bytes, options),
        Format::Csv => csv::convert(bytes, b',', options),
        Format::Tsv => csv::convert(bytes, b'\t', options),
        Format::Epub => epub::convert(bytes, options),
        Format::Html => html::convert(bytes, options),
        Format::Image => image::convert(bytes, options),
        Format::Video => video::convert_with_source_access(bytes, options, source_access),
        Format::Text => Ok(Converted {
            outline: Vec::new(),
            format: "text".into(),
            title: None,
            sections: vec![Section {
                label: "document".into(),
                markdown: encoding::text(bytes, "text")?.into_owned(),
            }],
            metadata: Vec::new(),
        }),
        Format::Subtitles => Ok(Converted {
            outline: Vec::new(),
            format: "subtitles".into(),
            title: None,
            sections: vec![Section {
                label: "subtitles".into(),
                markdown: video::subtitles_to_markdown(&String::from_utf8_lossy(bytes)),
            }],
            metadata: Vec::new(),
        }),
        Format::Pdf => Err(ConvertError::Unsupported(
            "PDF is converted by the PDF engine, not anymd-formats".into(),
        )),
    }?;
    let mut limits = limits::Limits::default();
    for section in &converted.sections {
        limits.output(&section.markdown)?;
    }
    Ok(converted)
}
