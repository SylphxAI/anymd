use std::path::PathBuf;

use mcp_kit::licence::{Licence, LicenceError, LicencePolicy};

use super::pricing::PRO_URL;

/// Trusted public keys (base64url raw Ed25519). A list so rotation is additive.
pub const PRO_PUBLIC_KEYS: &[&str] = &[
    // Preserve every historical licence when Money rotates its signer.
    "xO9jSvEq5nsVPMk9x62Egr0_n5WPpWCF8yCYmrwzH3Y",
    "zYLCVWNwA6w-EI4z-Z1HNYsKkZj1c3JrZtewsGVjUiM",
];

/// Money test signers are separate: they never unlock ordinary Pro operations.
const TEST_PUBLIC_KEYS: &[&str] = &["PEdpVC-uXLyTfAcYDWXh-FoP9vRE12eb4Fax9OZ4QT8"];

pub(super) fn test_policy() -> LicencePolicy<'static> {
    LicencePolicy {
        public_keys: TEST_PUBLIC_KEYS,
        env_var: "ANYMD_TEST_PRO_TOKEN",
        file_name: "test-pro-token",
        tier: "Pro (TEST)",
        ..policy_with(TEST_PUBLIC_KEYS)
    }
}

/// Env var holding the token (wins over the token file).
pub const TOKEN_ENV: &str = "ANYMD_PRO_TOKEN";

/// anymd's licence policy. `require_product` is false because tokens issued
/// before the generic flow carry no `product` field.
pub(super) fn policy_with<'a>(keys: &'a [&'a str]) -> LicencePolicy<'a> {
    LicencePolicy {
        product: "anymd",
        require_product: false,
        accepted_plans: &["pro"],
        public_keys: keys,
        env_var: TOKEN_ENV,
        // `<config dir>/anymd/pro-token`, as before.
        file_name: "pro-token",
        upgrade_url: PRO_URL,
        tier: "Pro",
        checkout_base: None,
    }
}

pub(super) fn policy() -> LicencePolicy<'static> {
    policy_with(PRO_PUBLIC_KEYS)
}

pub type ProLicense = Licence;
pub type LicenseError = LicenceError;

/// Verify a token against [`PRO_PUBLIC_KEYS`].
pub fn verify_token(token: &str) -> Result<ProLicense, LicenseError> {
    policy().verify(token)
}

/// The token file: `<config dir>/anymd/pro-token`.
pub fn token_path() -> Option<PathBuf> {
    policy().token_path()
}

/// The active licence, or `None` when no valid token is configured.
pub fn current_license() -> Option<ProLicense> {
    policy().current()
}
