use std::fmt;

use super::license::current_license;
use super::pricing::PRO_URL;

/// Returned by [`require_pro`] when a Pro feature is used without a licence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProRequired {
    pub feature: String,
}

impl fmt::Display for ProRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is part of anymd Pro ($29 one-time offline licence). See current pricing or buy it with `anymd pro buy`. Learn more: {PRO_URL}",
            self.feature
        )
    }
}

impl std::error::Error for ProRequired {}

/// Gate for new Pro operations: `Ok(())` with a valid licence, else a polite,
/// agent-relayable error. Not called by any free operation.
pub fn require_pro(feature: &str) -> Result<(), ProRequired> {
    require_pro_with(feature, current_license().is_some())
}

pub(crate) fn require_pro_with(feature: &str, active: bool) -> Result<(), ProRequired> {
    if active {
        Ok(())
    } else {
        Err(ProRequired {
            feature: feature.to_string(),
        })
    }
}

/// The normal (non-error) MCP tool result an unlicensed Pro call returns, so
/// the agent relays the message to the user.
pub fn required_result(required: &ProRequired) -> rmcp::model::CallToolResult {
    rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::text(
        required.to_string(),
    )])
}
