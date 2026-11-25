use std::path::{Path, PathBuf};

use anymd_formats::{ConvertError, Format};

use super::OpenOptions;

/// A source spec is a URL when it starts with http:// or https://.
pub fn is_url(spec: &str) -> bool {
    let lower = spec.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub(super) fn read_head(path: &Path, len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut head = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        let read = file.read(&mut head[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    head.truncate(filled);
    Ok(head)
}

pub(super) fn convert_other(
    format: Format,
    bytes: &[u8],
    base_url: Option<String>,
    path: Option<PathBuf>,
    options: &OpenOptions,
) -> Result<anymd_formats::Converted, ConvertError> {
    let ocr = match options.ocr {
        Some(value) => value,
        None => format == Format::Image && anymd_formats::image::ocr_available(),
    };
    let vlm = format == Format::Image
        && options.ocr != Some(false)
        && crate::ocr_vlm::requested(options.ocr_engine.unwrap_or_default());
    let mut converted = anymd_formats::convert(
        format,
        bytes,
        &anymd_formats::Options {
            base_url,
            ocr: ocr && !vlm,
            transcript: options.transcript,
            download_asr_model: options.download_asr_model,
            path,
            images: options.images.clone(),
            revisions: options.revisions,
        },
    )?;
    if vlm {
        let page = crate::ocr_vlm::recognize(bytes).map_err(ConvertError::Unsupported)?;
        if let Some(section) = converted.sections.first_mut() {
            if let Some(hint) = section
                .markdown
                .find("_Image text is not extracted by default")
            {
                section.markdown.truncate(hint);
            }
            section
                .markdown
                .push_str(&format!("\n\n## Text (OCR)\n\n{}", page.markdown()));
        }
    }
    Ok(converted)
}
