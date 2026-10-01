//! anymd Pro licence: an offline-verified Ed25519 token that unlocks only new
//! operations (video evidence, cite-check). The core stays MIT and free; no
//! existing operation calls [`require_pro`].
//!
//! Token: `base64url(payloadJSON).base64url(ed25519 signature over the payload
//! bytes)`, payload `{"plan":"pro","email"?:string,"issuedAt":number}`.
//! The token is never logged or printed.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use std::fmt;
use std::path::PathBuf;

/// Trusted public keys (base64url raw Ed25519). A list so rotation is additive.
pub const PRO_PUBLIC_KEYS: &[&str] = &["xO9jSvEq5nsVPMk9x62Egr0_n5WPpWCF8yCYmrwzH3Y"];

/// Where Pro is explained and sold. The price lives on that page, not in the binary.
pub const PRO_URL: &str = "https://sylphxai.github.io/anymd/pro";

/// Env var holding the token (wins over the token file).
pub const TOKEN_ENV: &str = "ANYMD_PRO_TOKEN";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProLicense {
    pub plan: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(rename = "issuedAt")]
    pub issued_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseError {
    Malformed,
    BadSignature,
    WrongPlan,
}

impl fmt::Display for LicenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "the anymd Pro token is malformed",
            Self::BadSignature => "the anymd Pro token signature is not valid",
            Self::WrongPlan => "the token is not an anymd Pro token",
        })
    }
}

impl std::error::Error for LicenseError {}

/// Verify a token against [`PRO_PUBLIC_KEYS`].
pub fn verify_token(token: &str) -> Result<ProLicense, LicenseError> {
    verify_token_with(token, PRO_PUBLIC_KEYS)
}

fn verify_token_with(token: &str, keys: &[&str]) -> Result<ProLicense, LicenseError> {
    let (payload_b64, sig_b64) = token
        .trim()
        .split_once('.')
        .ok_or(LicenseError::Malformed)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| LicenseError::Malformed)?;
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| LicenseError::Malformed)?;
    let signature = Signature::from_slice(&sig_bytes).map_err(|_| LicenseError::Malformed)?;
    let license: ProLicense =
        serde_json::from_slice(&payload).map_err(|_| LicenseError::Malformed)?;
    let verified = keys.iter().any(|key| {
        URL_SAFE_NO_PAD
            .decode(key)
            .ok()
            .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
            .and_then(|raw| VerifyingKey::from_bytes(&raw).ok())
            .is_some_and(|key| key.verify(&payload, &signature).is_ok())
    });
    if !verified {
        return Err(LicenseError::BadSignature);
    }
    if license.plan != "pro" {
        return Err(LicenseError::WrongPlan);
    }
    Ok(license)
}

/// The token file: `<config dir>/anymd/pro-token`.
pub fn token_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("anymd").join("pro-token"))
}

/// The configured token: env first, else the token file.
fn find_token(env: Option<String>, file: Option<PathBuf>) -> Option<String> {
    if let Some(token) = env.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        return Some(token);
    }
    let text = std::fs::read_to_string(file?).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The active licence, or `None` when no valid token is configured.
pub fn current_license() -> Option<ProLicense> {
    current_license_with(std::env::var(TOKEN_ENV).ok(), token_path(), PRO_PUBLIC_KEYS)
}

fn current_license_with(
    env: Option<String>,
    file: Option<PathBuf>,
    keys: &[&str],
) -> Option<ProLicense> {
    verify_token_with(&find_token(env, file)?, keys).ok()
}

/// Returned by [`require_pro`] when a Pro feature is used without a licence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProRequired {
    pub feature: String,
}

impl fmt::Display for ProRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is part of anymd Pro. Learn more and get it: {PRO_URL}",
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

/// `anymd pro status | activate <token>`; returns the exit code.
pub fn run(arguments: &[String]) -> i32 {
    match arguments.first().map(String::as_str) {
        Some("status") if arguments.len() == 1 => {
            match current_license() {
                Some(license) => {
                    println!("anymd Pro: active");
                    println!("plan: {}", license.plan);
                    println!("issuedAt: {}", license.issued_at);
                }
                None => {
                    println!("anymd Pro: inactive");
                    println!("Learn more and get it: {PRO_URL}");
                }
            }
            0
        }
        Some("activate") if arguments.len() == 2 => match activate(&arguments[1]) {
            Ok(path) => {
                println!("anymd Pro activated ({})", path.display());
                0
            }
            Err(message) => {
                eprintln!("anymd pro activate: {message}");
                1
            }
        },
        _ => {
            eprintln!("usage: anymd pro status | anymd pro activate <token>");
            2
        }
    }
}

fn activate(token: &str) -> Result<PathBuf, String> {
    verify_token(token).map_err(|e| e.to_string())?;
    let path = token_path().ok_or("no config directory on this machine")?;
    write_token(&path, token.trim()).map_err(|e| format!("cannot write the token file: {e}"))?;
    Ok(path)
}

fn write_token(path: &std::path::Path, token: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(token.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public(key: &SigningKey) -> String {
        URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes())
    }

    fn token(key: &SigningKey, payload: &str) -> String {
        let sig = key.sign(payload.as_bytes());
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    const PRO: &str = r#"{"plan":"pro","email":"a@example.com","issuedAt":1700000000}"#;

    #[test]
    fn valid_token() {
        let k = key(1);
        let l = verify_token_with(&token(&k, PRO), &[&public(&k)]).unwrap();
        assert_eq!(l.plan, "pro");
        assert_eq!(l.email.as_deref(), Some("a@example.com"));
        assert_eq!(l.issued_at, 1_700_000_000);
        // email is optional; a rotated list still matches any key.
        let l = verify_token_with(
            &token(&k, r#"{"plan":"pro","issuedAt":5}"#),
            &["bogus", &public(&key(9)), &public(&k)],
        )
        .unwrap();
        assert_eq!(l.email, None);
    }

    #[test]
    fn bad_signature() {
        let (k, other) = (key(1), key(2));
        assert_eq!(
            verify_token_with(&token(&other, PRO), &[&public(&k)]),
            Err(LicenseError::BadSignature)
        );
        // Payload tampered after signing.
        let t = token(&k, PRO);
        let sig = t.split_once('.').unwrap().1;
        let forged = format!(
            "{}.{sig}",
            URL_SAFE_NO_PAD.encode(r#"{"plan":"pro","issuedAt":1}"#)
        );
        assert_eq!(
            verify_token_with(&forged, &[&public(&k)]),
            Err(LicenseError::BadSignature)
        );
        // The embedded production key does not accept a throwaway signer.
        assert_eq!(verify_token(&t), Err(LicenseError::BadSignature));
    }

    #[test]
    fn wrong_plan() {
        let k = key(1);
        assert_eq!(
            verify_token_with(
                &token(&k, r#"{"plan":"free","issuedAt":1}"#),
                &[&public(&k)]
            ),
            Err(LicenseError::WrongPlan)
        );
    }

    #[test]
    fn malformed() {
        let k = [public(&key(1))];
        let keys: Vec<&str> = k.iter().map(String::as_str).collect();
        for t in ["", "abc", "a.b", "!!.!!", "e30.AAAA"] {
            assert_eq!(
                verify_token_with(t, &keys),
                Err(LicenseError::Malformed),
                "{t}"
            );
        }
    }

    #[test]
    fn env_beats_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pro-token");
        let k = key(1);
        let good = token(&k, PRO);
        let pk = public(&k);
        write_token(&file, &good).unwrap();
        // Env wins even when it is invalid: no silent fallback to the file.
        assert!(current_license_with(Some("junk".into()), Some(file.clone()), &[&pk]).is_none());
        assert!(current_license_with(None, Some(file.clone()), &[&pk]).is_some());
        assert!(current_license_with(Some("  ".into()), Some(file.clone()), &[&pk]).is_some());
        let other = dir.path().join("none");
        assert!(current_license_with(Some(good), Some(other), &[&pk]).is_some());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn require_pro_message() {
        assert!(require_pro_with("Video evidence", true).is_ok());
        let message = require_pro_with("Cite-check", false)
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            "Cite-check is part of anymd Pro. Learn more and get it: https://sylphxai.github.io/anymd/pro"
        );
        assert!(!message.contains('$'));
    }
}
