//! The request budget every provider-backed tool shares: at most a fixed
//! number of provider requests run at once, and each request may spend only so
//! many provider bytes and output characters before it fails closed.
//!
//! `read_pdf`'s OCR evidence and `pdf_evidence`'s region analysis add
//! per-source render budgets of their own; this is the process-wide concurrency
//! slot and the per-request provider allowance both of them enforce.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Provider requests one process may run at the same time.
pub(crate) const MAX_CONCURRENT_PROVIDER_REQUESTS: usize = 2;
/// The longest one provider request may run.
pub(crate) const MAX_REQUEST_TIMEOUT_MS: u64 = 600_000;
/// Provider output one request may read.
pub(crate) const MAX_REQUEST_OUTPUT_CHARS: usize = 2_000_000;
/// Provider bytes one request may hand to the provider.
pub(crate) const MAX_REQUEST_PROVIDER_BYTES: usize = 16 * 1024 * 1024;

static ACTIVE_REQUESTS: AtomicUsize = AtomicUsize::new(0);

/// The number of provider requests running now (tests).
#[cfg(test)]
pub(crate) fn active_requests() -> usize {
    ACTIVE_REQUESTS.load(Ordering::Acquire)
}

/// One occupied concurrency slot; released when dropped.
#[derive(Debug)]
pub(crate) struct ProviderRequestPermit;

impl ProviderRequestPermit {
    /// Takes a slot, or refuses with `refusal` when both are busy.
    pub(crate) fn acquire(refusal: impl FnOnce() -> String) -> Result<Self, String> {
        ACTIVE_REQUESTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_CONCURRENT_PROVIDER_REQUESTS).then_some(active + 1)
            })
            .map(|_| Self)
            .map_err(|_| refusal())
    }
}

impl Drop for ProviderRequestPermit {
    fn drop(&mut self) {
        ACTIVE_REQUESTS.fetch_sub(1, Ordering::Release);
    }
}

/// The provider byte and output character allowance of one request. Once a
/// limit is crossed the refusal is sticky: every later charge fails with the
/// same message, so a caller cannot recover by trying again.
#[derive(Default)]
pub(crate) struct ProviderRequestBudget {
    output_chars: usize,
    provider_bytes: usize,
    exhausted: Option<String>,
}

impl ProviderRequestBudget {
    pub(crate) fn ensure_available(&self) -> Result<(), String> {
        self.exhausted.clone().map_or(Ok(()), Err)
    }

    pub(crate) fn exhaust(&mut self, message: String) -> Result<(), String> {
        self.exhausted = Some(message.clone());
        Err(message)
    }

    /// Charges one provider answer, with the tool's own words in each refusal.
    pub(crate) fn charge(
        &mut self,
        provider_bytes: usize,
        output_chars: usize,
        refusal: impl Fn(&str) -> String,
    ) -> Result<(), String> {
        self.ensure_available()?;
        let Some(next_provider_bytes) = self.provider_bytes.checked_add(provider_bytes) else {
            return self.exhaust(refusal("provider byte count overflow"));
        };
        self.provider_bytes = next_provider_bytes;
        if self.provider_bytes > MAX_REQUEST_PROVIDER_BYTES {
            return self.exhaust(refusal("provider output limit"));
        }
        let Some(next_output_chars) = self.output_chars.checked_add(output_chars) else {
            return self.exhaust(refusal("output character count overflow"));
        };
        self.output_chars = next_output_chars;
        if self.output_chars > MAX_REQUEST_OUTPUT_CHARS {
            return self.exhaust(refusal("output limit"));
        }
        Ok(())
    }

    /// A provider that failed still spends the bytes it read.
    pub(crate) fn charge_failed_provider(
        &mut self,
        provider_bytes: usize,
        refusal: impl Fn(&str) -> String,
    ) -> Result<(), String> {
        self.charge(provider_bytes, 0, refusal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(what: &str) -> String {
        format!("Request exceeds test {what}.")
    }

    #[test]
    fn a_request_budget_is_sticky_and_words_its_refusal() {
        let mut budget = ProviderRequestBudget::default();
        budget
            .charge(
                MAX_REQUEST_PROVIDER_BYTES - 1,
                MAX_REQUEST_OUTPUT_CHARS - 1,
                refusal,
            )
            .expect("within budget");
        let error = budget.charge(2, 0, refusal).expect_err("byte overflow");
        assert!(error.contains("provider output limit"), "{error}");
        assert_eq!(budget.ensure_available().expect_err("sticky"), error);

        let mut budget = ProviderRequestBudget::default();
        let error = budget
            .charge(0, MAX_REQUEST_OUTPUT_CHARS + 1, refusal)
            .expect_err("character overflow");
        assert!(error.contains("output limit"), "{error}");
    }

    #[test]
    fn the_concurrency_slot_is_process_wide_and_released_on_drop() {
        let refusal = || "concurrency limit reached".to_string();
        let first = ProviderRequestPermit::acquire(refusal).expect("first slot");
        let second = ProviderRequestPermit::acquire(refusal).expect("second slot");
        let refused = ProviderRequestPermit::acquire(refusal).expect_err("third refused");
        assert!(refused.contains("concurrency limit"), "{refused}");
        drop(first);
        let replacement = ProviderRequestPermit::acquire(refusal).expect("released slot reuses");
        drop(second);
        drop(replacement);
        assert_eq!(active_requests(), 0);
    }
}
