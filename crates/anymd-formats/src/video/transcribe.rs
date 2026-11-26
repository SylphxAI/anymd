//! Local ASR orchestration, word alignment, and transcript rendering.

#[cfg(any(feature = "native", test))]
mod align;
#[cfg(feature = "native")]
mod audio;
mod pipeline;
#[cfg(any(feature = "native", test))]
mod render;
#[cfg(any(feature = "native", test))]
mod window;

#[cfg(any(feature = "native", test))]
pub(super) use align::parse_alignment;
pub(super) use pipeline::transcribe;
#[cfg(any(feature = "native", test))]
pub(super) use render::render_transcript;
#[cfg(feature = "native")]
pub use window::transcript_window;
#[cfg(any(feature = "native", test))]
pub(super) use window::TranscriptCue;
