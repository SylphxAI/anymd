//! One local speech model for every language: Qwen3-ASR-1.7B Q8_0.
//! Weights are downloaded only when requested, size/SHA-256 verified and
//! atomically installed. Documents and audio never leave this machine.

mod download;
mod resolve;
mod status;

pub use download::download_model;
pub use resolve::{ensure_model, models_dir};
pub use status::{ffmpeg_hint, status_lines};

pub const MODEL_ENV: &str = "ANYMD_ASR_MODEL";
pub const CACHE_ENV: &str = "ANYMD_CACHE_DIR";
pub const ALIGNER_ENV: &str = "ANYMD_ALIGNER_MODEL";
pub const ALIGNER_BIN_ENV: &str = "ANYMD_ALIGNER_BIN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub base_url: &'static str,
}

impl ModelSpec {
    pub fn file_name(&self) -> String {
        self.name.to_string()
    }
    pub fn size_label(&self) -> String {
        format!("{} MB", (self.bytes as f64 / 1_000_000.0).round())
    }
}

pub const ASR: ModelSpec = ModelSpec {
    name: "Qwen3-ASR-1.7B-Q8_0.gguf",
    bytes: 2_185_030_624,
    sha256: "9a0d81792dfea2d5f278b8a63deb3ea6e02139ce42c2301f32ea19c4f77526b7",
    base_url: "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/3555bd238a8572bbace3ebf60d23b036dc0a5dbe",
};
pub const ALIGNER: ModelSpec = ModelSpec {
    name: "qwen3-forced-aligner-0.6b-q8_0.gguf",
    bytes: 985_594_624,
    sha256: "539df5dd0fe1721e378ac13bfac9a26b1260dafb62d892c518c1f21244762636",
    base_url: "https://huggingface.co/cstr/qwen3-forced-aligner-0.6b-GGUF/resolve/1ec5110602ccab18c878ddebedab0891e290a95c",
};

#[cfg(all(test, feature = "native"))]
mod tests;
