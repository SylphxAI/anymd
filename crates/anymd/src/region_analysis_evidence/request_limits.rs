//! Shared PDF region-analysis defaults and refusal messages.

use crate::provider_budget::{
    ProviderRequestBudget, ProviderRequestPermit, MAX_CONCURRENT_PROVIDER_REQUESTS,
    MAX_REQUEST_OUTPUT_CHARS, MAX_REQUEST_PROVIDER_BYTES, MAX_REQUEST_TIMEOUT_MS,
};

pub(super) const DEFAULT_TIMEOUT_MS: u64 = 60_000;
pub(super) const DEFAULT_MAX_OUTPUT_CHARS: usize = 200_000;
/// A refusal in the region-analysis tool's words, given what the budget refused.
pub(super) fn refusal(what: &str) -> String {
    match what {
        "provider output limit" => format!(
            "Request exceeds region analysis provider output limit of {MAX_REQUEST_PROVIDER_BYTES} bytes."
        ),
        "output limit" => format!(
            "Request exceeds region analysis output limit of {MAX_REQUEST_OUTPUT_CHARS} characters."
        ),
        other => format!("Request region analysis {other}."),
    }
}

/// What every region-analysis request refuses with once its time is up.
pub(super) fn timeout_refusal() -> String {
    format!(
        "Request exceeds region analysis provider time limit of {MAX_REQUEST_TIMEOUT_MS} milliseconds."
    )
}

pub(super) type RequestBudget = ProviderRequestBudget;

pub(super) fn acquire_permit() -> Result<ProviderRequestPermit, String> {
    ProviderRequestPermit::acquire(|| {
        format!(
            "Region analysis provider concurrency limit of {MAX_CONCURRENT_PROVIDER_REQUESTS} active requests reached; retry later."
        )
    })
}
