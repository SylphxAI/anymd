//! anymd Pro licence: an offline-verified Ed25519 token that unlocks only new
//! operations (video evidence, cite-check). The core stays MIT and free; no
//! existing operation calls [`require_pro`].
//!
//! Token: `base64url(payloadJSON).base64url(ed25519 signature over the payload
//! bytes)`, payload `{"plan":"pro","email"?:string,"issuedAt":number}`.
//! The token is never logged or printed.
//!
//! Verification, the token file, `status`, `activate` and `buy` are the generic
//! `mcp_kit::licence` flow; this module only holds anymd's policy (key, env
//! var, file name, URLs). The token format and file location are unchanged, so
//! tokens issued before the move keep working.

mod cli;
mod entitlements;
mod license;
mod pricing;

#[cfg(test)]
mod tests;

pub use cli::run;
pub(crate) use entitlements::require_pro_with;
pub use entitlements::{require_pro, required_result, ProRequired};
pub use license::{
    current_license, token_path, verify_token, LicenseError, ProLicense, PRO_PUBLIC_KEYS, TOKEN_ENV,
};
pub use pricing::{
    discovery_hint, format_currency_price, pre_checkout_disclosure, PRO_CURRENCIES, PRO_URL,
};
