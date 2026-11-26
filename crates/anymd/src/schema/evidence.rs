use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{
    validate_f64_range, validate_source_count, validate_u32_min, validate_u32_range,
    validate_u64_range, PageSpecifier, PdfSource,
};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RegionBoundingBox {
    #[schemars(range(min = 0.0))]
    pub left: f64,
    #[schemars(range(min = 0.0))]
    pub bottom: f64,
    #[schemars(range(min = 0.0))]
    pub right: f64,
    #[schemars(range(min = 0.0))]
    pub top: f64,
}

impl RegionBoundingBox {
    fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("left", self.left),
            ("bottom", self.bottom),
            ("right", self.right),
            ("top", self.top),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!(
                    "sources[].regions[].bounding_box.{name} must be at least 0."
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfEvidenceRegion {
    pub id: Option<String>,
    #[schemars(range(min = 1))]
    pub page: u32,
    pub bounding_box: RegionBoundingBox,
    #[schemars(range(min = 0.0, max = 200.0))]
    pub padding: Option<f64>,
}

impl PdfEvidenceRegion {
    fn validate(&self) -> Result<(), String> {
        validate_u32_min("sources[].regions[].page", Some(self.page), 1)?;
        self.bounding_box.validate()?;
        if let Some(padding) = self.padding {
            if !padding.is_finite() || !(0.0..=200.0).contains(&padding) {
                return Err("sources[].regions[].padding must be between 0 and 200.".into());
            }
        }
        Ok(())
    }
}

// Same provider-compat policy as PdfSource (#562): no oneOf/not in tools/list;
// exclusive path|url enforced at runtime by validate()/as_pdf_source().
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfEvidenceSource {
    #[schemars(
        length(min = 1),
        description = "Path to a local source (PDF for PDF operations; media for video operations). Provide exactly one of path or url (not both)."
    )]
    pub path: Option<String>,
    #[schemars(
        length(min = 1),
        description = "Explicit source URL. Provide exactly one of path or url (not both)."
    )]
    pub url: Option<String>,
    pub pages: Option<PageSpecifier>,
    pub regions: Option<Vec<PdfEvidenceRegion>>,
}

impl PdfEvidenceSource {
    pub fn validate(&self) -> Result<(), String> {
        let source = self.as_pdf_source();
        source.validate()?;
        if let Some(regions) = &self.regions {
            for region in regions {
                region.validate()?;
            }
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.path
            .clone()
            .or_else(|| self.url.clone())
            .unwrap_or_else(|| "unknown".into())
    }

    pub fn as_pdf_source(&self) -> PdfSource {
        PdfSource {
            path: self.path.clone(),
            url: self.url.clone(),
            pages: self.pages.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PdfEvidenceOperation {
    Inspect,
    RenderPage,
    ExtractRegions,
    OcrPages,
    AnalyzeRegions,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfEvidenceArgs {
    pub operation: PdfEvidenceOperation,
    pub sources: Vec<PdfEvidenceSource>,
    #[schemars(range(min = 1, max = 20))]
    pub sample_pages: Option<u32>,
    pub include_metadata: Option<bool>,
    #[schemars(range(min = 0.25, max = 4.0))]
    pub scale: Option<f64>,
    #[schemars(range(min = 1, max = 20))]
    pub max_pages: Option<u32>,
    #[schemars(range(min = 1, max = 100))]
    pub max_regions: Option<u32>,
    #[schemars(range(min = 10_000, max = 64_000_000))]
    pub max_pixels_per_page: Option<u64>,
    pub include_image: Option<bool>,
    #[schemars(range(min = 1_000, max = 300_000))]
    pub timeout_ms: Option<u32>,
    #[schemars(range(min = 1_000, max = 1_000_000))]
    pub max_output_chars: Option<u32>,
    pub languages: Option<Vec<String>>,
    #[schemars(
        description = "Local OCR engine for ocr_pages: auto, vlm, or tesseract. Omit to use the configured command provider."
    )]
    pub ocr: Option<crate::ocr_vlm::OcrEngine>,
}

impl PdfEvidenceArgs {
    pub fn validate(&self) -> Result<(), String> {
        validate_source_count(&self.sources)?;
        for source in &self.sources {
            source.validate()?;
        }
        validate_u32_range("sample_pages", self.sample_pages, 1, 20)?;
        validate_f64_range("scale", self.scale, 0.25, 4.0)?;
        validate_u32_range("max_pages", self.max_pages, 1, 20)?;
        validate_u32_range("max_regions", self.max_regions, 1, 100)?;
        validate_u64_range(
            "max_pixels_per_page",
            self.max_pixels_per_page,
            10_000,
            64_000_000,
        )?;
        validate_u32_range("timeout_ms", self.timeout_ms, 1_000, 300_000)?;
        validate_u32_range("max_output_chars", self.max_output_chars, 1_000, 1_000_000)
    }
}
