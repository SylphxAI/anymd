//! Error helpers shared by the PP-DocLayout modules.

use crate::error::Error;
use crate::error::ProcessingStage;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};

pub(super) const MODEL_NAME: &str = "PP-DocLayout";

/// Wraps a Candle failure that happened while loading or running the model.
pub(super) fn infer_err(context: &'static str, error: candle_core::Error) -> Error {
    candle_to_ocr_inference(MODEL_NAME, context, error)
}

/// Wraps a Candle failure in tensor pre/post-processing.
pub(super) fn proc_err(context: &'static str, error: candle_core::Error) -> Error {
    candle_to_ocr_processing(ProcessingStage::TensorOperation, context, error)
}
