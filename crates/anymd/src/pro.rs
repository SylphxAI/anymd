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

use mcp_kit::licence::{self, Licence, LicenceError, LicencePolicy, MoneyCheckout};
use std::fmt;
use std::path::PathBuf;

/// Trusted public keys (base64url raw Ed25519). A list so rotation is additive.
pub const PRO_PUBLIC_KEYS: &[&str] = &["xO9jSvEq5nsVPMk9x62Egr0_n5WPpWCF8yCYmrwzH3Y"];

/// Where Pro is explained and sold.
pub const PRO_URL: &str = "https://sylphxai.github.io/anymd/pricing";

/// Env var holding the token (wins over the token file).
pub const TOKEN_ENV: &str = "ANYMD_PRO_TOKEN";

/// anymd's licence policy. `require_product` is false because tokens issued
/// before the generic flow carry no `product` field.
fn policy_with<'a>(keys: &'a [&'a str]) -> LicencePolicy<'a> {
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

fn policy() -> LicencePolicy<'static> {
    policy_with(PRO_PUBLIC_KEYS)
}

fn checkout() -> MoneyCheckout {
    MoneyCheckout::from_docs_toml(include_str!("money_docs.toml"), "pro")
        .expect("anymd's embedded Keel Money configuration must be valid")
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

/// Returned by [`require_pro`] when a Pro feature is used without a licence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProRequired {
    pub feature: String,
}

impl fmt::Display for ProRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is part of anymd Pro with a one-time offline licence. See current pricing or buy it with `anymd pro buy`. Learn more: {PRO_URL}",
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

/// `anymd pro status | activate <token> | buy`; returns the exit code.
pub fn run(arguments: &[String]) -> i32 {
    let checkout = arguments
        .first()
        .filter(|command| command.as_str() == "buy")
        .map(|_| checkout());
    run_with(&policy(), checkout.as_ref(), arguments)
}

fn valid_buy_args(arguments: &[String]) -> bool {
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--no-browser" | "--json" | "--accept-terms" => index += 1,
            "--email" => {
                if arguments.get(index + 1).is_none_or(|v| v.starts_with("--")) {
                    return false;
                }
                index += 2;
            }
            // anymd sells one licence; seat packs and quantities do not apply.
            "--pack" | "--qty" => return false,
            _ => return false,
        }
    }
    true
}

fn run_with(policy: &LicencePolicy, checkout: Option<&MoneyCheckout>, arguments: &[String]) -> i32 {
    let ok = match arguments.first().map(String::as_str) {
        Some("status") => arguments.len() == 1,
        Some("activate") => arguments.len() == 2,
        Some("buy") => valid_buy_args(&arguments[1..]),
        _ => false,
    };
    if !ok {
        eprintln!(
            "usage: anymd pro status | anymd pro activate <token> | anymd pro buy [--email <address>] [--accept-terms] [--no-browser] [--json]"
        );
        return 2;
    }
    match checkout {
        Some(checkout) => licence::run_money_cli(policy, checkout, arguments),
        None => licence::run_cli(policy, arguments),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};

    fn verify_token_with(token: &str, keys: &[&str]) -> Result<ProLicense, LicenseError> {
        policy_with(keys).verify(token)
    }

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
    fn policy_keeps_existing_customer_contract() {
        let p = policy();
        assert_eq!(p.product, "anymd");
        assert!(!p.require_product);
        assert_eq!(p.accepted_plans, &["pro"]);
        assert_eq!(p.public_keys, PRO_PUBLIC_KEYS);
        assert_eq!(
            PRO_PUBLIC_KEYS,
            &["xO9jSvEq5nsVPMk9x62Egr0_n5WPpWCF8yCYmrwzH3Y"]
        );
        assert_eq!(p.env_var, "ANYMD_PRO_TOKEN");
        assert_eq!(p.upgrade_url, PRO_URL);
        let path = token_path().expect("config dir");
        assert!(path.ends_with("anymd/pro-token"), "{path:?}");
        // A token with no `product` (every pre-move token) is still accepted.
        let k = key(1);
        assert!(verify_token_with(&token(&k, PRO), &[&public(&k)]).is_ok());
    }

    #[test]
    fn buy_wiring() {
        // No checkout service: `buy` points at the upgrade page and succeeds.
        assert_eq!(
            run_with(&policy_with(PRO_PUBLIC_KEYS), None, &["buy".into()]),
            0
        );
        // The legacy central claims service is no longer wired into the product.
        assert_eq!(policy().checkout_base, None);
        let money = checkout();
        assert_eq!(money.product, "anymd_pro");
        assert_eq!(money.prices.len(), 1);
        assert_eq!(money.prices[0].key, "anymd_pro");
        assert_eq!(money.prices[0].quantity, 1);
        assert!(money.publishable_key.starts_with("sylphx_pk_"));
        // anymd sells one licence: pack and quantity flags are usage errors.
        for flag in ["--pack", "--qty", "--x"] {
            assert_eq!(
                run_with(
                    &policy(),
                    Some(&checkout()),
                    &["buy".into(), flag.into(), "1".into()]
                ),
                2
            );
        }
        assert_eq!(run_with(&policy(), Some(&checkout()), &[]), 2);
        assert_eq!(
            run_with(&policy(), Some(&checkout()), &["status".into(), "x".into()]),
            2
        );
    }

    // Run the public CLI dispatcher in a separate process so its config directory
    // and token environment cannot affect other licence tests.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "helper for buy_loopback_contract"]
    fn buy_loopback_child() {
        let base = std::env::var("ANYMD_TEST_CHECKOUT").unwrap();
        let public_key = public(&key(1));
        let keys = [&*public_key];
        let p = policy_with(&keys);
        let mut money = checkout();
        money.api_origin = base;
        let expected: i32 = std::env::var("ANYMD_TEST_EXIT").unwrap().parse().unwrap();
        assert_eq!(
            run_with(
                &p,
                Some(&money),
                &[
                    "buy".into(),
                    "--no-browser".into(),
                    "--json".into(),
                    "--email".into(),
                    "buyer@example.test".into(),
                    "--accept-terms".into(),
                ]
            ),
            expected
        );
        assert_eq!(p.current().is_some(), expected == 0);
        assert_eq!(p.token_path().unwrap().exists(), expected == 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn buy_loopback_contract() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use std::process::Command;

        for (signer, expected) in [(1, 0), (2, 1)] {
            let config = tempfile::tempdir().unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let parent = "orgs/org_fs01afbdccgn8xdk1ghj5y47w0vtph/projects/prj_fs013ejakyn22t4bsvrvyj0f2x1pgq/envs/env_fs01p2f389mwmm2xsm1nmvcst69vgd";
            let created = serde_json::json!({
                "processor_session_id": "cs_test_localcontract123",
                "url": "https://pay.example.test/anymd",
                "claim_secret": "clm_local_contract",
            });
            let pending = serde_json::json!({"details": [{"reason": "purchase_pending"}]});
            let paid = serde_json::json!({
                "paid": true,
                "licence_tokens": [{"token": token(&key(signer), PRO)}],
            });
            let server = std::thread::spawn(move || {
                for (method, path, status, response) in [
                    (
                        "POST",
                        format!("/v1/money/{parent}/checkout_sessions:clientCreate"),
                        200,
                        created,
                    ),
                    (
                        "POST",
                        "/v1/money/checkout_sessions:redeem".to_string(),
                        412,
                        pending,
                    ),
                    (
                        "POST",
                        "/v1/money/checkout_sessions:redeem".to_string(),
                        200,
                        paid,
                    ),
                ] {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    assert_eq!(line.trim(), format!("{method} {path} HTTP/1.1"));
                    let mut length = 0;
                    loop {
                        line.clear();
                        assert!(reader.read_line(&mut line).unwrap() > 0);
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    if path.ends_with(":clientCreate") {
                        assert_eq!(body["product"], "anymd_pro");
                        assert_eq!(body["price"], "anymd_pro");
                        assert_eq!(body["quantity"], 1);
                        assert_eq!(body["buyer_email"], "buyer@example.test");
                        assert_eq!(body["consent"]["terms_accepted"], true);
                    } else {
                        assert_eq!(body["checkout_session"], "cs_test_localcontract123");
                        assert_eq!(body["claim_secret"], "clm_local_contract");
                    }
                    let body = response.to_string();
                    write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
            });
            let output = Command::new("timeout")
                .arg("30s")
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "pro::tests::buy_loopback_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("XDG_CONFIG_HOME", config.path())
                .env("HOME", config.path())
                .env_remove(TOKEN_ENV)
                .env("ANYMD_TEST_CHECKOUT", &base)
                .env("ANYMD_TEST_EXIT", expected.to_string())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            server.join().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("\"event\":\"checkout\""));
            assert!(stdout.contains(if expected == 0 {
                "\"status\":\"paid\""
            } else {
                "\"status\":\"invalid\""
            }));
            assert!(!stdout.contains("\"token\":"));
        }
    }

    #[test]
    fn require_pro_message() {
        assert!(require_pro_with("Video evidence", true).is_ok());
        let required = require_pro_with("Cite-check", false).unwrap_err();
        let message = required.to_string();
        assert_eq!(
            message,
            "Cite-check is part of anymd Pro with a one-time offline licence. See current pricing or buy it with `anymd pro buy`. Learn more: https://sylphxai.github.io/anymd/pricing"
        );
        let result = required_result(&required);
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].as_text().unwrap().text, message);
    }
}
