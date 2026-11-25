//! Evidence redaction policies and detectors.

use std::sync::OnceLock;

use regex::Regex;

use super::super::pattern;

static PRIVATE_KEY_PATTERN: OnceLock<Regex> = OnceLock::new();
static JWT_PATTERN: OnceLock<Regex> = OnceLock::new();
static SECRET_PATTERN: OnceLock<Regex> = OnceLock::new();
static EMAIL_PATTERN: OnceLock<Regex> = OnceLock::new();
static SSN_PATTERN: OnceLock<Regex> = OnceLock::new();
static CREDIT_CARD_PATTERN: OnceLock<Regex> = OnceLock::new();
static IPV4_PATTERN: OnceLock<Regex> = OnceLock::new();
static PHONE_PATTERN: OnceLock<Regex> = OnceLock::new();

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|entry| entry == value) {
        values.push(value.to_owned());
    }
}

fn luhn_check(digits: &str) -> bool {
    let mut sum = 0u32;
    let mut should_double = false;
    for byte in digits.bytes().rev() {
        let mut digit = u32::from(byte - b'0');
        if should_double {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
        should_double = !should_double;
    }
    sum > 0 && sum.is_multiple_of(10)
}

pub(super) fn redact_trust_evidence(value: &str, policy: &str) -> (String, Vec<String>) {
    if policy == "off" {
        return (value.to_owned(), Vec::new());
    }

    let mut types = Vec::new();
    let mut text = pattern(&PRIVATE_KEY_PATTERN, r"-----BEGIN [A-Z ]*PRIVATE KEY-----")
        .replace_all(value, |_: &regex::Captures<'_>| -> String {
            push_unique(&mut types, "private_key_marker");
            "[REDACTED_PRIVATE_KEY_MARKER]".to_owned()
        })
        .into_owned();
    text = pattern(
        &JWT_PATTERN,
        r"(?-u:\b)eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}(?-u:\b)",
    )
    .replace_all(&text, |_: &regex::Captures<'_>| -> String {
        push_unique(&mut types, "jwt");
        "[REDACTED_JWT]".to_owned()
    })
    .into_owned();
    text = pattern(
        &SECRET_PATTERN,
        r#"(?i)(?-u:\b)(api[_-]?key|secret|token|password)\s*[:=]\s*['\"]?[A-Za-z0-9._~+/=-]{12,}['\"]?"#,
    )
    .replace_all(&text, |captures: &regex::Captures<'_>| {
        push_unique(&mut types, "secret");
        format!("{}=[REDACTED_SECRET]", &captures[1])
    })
    .into_owned();
    text = pattern(
        &EMAIL_PATTERN,
        r"(?i)(?-u:\b)[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}(?-u:\b)",
    )
    .replace_all(&text, |_: &regex::Captures<'_>| -> String {
        push_unique(&mut types, "email");
        "[REDACTED_EMAIL]".to_owned()
    })
    .into_owned();
    text = pattern(&SSN_PATTERN, r"(?-u:\b)[0-9]{3}-[0-9]{2}-[0-9]{4}(?-u:\b)")
        .replace_all(&text, |_: &regex::Captures<'_>| -> String {
            push_unique(&mut types, "ssn");
            "[REDACTED_SSN]".to_owned()
        })
        .into_owned();
    text = pattern(
        &CREDIT_CARD_PATTERN,
        r"(?-u:\b)(?:[0-9][ -]*?){13,19}(?-u:\b)",
    )
    .replace_all(&text, |captures: &regex::Captures<'_>| {
        let candidate = captures.get(0).map_or("", |entry| entry.as_str());
        let digits = candidate
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(char::from)
            .collect::<String>();
        if !(13..=19).contains(&digits.len()) || !luhn_check(&digits) {
            return candidate.to_owned();
        }
        push_unique(&mut types, "credit_card");
        format!(
            "[REDACTED_CREDIT_CARD_LAST4_{}]",
            &digits[digits.len() - 4..]
        )
    })
    .into_owned();

    if policy == "strict" {
        text = pattern(
            &IPV4_PATTERN,
            r"(?-u:\b)(?:(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])\.){3}(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])(?-u:\b)",
        )
            .replace_all(&text, |_: &regex::Captures<'_>| -> String {
                push_unique(&mut types, "ipv4");
                "[REDACTED_IPV4]".to_owned()
        })
        .into_owned();
        text = pattern(
            &PHONE_PATTERN,
            r"(^|[^A-Za-z0-9_+])(\+?[0-9][0-9 .()/-]{7,}[0-9])(?-u:\b)",
        )
        .replace_all(&text, |captures: &regex::Captures<'_>| {
            let prefix = captures.get(1).map_or("", |entry| entry.as_str());
            let candidate = captures.get(2).map_or("", |entry| entry.as_str());
            let digits = candidate
                .bytes()
                .filter(u8::is_ascii_digit)
                .map(char::from)
                .collect::<String>();
            if !(8..=15).contains(&digits.len()) {
                return captures
                    .get(0)
                    .map_or("", |entry| entry.as_str())
                    .to_owned();
            }
            push_unique(&mut types, "phone");
            format!(
                "{prefix}[REDACTED_PHONE_LAST4_{}]",
                &digits[digits.len() - 4..]
            )
        })
        .into_owned();
    }

    (text, types)
}
