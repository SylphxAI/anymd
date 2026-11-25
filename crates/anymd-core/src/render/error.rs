//! Structured errors shared across render planning, rasterization, and cropping.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderErrorCode {
    InvalidRequest,
    InvalidPdf,
    EncryptedPdf,
    LimitExceeded,
    EncodeFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderError {
    pub code: RenderErrorCode,
    pub message: String,
}

impl RenderError {
    pub(super) fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::InvalidRequest,
            message: message.into(),
        }
    }

    pub(super) fn invalid_pdf(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::InvalidPdf,
            message: message.into(),
        }
    }

    pub(super) fn encrypted_pdf(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::EncryptedPdf,
            message: message.into(),
        }
    }

    pub(super) fn limit_exceeded(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::LimitExceeded,
            message: message.into(),
        }
    }

    pub(super) fn encode_failed(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::EncodeFailed,
            message: message.into(),
        }
    }
}
