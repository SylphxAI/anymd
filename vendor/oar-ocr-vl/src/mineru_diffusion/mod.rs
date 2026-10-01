//! MinerU-Diffusion-V1 document OCR model.
//!
//! A Qwen2-VL vision tower (reusing [`crate::mineru`]'s backbone) feeds a
//! `patch_merger2x` abstractor and an SDAR block-diffusion text decoder. Unlike
//! the autoregressive MinerU2.5, text is produced by *parallel diffusion
//! decoding*: each fixed-size block of output tokens is denoised from
//! `<|MASK|>` over several steps, committing the most confident positions first.
//!
//! See `MinerU-Diffusion: Rethinking Document OCR as Inverse Rendering via
//! Diffusion Decoding` (arXiv:2603.22458).

mod config;
mod model;
mod projector;
pub(crate) mod text;

pub use config::{MinerUDiffusionConfig, SdarConfig};
pub use model::{DEFAULT_PROMPT, DiffusionGenerationConfig, MinerUDiffusion};
