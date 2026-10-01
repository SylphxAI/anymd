//! PaddlePaddle HPD-Parsing document VLM.
//!
//! HPD-Parsing combines an InternViT encoder and Qwen3 decoder with
//! hierarchical `<FORK>`/`<CHILD>` generation. Child branches reuse the
//! parent's KV prefix, while the optional P-MTP head drafts and greedily
//! verifies several future tokens per target-model step.

mod config;
mod model;
mod processing;
mod vision;

pub use config::{HpdParsingConfig, HpdVisionConfig};
pub use model::{
    DEFAULT_MAX_NEW_TOKENS, DEFAULT_PROMPT, DEFAULT_SPECULATIVE_TOKENS, HpdGenerationConfig,
    HpdOutput, HpdParsing, HpdRuntimeStats,
};
