use super::cli::{checkout, run_with};
use super::license::{policy, policy_with, test_policy};
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
fn test_activation_is_separate_from_production() {
    let production = policy();
    let test = test_policy();
    assert!(test
        .public_keys
        .iter()
        .all(|key| !production.public_keys.contains(key)));
    assert_ne!(production.env_var, test.env_var);
    assert_ne!(production.file_name, test.file_name);
    assert!(test.tier.contains("TEST"));
    assert_eq!(run(&["--test".into(), "buy".into()]), 2);
    // Rotation accepts both an old and new signer, never an unrelated one.
    let (old, new, other) = (key(21), key(22), key(23));
    let (old_public, new_public) = (public(&old), public(&new));
    let keys = [old_public.as_str(), new_public.as_str()];
    for signer in [&old, &new] {
        assert!(verify_token_with(&token(signer, PRO), &keys).is_ok());
        assert_eq!(
            verify_token_with(
                &token(signer, r#"{"plan":"pro","issuedAt":1,"product":"foreign"}"#),
                &keys
            ),
            Err(LicenseError::WrongProduct)
        );
        assert_eq!(
            verify_token_with(
                &token(signer, r#"{"plan":"pro","issuedAt":1,"expiresAt":1}"#),
                &keys
            ),
            Err(LicenseError::Expired)
        );
    }
    assert_eq!(
        verify_token_with(&token(&other, PRO), &keys),
        Err(LicenseError::BadSignature)
    );
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
        &[
            "xO9jSvEq5nsVPMk9x62Egr0_n5WPpWCF8yCYmrwzH3Y",
            "zYLCVWNwA6w-EI4z-Z1HNYsKkZj1c3JrZtewsGVjUiM",
        ]
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
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
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
        "Cite-check is part of anymd Pro ($29 one-time offline licence). See current pricing or buy it with `anymd pro buy`. Learn more: https://anymd.sylphx.com/pricing"
    );
    let result = required_result(&required);
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.content[0].as_text().unwrap().text, message);
}

#[test]
fn pre_checkout_disclosure_and_multi_currency_selection() {
    assert_eq!(PRO_CURRENCIES.len(), 16);
    assert_eq!(format_currency_price("usd"), Some(("USD", "US$29")));
    assert_eq!(format_currency_price("GBP"), Some(("GBP", "£21.99")));
    assert_eq!(format_currency_price("jpy"), Some(("JPY", "¥4,639")));
    assert_eq!(format_currency_price("INVALID"), None);
    let disclosure = pre_checkout_disclosure("GBP");
    assert!(disclosure.contains("£21.99 (GBP)"));
    assert!(disclosure.contains("US$29 once"));
    assert!(disclosure.contains("anymd pro activate '<token>'"));
    assert_eq!(
        run_with(
            &policy(),
            None,
            &["buy".into(), "--currency".into(), "GBP".into()]
        ),
        0
    );
    assert_eq!(
        run_with(&policy(), None, &["buy".into(), "--currency=EUR".into()]),
        0
    );
    assert_eq!(
        run_with(
            &policy(),
            Some(&checkout()),
            &["buy".into(), "--currency".into(), "INVALID".into()]
        ),
        2
    );
    assert!(discovery_hint().contains("$29 once"));
}
