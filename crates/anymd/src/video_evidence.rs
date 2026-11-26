//! Video evidence orchestration over already-admitted media. The shared request
//! owner supplies source identity, one deadline, budget, OCR permit and the
//! existing local-command caption/cache adapter. No parallel provider or cache.

use std::time::Instant;

use anymd_formats::video::timeline::DecodedFrame;

mod enrich;
mod render;
mod types;

pub use enrich::{enrich, video_timeline};
pub use render::{render_frames, sections};
pub use types::{
    FrameDescription, RasterObservation, RasterRegion, SampledDescription, SampledOcr,
    VideoEvidence,
};

/// Integration seam to the existing request owners, not a provider runner.
/// `caption_cached` must reuse the local-command region adapter only, never its
/// HTTP branch or doc-VLM. Successful descriptions are keyed by source/sidecar
/// hashes, resolved policy/options, frame hash and provider/model revision.
/// Failed/incomplete descriptions must not be cached as successful completion.
pub trait FrameContext {
    /// Checks the caller's shared deadline/exhaustion state before any work.
    fn ensure_available(&self, deadline: Instant) -> Result<(), String>;
    /// Charges the EXISTING request aggregate (40 MP and 32 MiB); no new permit.
    fn charge_frame(&mut self, pixels: u64, encoded_bytes: u64) -> Result<(), String>;
    /// A requested OCR operation without an installed/selected adapter is None.
    /// The adapter borrows the caller's existing OCR permit and raster entrypoint.
    fn ocr(
        &mut self,
        frame: &DecodedFrame,
        deadline: Instant,
    ) -> Result<Option<RasterObservation>, String>;
    /// True only for an explicitly configured local command vision adapter.
    fn local_caption_available(&self) -> bool;
    /// At most one successful cached caption per representative frame. Outline
    /// and render_frame never invoke this method; repeated reads reuse manifest.
    fn caption_cached(
        &mut self,
        source_hash: &str,
        scene_id: &str,
        frame: &DecodedFrame,
        deadline: Instant,
    ) -> Result<Option<FrameDescription>, String>;
}

#[cfg(test)]
pub(crate) mod tests;
