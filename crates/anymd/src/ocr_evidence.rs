//! Optional command-provider OCR over the bounded pure-Rust page renderer.

mod normalize;
mod pages;
mod provider;
mod raster;
mod read_ocr;
mod request_limits;
#[cfg(test)]
mod tests;

pub use pages::ocr_pages;
pub(crate) use raster::recognize_raster;
pub(crate) use read_ocr::{run_read_ocr, ReadOcrOptions};
pub(crate) use request_limits::{acquire_ocr_permit, timeout_refusal, OcrRequestPermit};

#[cfg(test)]
use self::{
    normalize::{normalize_output, restore_render_geometry},
    request_limits::{acquire_permit, admit_read_ocr_source, refusal, RequestOcrBudget},
};
#[cfg(test)]
use crate::{
    provider_budget::{MAX_REQUEST_OUTPUT_CHARS, MAX_REQUEST_PROVIDER_BYTES},
    visual_evidence::RequestWorkBudget,
};
#[cfg(test)]
use anymd_core::{OcrPage, SourceOcrOutcome};
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use std::time::{Duration, Instant};
