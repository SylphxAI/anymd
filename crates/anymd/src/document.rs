//! One entry point for every input anymd reads: a local path or an http(s)
//! URL becomes an [`Opened`] document whose citable units (pages, slides,
//! sheets, chapters) convert to Markdown on demand.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anymd_core::document::scanned_pdf_pages;
use anymd_core::markdown_layout;
pub use anymd_core::outline::Unit;
use anymd_core::outline::{title_from_units, DocumentInfo, Outline};
use anymd_formats::images::ImageStore;
use anymd_formats::{ConvertError, Format};

mod cache;
mod images;
mod ocr;
mod open;
#[cfg(test)]
mod tests;

use cache::{cache_get, cache_key, cache_put, referenced_images, CachedDoc};
use images::place_image;
pub use images::{attach_structure_images, pdf_image_listing};

#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// None = OCR image-only pages and images when `tesseract` is installed.
    pub ocr: Option<bool>,
    pub ocr_engine: Option<crate::ocr_vlm::OcrEngine>,
    pub transcript: bool,
    /// With `transcript`: fetch the Qwen3-ASR model when none is installed.
    pub download_asr_model: bool,
    /// Export images embedded in PDF, DOCX, PPTX and EPUB files into this
    /// store and mark them in the Markdown. `None` leaves them out.
    pub images: Option<ImageStore>,
    /// How Word tracked changes and comments come out.
    pub revisions: anymd_formats::Revisions,
}

enum Body {
    Pdf {
        doc: Box<markdown_layout::PdfDocument>,
        bytes: Option<Arc<Vec<u8>>>,
        path: Option<PathBuf>,
        title: Option<String>,
        /// Pictures that repeat across pages (logos), found on first use.
        repeated: OnceLock<HashSet<u64>>,
    },
    Units(Arc<Vec<Unit>>),
}

pub struct Opened {
    /// What the caller passed (path or URL).
    pub label: String,
    pub format: &'static str,
    pub title: Option<String>,
    /// Singular unit noun for headers: "page", "slide", "sheet", "chapter", "section".
    pub unit_noun: &'static str,
    pub total: u32,
    pub metadata: Vec<(String, String)>,
    options: OpenOptions,
    body: Body,
    native_outline: Vec<(usize, String, Option<u32>)>,
    ocr_request: Mutex<Option<crate::ocr_vlm::OcrRequest>>,
    /// Pages an OCR engine actually ran on (whether or not it found text).
    ocr_ran: Mutex<Vec<u32>>,
}

/// Evidence relevant to the CLI, derived from the opened source rather than
/// its filename or the rendered output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceKind {
    MultiPagePdf,
    Video,
}

impl Opened {
    pub(crate) fn evidence_kind(&self, units_processed: usize) -> Option<EvidenceKind> {
        match self.format {
            "pdf" if units_processed > 1 => Some(EvidenceKind::MultiPagePdf),
            "video"
                if self
                    .metadata
                    .iter()
                    .any(|(key, value)| key == "kind" && value == "video") =>
            {
                Some(EvidenceKind::Video)
            }
            _ => None,
        }
    }
}

/// A source spec is a URL when it starts with http:// or https://.
pub fn is_url(spec: &str) -> bool {
    let lower = spec.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

impl Opened {
    /// True when every unit converts in one pass (not a lazily paged PDF).
    pub fn is_paged(&self) -> bool {
        matches!(self.body, Body::Pdf { .. })
    }

    /// Bookmarks (PDF only): (depth, title, page).
    pub fn outline(&self) -> Vec<(usize, String, Option<u32>)> {
        match &self.body {
            Body::Pdf { doc, .. } => markdown_layout::outline(doc),
            Body::Units(_) => self.native_outline.clone(),
        }
    }

    pub(crate) fn info(&self) -> DocumentInfo<'_> {
        DocumentInfo {
            source: &self.label,
            format: self.format,
            title: self.title.as_deref(),
            unit_noun: self.unit_noun,
            total: self.total,
            paged: self.is_paged(),
        }
    }

    pub fn build_outline(&self) -> Result<Outline, String> {
        let units = self.all_units()?;
        if units.is_empty() {
            return Err("document has no readable units".into());
        }
        Ok(self.outline_from_units(&units))
    }

    pub fn outline_from_units(&self, units: &[Unit]) -> Outline {
        Outline::from_units(self.info(), units, &self.outline())
    }

    /// Convert the requested units (1-based numbers, ascending).
    pub fn units(&self, numbers: &[u32]) -> Result<Vec<Unit>, String> {
        match &self.body {
            Body::Units(units) => Ok(numbers
                .iter()
                .filter_map(|n| units.get(*n as usize - 1).cloned())
                .collect()),
            Body::Pdf {
                doc,
                bytes,
                path,
                title,
                repeated,
            } => {
                let converted = match &self.options.images {
                    Some(store) => {
                        let repeated =
                            repeated.get_or_init(|| markdown_layout::repeated_images(doc));
                        let place = |image: &markdown_layout::EncodedImage,
                                     caption: Option<&str>,
                                     page: u32| {
                            place_image(store, image, caption, page)
                        };
                        let options = markdown_layout::ImageOptions {
                            repeated,
                            place: &place,
                        };
                        markdown_layout::pdf_to_markdown_with_images(
                            doc,
                            Some(numbers),
                            Some(&options),
                        )
                    }
                    None => markdown_layout::pdf_to_markdown(doc, Some(numbers)),
                }
                .map_err(|error| error.message)?;
                let _ = title;
                let mut units: Vec<Unit> = converted.pages.into_iter().map(Unit::from).collect();
                self.ocr_sparse_pages(&mut units, doc, bytes.as_deref(), path.as_deref())?;
                Ok(units)
            }
        }
    }

    /// PDF pages among `units` with no text (after any OCR) that are covered by
    /// images: scans that were not read, as opposed to genuinely empty pages.
    pub fn scanned_pages(&self, units: &[Unit]) -> Vec<u32> {
        let Body::Pdf { doc, .. } = &self.body else {
            return Vec::new();
        };
        scanned_pdf_pages(doc, units)
    }

    /// Title from page 1 when the PDF has no usable /Title.
    pub fn title_from_units(&mut self, units: &[Unit]) {
        if self.title.is_none() {
            self.title = title_from_units(units);
        }
    }

    /// Every unit (PDFs convert in parallel chunks). Cached for local files.
    pub fn all_units(&self) -> Result<Arc<Vec<Unit>>, String> {
        match &self.body {
            Body::Units(units) => Ok(units.clone()),
            Body::Pdf { path, .. } => {
                let ocr_on =
                    self.options.ocr != Some(false) && anymd_formats::image::ocr_available();
                let key = path
                    .as_deref()
                    .and_then(|p| cache_key(p, ocr_on, &self.options));
                if let Some(cached) = key.as_ref().and_then(cache_get) {
                    return Ok(cached.units.clone());
                }
                let numbers: Vec<u32> = (1..=self.total).collect();
                let units = Arc::new(self.units(&numbers)?);
                if let Some(key) = key {
                    cache_put(
                        key,
                        Arc::new(CachedDoc {
                            format: "pdf",
                            native_outline: Vec::new(),
                            title: self.title.clone(),
                            metadata: Vec::new(),
                            units: units.clone(),
                            bytes: units.iter().map(|u| u.markdown.len()).sum(),
                            images: referenced_images(&units, self.options.images.as_ref()),
                        }),
                    );
                }
                Ok(units)
            }
        }
    }
}

fn read_head(path: &Path, len: usize) -> std::io::Result<Vec<u8>> {
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

fn convert_other(
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

/// File extensions a directory walk picks up for search.
pub fn searchable_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdf"
            | "docx"
            | "pptx"
            | "xlsx"
            | "xls"
            | "xlsm"
            | "ods"
            | "csv"
            | "tsv"
            | "epub"
            | "html"
            | "htm"
            | "xhtml"
            | "md"
            | "markdown"
            | "txt"
            | "rst"
            | "srt"
            | "vtt"
    )
}

/// Readable file extensions (search set plus media).
pub fn readable_extension(path: &Path) -> bool {
    searchable_extension(path)
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "webp"
                        | "tif"
                        | "tiff"
                        | "bmp"
                        | "mp4"
                        | "mov"
                        | "mkv"
                        | "webm"
                        | "mp3"
                        | "wav"
                        | "m4a"
                        | "flac"
                        | "ogg"
                )
            })
}
