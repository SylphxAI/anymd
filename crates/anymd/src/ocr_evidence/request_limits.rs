use std::time::Instant;

use crate::provider_budget::{
    ProviderRequestBudget, ProviderRequestPermit, MAX_CONCURRENT_PROVIDER_REQUESTS,
    MAX_REQUEST_OUTPUT_CHARS, MAX_REQUEST_PROVIDER_BYTES, MAX_REQUEST_TIMEOUT_MS,
};
use crate::visual_evidence::RequestWorkBudget;

pub(super) const DEFAULT_MAX_OUTPUT_CHARS: usize = 200_000;
pub(super) const MAX_SOURCES_PER_REQUEST: usize = 32;

/// A refusal in the OCR tool's words, given what the budget refused.
pub(super) fn refusal(what: &str) -> String {
    match what {
        "provider output limit" => format!(
            "Request exceeds OCR provider output limit of {MAX_REQUEST_PROVIDER_BYTES} bytes."
        ),
        "output limit" => {
            format!("Request exceeds OCR output limit of {MAX_REQUEST_OUTPUT_CHARS} characters.")
        }
        other => format!("Request {other}."),
    }
}

/// What every OCR request refuses with once its time is up.
pub(crate) fn timeout_refusal() -> String {
    format!("Request exceeds OCR provider time limit of {MAX_REQUEST_TIMEOUT_MS} milliseconds.")
}

pub(super) type RequestOcrBudget = ProviderRequestBudget;
/// The OCR tool's concurrency slot: one shared process-wide provider budget.
pub(crate) type OcrRequestPermit = ProviderRequestPermit;

/// The one admission call every OCR-path caller uses.
pub(crate) fn acquire_ocr_permit() -> Result<OcrRequestPermit, String> {
    acquire_permit()
}

pub(super) fn acquire_permit() -> Result<OcrRequestPermit, String> {
    ProviderRequestPermit::acquire(|| {
        format!(
            "OCR provider concurrency limit of {MAX_CONCURRENT_PROVIDER_REQUESTS} active requests reached; retry later."
        )
    })
}

pub(super) fn admit_read_ocr_source(
    request_budget: &RequestOcrBudget,
    render_budget: &mut RequestWorkBudget,
    request_deadline: Instant,
) -> Result<(), String> {
    request_budget.ensure_available()?;
    render_budget.ensure_available()?;
    if Instant::now() >= request_deadline {
        return render_budget.exhaust(timeout_refusal());
    }
    Ok(())
}
