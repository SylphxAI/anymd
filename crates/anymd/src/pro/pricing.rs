/// Where Pro is explained and sold.
pub const PRO_URL: &str = "https://anymd.sylphx.com/pricing";

/// All 16 currencies in the live anymd Pro catalogue (tax-inclusive one-time price).
pub const PRO_CURRENCIES: &[(&str, &str)] = &[
    ("USD", "US$29"),
    ("HKD", "HK$229"),
    ("TWD", "NT$959"),
    ("GBP", "£21.99"),
    ("EUR", "€26.99"),
    ("JPY", "¥4,639"),
    ("KRW", "₩38,900"),
    ("MYR", "RM118.99"),
    ("SGD", "S$37.99"),
    ("CAD", "CA$41.99"),
    ("AUD", "A$42.99"),
    ("NZD", "NZ$52.99"),
    ("THB", "฿989"),
    ("PHP", "₱1,859"),
    ("IDR", "Rp529,000"),
    ("VND", "₫749,000"),
];

/// Look up the formatted one-time price for a supported 3-letter currency code.
pub fn format_currency_price(code: &str) -> Option<(&'static str, &'static str)> {
    let trimmed = code.trim();
    PRO_CURRENCIES
        .iter()
        .copied()
        .find(|(c, _)| c.eq_ignore_ascii_case(trimmed))
}

/// Pre-checkout price, currency, and post-checkout CLI activation disclosure shown BEFORE
/// requesting the statutory 14-day immediate-supply waiver consent.
pub fn pre_checkout_disclosure(currency_code: &str) -> String {
    let (code, formatted) = format_currency_price(currency_code).unwrap_or(("USD", "US$29"));
    let currencies = PRO_CURRENCIES
        .iter()
        .map(|(c, p)| format!("{c} ({p})"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "anymd Pro — {formatted} ({code}) one-time payment (US$29 once; 100-year offline licence for 1 person, applicable tax included).\n\
         Supported currencies (--currency <CODE>): {currencies}\n\
         Post-checkout activation: keep this terminal open and `anymd pro buy` saves your licence automatically as soon as payment completes; \
         or copy the token from the browser checkout confirmation page and run `anymd pro activate '<token>'`."
    )
}

/// Crisp, non-blocking Aha hint for CLI conversion/doctor/setup discovery.
pub fn discovery_hint() -> String {
    format!(
        "anymd Pro ($29 once): verify PDF quotes (`cite_check`) and extract SHA-256 video frames offline — run `anymd pro buy` or visit {PRO_URL}"
    )
}
