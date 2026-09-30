//! Error type for the VL models.

use std::fmt;
use thiserror::Error;

/// Convenience alias for results produced by this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Result of a batch operation with both batch-level and item-level failures.
///
/// The outer [`Result`] reports failures that prevented the batch from running;
/// each inner result reports decoding or post-processing failure for one item.
pub type BatchResult<T> = Result<Vec<Result<T>>>;

/// Boxed source error carried by [`Error::Processing`] and [`Error::Inference`].
pub type Source = Box<dyn std::error::Error + Send + Sync>;

/// Where in a pipeline a failure happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingStage {
    /// Tensor construction, reshaping or arithmetic.
    TensorOperation,
    /// Image normalization.
    Normalization,
    /// Image resizing.
    Resize,
    /// Any other image processing step.
    ImageProcessing,
    /// Decoding or assembling model output.
    PostProcessing,
}

impl fmt::Display for ProcessingStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::TensorOperation => "tensor operation",
            Self::Normalization => "normalization",
            Self::Resize => "resize",
            Self::ImageProcessing => "image processing",
            Self::PostProcessing => "post-processing",
        };
        f.write_str(name)
    }
}

/// A displayable error captured as a string, for sources that are neither
/// `Send` nor `Sync`.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct OpaqueError(pub String);

impl OpaqueError {
    /// Captures any displayable error.
    pub fn from_display(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

/// Everything that can go wrong loading or running a model in this crate.
#[derive(Debug, Error)]
pub enum Error {
    /// A model configuration or checkpoint layout is unusable.
    #[error("configuration: {message}")]
    Config {
        /// What is wrong with the configuration.
        message: String,
    },

    /// A caller-supplied value is out of range or the wrong shape.
    #[error("invalid input: {message}")]
    InvalidInput {
        /// What is wrong with the input.
        message: String,
    },

    /// Pre- or post-processing failed.
    #[error("{kind} failed: {context}")]
    Processing {
        /// The stage that failed.
        kind: ProcessingStage,
        /// What was being done.
        context: String,
        /// The underlying failure.
        #[source]
        source: Source,
    },

    /// A forward pass failed.
    #[error("inference failed in model '{model_name}': {context}")]
    Inference {
        /// The model that failed.
        model_name: String,
        /// What was being done.
        context: String,
        /// The underlying failure.
        #[source]
        source: Source,
    },

    /// Reading a checkpoint or image from disk failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Decoding an image failed.
    #[error("image load: {0}")]
    ImageLoad(#[from] image::ImageError),
}

impl Error {
    /// Builds a [`Error::Config`] from a message.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    /// Builds an [`Error::InvalidInput`] from a message.
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }
}

impl From<candle_core::Error> for Error {
    fn from(error: candle_core::Error) -> Self {
        Self::Processing {
            kind: ProcessingStage::TensorOperation,
            context: "candle operation".to_string(),
            source: Box::new(error),
        }
    }
}
