//! Explicit model setup and bounded local doc-VLM worker.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod companion;
mod model_cache;
mod weights;
mod worker;

pub use companion::{
    companion_asset, companion_file_name, companion_platform, manifest_hash, COMPANION_MANIFEST,
};
pub(crate) use companion::{companion_installed_at, ready_at, worker_command};
pub use model_cache::root;
pub(crate) use model_cache::{installed_at, model_revision};
pub use worker::worker;
pub(crate) use worker::{max_tokens, page_timeout, remaining_page_timeout, run_worker};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OcrEngine {
    #[default]
    Auto,
    Vlm,
    Tesseract,
}

/// Retain the existing boolean MCP contract while adding named engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum OcrSelection {
    Enabled(bool),
    Engine(OcrEngine),
}
impl OcrSelection {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Engine(OcrEngine::Auto)),
            "vlm" => Ok(Self::Engine(OcrEngine::Vlm)),
            "tesseract" => Ok(Self::Engine(OcrEngine::Tesseract)),
            _ => Err("OCR needs auto, vlm or tesseract".into()),
        }
    }
    pub fn enabled(self) -> bool {
        !matches!(self, Self::Enabled(false))
    }
    pub fn engine(self) -> OcrEngine {
        match self {
            Self::Engine(e) => e,
            _ => OcrEngine::Auto,
        }
    }
}

/// A boolean enables/disables OCR but does not choose a runtime. Named modes
/// override the environment; legacy `--ocr`/`ocr: true` inherit its runtime.
pub fn resolve_engine(
    selection: Option<OcrSelection>,
    environment: Option<&str>,
) -> Result<OcrEngine, String> {
    match selection {
        Some(OcrSelection::Engine(engine)) => Ok(engine),
        Some(OcrSelection::Enabled(false)) => Ok(OcrEngine::Auto),
        _ => environment
            .map(OcrSelection::parse)
            .transpose()
            .map(|s| s.map(OcrSelection::engine).unwrap_or_default()),
    }
}

/// The one-line message for a VLM request on a machine without the engine or
/// weights. MCP returns it as a normal tool result, like the Pro notice.
pub const SETUP_NOTICE: &str = "VLM OCR needs a one-time setup: run `anymd setup ocr` to download the local OCR engine and ~2 GB of model weights, then retry this read with OCR enabled. Setup opts into the download; documents stay on this machine.";

pub fn is_setup_notice(message: &str) -> bool {
    message.contains(SETUP_NOTICE)
}

/// Trusted companion signing keys (base64url raw Ed25519), separate from the
/// Pro licence keys. A list so rotation is additive.
pub const COMPANION_PUBLIC_KEYS: &[&str] = &["zQhktMv7ijbI888ZY9i7xXqExloXuZ-yZUma1ll36j4"];

pub fn requested(engine: OcrEngine) -> bool {
    if engine == OcrEngine::Vlm {
        return true;
    }
    if engine == OcrEngine::Tesseract {
        return false;
    }
    let mut refresh_error = None;
    if let Ok(root) = root() {
        match companion::refresh_if_stale(&root, &companion::refreshed_companion) {
            Ok(()) if ready_at(&root) && backend_available() => return true,
            Ok(()) => {}
            Err(e) => refresh_error = Some(e),
        }
    }
    static HINT: std::sync::Once = std::sync::Once::new();
    HINT.call_once(|| {
        if let Some(error) = &refresh_error {
            eprintln!("anymd OCR: using tesseract; {error}");
        } else if !anymd_ocr_vlm::hardware::cpu_available() { eprintln!("anymd OCR: using tesseract; doc-VLM needs FP16-capable Linux arm64 hardware."); }
        else { eprintln!("anymd OCR: using tesseract; run `anymd setup ocr` to opt into local doc-VLM OCR (~2 GB weights and the OCR engine)."); }
    });
    // Acceleration alone cannot trigger a model download.
    false
}

/// The hardware can run the engine. Whether the engine is installed is
/// `ready_at`.
pub fn backend_available() -> bool {
    anymd_ocr_vlm::hardware::cpu_available()
}

pub fn metal_available() -> bool {
    #[cfg(feature = "ocr-vlm")]
    {
        anymd_ocr_vlm::candle_backend::CandleBackend::metal_available()
    }
    #[cfg(not(feature = "ocr-vlm"))]
    {
        false
    }
}

pub fn install() -> Result<PathBuf, String> {
    let root = root()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // The engine is small and fails fastest, so fetch it before ~2 GB of weights.
    if !companion_installed_at(&root) || !companion::companion_intact_at(&root)? {
        companion::install_companion(&root)?;
    }
    model_cache::install_weights(&root)?;
    Ok(root)
}

/// One admitted request, shared by every lazily read PDF page. A failed page
/// poisons this request so callers cannot start a later worker.
pub(crate) struct OcrRequest {
    permit: crate::ocr_evidence::OcrRequestPermit,
    deadline: std::time::Instant,
    failed: Option<String>,
}

impl OcrRequest {
    pub(crate) fn acquire() -> Result<Self, String> {
        Ok(Self {
            permit: crate::ocr_evidence::acquire_ocr_permit()?,
            deadline: std::time::Instant::now()
                + std::time::Duration::from_millis(crate::provider_budget::MAX_REQUEST_TIMEOUT_MS),
            failed: None,
        })
    }

    pub(crate) fn run_page<T>(
        &mut self,
        run: impl FnOnce(
            std::time::Instant,
            &crate::ocr_evidence::OcrRequestPermit,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        let result =
            remaining_page_timeout(self.deadline).and_then(|_| run(self.deadline, &self.permit));
        if let Err(error) = &result {
            self.failed = Some(error.clone());
        }
        result
    }
}

pub fn recognize(bytes: &[u8]) -> Result<anymd_ocr_vlm::PageResult, String> {
    let mut request = OcrRequest::acquire()?;
    request.run_page(|deadline, permit| recognize_admitted(bytes, deadline, permit))
}

pub(crate) fn recognize_admitted(
    bytes: &[u8],
    deadline: std::time::Instant,
    _permit: &crate::ocr_evidence::OcrRequestPermit,
) -> Result<anymd_ocr_vlm::PageResult, String> {
    let root = root()?;
    if !installed_at(&root) {
        return Err(SETUP_NOTICE.into());
    }
    let command = worker_command()?;
    let timeout = remaining_page_timeout(deadline)?;
    run_worker(&command, bytes, timeout, max_tokens()?)
}

#[cfg(test)]
mod tests;
