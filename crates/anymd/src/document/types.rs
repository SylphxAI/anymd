use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use anymd_core::markdown_layout;
use anymd_core::outline::{title_from_units, DocumentInfo, Outline};
use anymd_formats::images::ImageStore;

use super::Unit;

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

pub(super) enum Body {
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
    pub(super) options: OpenOptions,
    pub(super) body: Body,
    pub(super) native_outline: Vec<(usize, String, Option<u32>)>,
    pub(super) ocr_request: Mutex<Option<crate::ocr_vlm::OcrRequest>>,
    /// Pages an OCR engine actually ran on (whether or not it found text).
    pub(super) ocr_ran: Mutex<Vec<u32>>,
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

    /// Title from page 1 when the PDF has no usable /Title.
    pub fn title_from_units(&mut self, units: &[Unit]) {
        if self.title.is_none() {
            self.title = title_from_units(units);
        }
    }
}
