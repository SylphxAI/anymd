//! Optional command or HTTP provider analysis over bounded pure-Rust region crops.

mod analyze;
mod candidate_regions;
mod candidates;
mod caption;
mod normalize;
mod provider;
mod provider_config;
mod request_limits;
#[cfg(test)]
mod tests;

pub use analyze::analyze_regions;
pub(crate) use candidates::analyze_visual_candidates_for_sources;
pub(crate) use caption::{caption_raster, local_caption_identity};

use provider_config::provider_config;

#[cfg(test)]
use crate::provider_budget::MAX_REQUEST_PROVIDER_BYTES;
#[cfg(test)]
use request_limits::{refusal, RequestBudget};

/// True when a valid region-analysis provider is configured (command, HTTP, or preset).
pub(crate) fn command_provider_ready() -> bool {
    provider_config().is_ok()
}
