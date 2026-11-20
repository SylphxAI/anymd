// Keel Money checkout & licence recovery client runtime.
//
// Mounts every [data-keel-money-checkout] component rendered by keel-web or
// keel-docs-site. Uses only public Sylphx Money client endpoints with a
// publishable key (sylphx_pk_* / pk_*):
//   - GET  {api_origin}/v1/money/{parent}/catalog
//   - POST {api_origin}/v1/money/{parent}/checkout_sessions:clientCreate
//   - POST {api_origin}/v1/money/checkout_sessions:redeem
//   - POST {api_origin}/v1/money/{parent}/licence_tokens:recover
//
// Invariants:
//   - Refuses to boot if publishable_key is missing or looks like a secret key.
//   - Never creates a checkout session or payment on page render.
//   - Requires live catalog hydration before enabling checkout; fails closed
//     if the catalog request fails or any configured product/price is missing
//     or archived (never sells stale build-time prices).
//   - Preserves distinct pack options when multiple seat packs share one
//     catalog price key with different quantities.
//   - Prevents double-submit and reuses a deterministic Idempotency-Key when
//     retrying the same logical checkout attempt.
//   - Never grants or claims an entitlement from URL query parameters alone;
//     requires both a verified paid response from checkout_sessions:redeem AND
//     the browser-bound claim_secret from sessionStorage before rendering any
//     offline licence token in the DOM.

const CLAIM_STORAGE_PREFIX = "keel:money:claim:";
const ATTEMPT_STORAGE_PREFIX = "keel:money:attempt:";
const ATTEMPT_NONCE_KEY = "keel:money:attempt-nonce";
const SESSION_PLACEHOLDER = "{CHECKOUT_SESSION_ID}";
const SESSION_PLACEHOLDER_TOKEN = "__KEEL_CHECKOUT_SESSION_ID_PLACEHOLDER__";
export const CLICK_ID_ALLOWLIST = [
  "gclid",
  "gbraid",
  "wbraid",
  "fbclid",
];
export const UTM_ALLOWLIST = [
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_content",
  "utm_term",
];
export const ATTRIBUTION_ALLOWLIST = [
  ...UTM_ALLOWLIST,
  ...CLICK_ID_ALLOWLIST,
  "entry_surface",
];
const ATTRIBUTION_STORAGE_KEY = "anymd_pro_attribution";

function readStoredAttribution() {
  try {
    if (typeof window === "undefined" || !window.sessionStorage) return {};
    const raw = window.sessionStorage.getItem(ATTRIBUTION_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const out = {};
    for (const key of ATTRIBUTION_ALLOWLIST) {
      const value = parsed[key];
      if (typeof value === "string" && value.trim()) {
        out[key] = value.trim().slice(0, MAX_UTM_VALUE_LENGTH);
      }
    }
    return out;
  } catch {
    return {};
  }
}

function writeStoredAttribution(attribution) {
  try {
    if (typeof window === "undefined" || !window.sessionStorage) return;
    if (!attribution || Object.keys(attribution).length === 0) return;
    window.sessionStorage.setItem(ATTRIBUTION_STORAGE_KEY, JSON.stringify(attribution));
  } catch {
    // Ignore storage errors in restricted environments.
  }
}
const MAX_UTM_VALUE_LENGTH = 500;

let inMemoryAttemptNonce = null;

// A UUIDv7 (RFC 9562 section 5.7): 48-bit big-endian Unix milliseconds,
// version 7, variant 0b10, the rest random. `crypto.getRandomValues` exists
// in every browser and, unlike `randomUUID`, outside secure contexts too.
export function mintAttemptNonce() {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let ms = Date.now();
  for (let i = 5; i >= 0; i -= 1) {
    bytes[i] = ms % 256;
    ms = Math.floor(ms / 256);
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x70;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function getOrCreateAttemptNonce() {
  try {
    if (typeof window !== "undefined" && window.sessionStorage) {
      const stored = window.sessionStorage.getItem(ATTEMPT_NONCE_KEY);
      if (typeof stored === "string" && stored.trim().length > 0) {
        return stored.trim();
      }
      const minted = mintAttemptNonce();
      window.sessionStorage.setItem(ATTEMPT_NONCE_KEY, minted);
      inMemoryAttemptNonce = minted;
      return minted;
    }
  } catch {
    // sessionStorage unavailable or blocked; fall back to in-memory nonce.
  }
  if (!inMemoryAttemptNonce) {
    inMemoryAttemptNonce = mintAttemptNonce();
  }
  return inMemoryAttemptNonce;
}

export function rotateAttemptNonce() {
  const next = mintAttemptNonce();
  inMemoryAttemptNonce = next;
  try {
    if (typeof window !== "undefined" && window.sessionStorage) {
      window.sessionStorage.setItem(ATTEMPT_NONCE_KEY, next);
    }
  } catch {
    // sessionStorage unavailable; in-memory nonce is updated.
  }
  return next;
}

export function extractAttributionMetadata(search) {
  const explicitSearch = typeof search === "string";
  const out = explicitSearch ? {} : readStoredAttribution();
  let rawSearch = search;
  if (!explicitSearch) {
    try {
      rawSearch = window.location.search || "";
    } catch {
      rawSearch = "";
    }
  }
  let params;
  try {
    params = new URLSearchParams(rawSearch);
  } catch {
    return out;
  }
  for (const key of ATTRIBUTION_ALLOWLIST) {
    const raw = params.get(key);
    if (typeof raw !== "string") continue;
    const trimmed = raw.trim();
    if (!trimmed) continue;
    out[key] = trimmed.slice(0, MAX_UTM_VALUE_LENGTH);
  }
  if (!explicitSearch) {
    writeStoredAttribution(out);
  }
  return out;
}

export function appendAttributionParams(rawUrl, attribution) {
  if (
    typeof rawUrl !== "string" ||
    !rawUrl ||
    !attribution ||
    typeof attribution !== "object"
  ) {
    return rawUrl;
  }
  const entries = ATTRIBUTION_ALLOWLIST.filter(
    (k) => typeof attribution[k] === "string" && attribution[k].trim().length > 0,
  ).map((k) => [k, attribution[k].trim().slice(0, MAX_UTM_VALUE_LENGTH)]);
  if (entries.length === 0) {
    return rawUrl;
  }
  try {
    const tokenized = rawUrl.split(SESSION_PLACEHOLDER).join(SESSION_PLACEHOLDER_TOKEN);
    const parsed = new URL(tokenized);
    for (const [k, v] of entries) {
      if (!parsed.searchParams.has(k)) {
        parsed.searchParams.set(k, v);
      }
    }
    return parsed.toString().split(SESSION_PLACEHOLDER_TOKEN).join(SESSION_PLACEHOLDER);
  } catch {
    return rawUrl;
  }
}

export function resolveReturnUrl(rawUrl, allowedOrigins, explicitLocationHref) {
  if (
    typeof rawUrl !== "string" ||
    !rawUrl ||
    !Array.isArray(allowedOrigins) ||
    allowedOrigins.length === 0
  ) {
    return rawUrl;
  }
  const locationHref =
    typeof explicitLocationHref === "string"
      ? explicitLocationHref
      : typeof window !== "undefined" && typeof window.location?.href === "string"
        ? window.location.href
        : "";
  if (!locationHref.startsWith("https://")) {
    return rawUrl;
  }
  const cleanBases = allowedOrigins
    .filter((entry) => isHttpsUrl(entry) && !entry.includes("?") && !entry.includes("#"))
    .map((entry) => entry.replace(/\/+$/, ""))
    .sort((a, b) => b.length - a.length);
  if (cleanBases.length === 0) {
    return rawUrl;
  }
  const matchesBaseBoundary = (url, base) =>
    url === base ||
    url.startsWith(`${base}/`) ||
    url.startsWith(`${base}?`) ||
    url.startsWith(`${base}#`);
  const matchedCurrent = cleanBases.find((base) => matchesBaseBoundary(locationHref, base));
  if (!matchedCurrent) {
    return rawUrl;
  }
  const matchedSource = cleanBases.find((base) => matchesBaseBoundary(rawUrl, base));
  if (matchedSource) {
    return `${matchedCurrent}${rawUrl.slice(matchedSource.length)}`;
  }
  try {
    const tokenized = rawUrl.split(SESSION_PLACEHOLDER).join(SESSION_PLACEHOLDER_TOKEN);
    const parsed = new URL(tokenized);
    if (parsed.protocol !== "https:") return rawUrl;
    return `${matchedCurrent}${parsed.pathname}${parsed.search}${parsed.hash}`
      .split(SESSION_PLACEHOLDER_TOKEN)
      .join(SESSION_PLACEHOLDER);
  } catch {
    return rawUrl;
  }
}

function queryAll(node, selector) {
  if (!node || typeof node.querySelectorAll !== "function") {
    return [];
  }
  return Array.from(node.querySelectorAll(selector));
}

export async function deriveIdempotencyKey(attemptFields, explicitNonce) {
  const nonce =
    typeof explicitNonce === "string" && explicitNonce.trim().length > 0
      ? explicitNonce.trim()
      : getOrCreateAttemptNonce();
  const canonical = JSON.stringify({ fields: attemptFields, nonce });
  try {
    const cached = window.sessionStorage.getItem(`${ATTEMPT_STORAGE_PREFIX}${canonical}`);
    if (typeof cached === "string" && cached.length > 0) {
      return cached;
    }
  } catch {
    // Continue to deterministic digest computation.
  }
  let key;
  if (
    typeof crypto !== "undefined" &&
    crypto.subtle &&
    typeof crypto.subtle.digest === "function" &&
    typeof TextEncoder !== "undefined"
  ) {
    const bytes = new TextEncoder().encode(canonical);
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    const hex = Array.from(new Uint8Array(digest))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    key = `keel-idem-${hex}`;
  } else {
    let h1 = 0x811c9dc5;
    let h2 = 0x01000193;
    for (let i = 0; i < canonical.length; i += 1) {
      const code = canonical.charCodeAt(i);
      h1 = Math.imul(h1 ^ code, 0x01000193) >>> 0;
      h2 = Math.imul(h2 ^ (code + i), 0x85ebca6b) >>> 0;
    }
    key = `keel-idem-${h1.toString(16).padStart(8, "0")}${h2.toString(16).padStart(8, "0")}-${canonical.length}`;
  }
  try {
    window.sessionStorage.setItem(`${ATTEMPT_STORAGE_PREFIX}${canonical}`, key);
  } catch {
    // Deterministic digest already guarantees reload/tab stability.
  }
  return key;
}

export function isPublishableKey(key) {
  if (typeof key !== "string") return false;
  const trimmed = key.trim();
  if (!trimmed || trimmed !== key || /\s/.test(trimmed)) return false;
  if (
    trimmed.startsWith("sk_") ||
    trimmed.startsWith("rk_") ||
    trimmed.startsWith("sylphx_sk_")
  ) {
    return false;
  }
  return (
    (trimmed.startsWith("sylphx_pk_") && trimmed.length > "sylphx_pk_".length) ||
    (trimmed.startsWith("pk_") && trimmed.length > "pk_".length)
  );
}

export function isHttpsUrl(value) {
  if (typeof value !== "string" || /\s/.test(value)) return false;
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:";
  } catch {
    return false;
  }
}

export function validParent(parent) {
  if (typeof parent !== "string" || /\s/.test(parent)) return false;
  const parts = parent.split("/");
  return (
    parts.length === 6 &&
    parts[0] === "orgs" &&
    parts[2] === "projects" &&
    parts[4] === "envs" &&
    parts[1].length > 0 &&
    parts[3].length > 0 &&
    parts[5].length > 0
  );
}

export function optionSelectorValue(option) {
  if (option && typeof option.id === "string" && option.id.trim().length > 0) {
    return option.id.trim();
  }
  return `${option.key}:${option.quantity}`;
}

export function validateConfig(config) {
  if (!config || typeof config !== "object") {
    return "invalid checkout configuration";
  }
  if (!isPublishableKey(config.publishable_key)) {
    return "publishable key required; secret keys are refused";
  }
  if (!isHttpsUrl(config.api_origin)) {
    return "api_origin must be an https:// URL";
  }
  if (!validParent(config.parent)) {
    return "parent must match orgs/{org}/projects/{project}/envs/{env}";
  }
  if (typeof config.product !== "string" || !config.product.trim()) {
    return "product key is required";
  }
  if (config.recovery_url && !isHttpsUrl(config.recovery_url)) {
    return "recovery_url must be an https:// URL";
  }
  if (config.allowed_origins !== undefined) {
    if (
      !Array.isArray(config.allowed_origins) ||
      config.allowed_origins.some(
        (origin) => !isHttpsUrl(origin) || origin.includes("?") || origin.includes("#"),
      )
    ) {
      return "allowed_origins entries must be https:// base URLs without query or fragment";
    }
  }
  if (config.checkout_enabled === false) {
    if (!isHttpsUrl(config.recovery_url || config.success_url)) {
      return "recovery_url (or success_url) must be an https:// URL";
    }
    return null;
  }
  if (!Array.isArray(config.prices) || config.prices.length === 0) {
    return "at least one price option is required";
  }
  const seenOptionValues = new Set();
  for (const option of config.prices) {
    if (
      !option ||
      typeof option.key !== "string" ||
      !option.key.trim() ||
      typeof option.label !== "string" ||
      !option.label.trim() ||
      !Number.isInteger(option.quantity) ||
      option.quantity < 1
    ) {
      return "every price option requires a non-empty key, label, and integer quantity >= 1";
    }
    const selectorValue = optionSelectorValue(option);
    if (seenOptionValues.has(selectorValue)) {
      return `duplicate price option selector "${selectorValue}"`;
    }
    seenOptionValues.add(selectorValue);
  }
  if (
    typeof config.consent_version !== "string" ||
    !config.consent_version.trim() ||
    typeof config.consent_text !== "string" ||
    !config.consent_text.trim()
  ) {
    return "consent_version and consent_text are required";
  }
  if (!isHttpsUrl(config.success_url) || !isHttpsUrl(config.cancel_url)) {
    return "success_url and cancel_url must be https:// URLs";
  }
  if (!config.success_url.includes("{CHECKOUT_SESSION_ID}")) {
    return "success_url must include {CHECKOUT_SESSION_ID} so the returned session can be verified";
  }
  return null;
}

function normalizePurchaseCode(source) {
  if (
    !source ||
    typeof source !== "object" ||
    typeof source.domain !== "string" ||
    (typeof source.code !== "string" && typeof source.code !== "number")
  ) {
    return null;
  }
  const code = { domain: source.domain, code: String(source.code) };
  const subcode = normalizePurchaseCode(source.subcode);
  if (subcode) code.subcode = subcode;
  return code;
}

// Purchase answers match keel-commerce's tagged PurchaseOutcome. Provider
// messages are diagnostics only; they must never determine cancellation.
function responseCode(payload, httpStatus) {
  const body = payload && typeof payload === "object" ? payload : {};
  const structured = normalizePurchaseCode(body.purchase_outcome?.code);
  if (structured) return structured;
  const error = body.error && typeof body.error === "object" ? body.error : {};
  const details = [...(Array.isArray(body.details) ? body.details : []),
    ...(Array.isArray(error.details) ? error.details : [])];
  const info = details.find((item) => item && (typeof item.code === "string" || typeof item.code === "number" || typeof item.reason === "string"));
  const source = info || (error.code != null || error.reason != null ? error : body);
  const raw =
    source.code ??
    source.reason ??
    (typeof body.error === "string" && body.error.length > 0 ? body.error : undefined) ??
    body.state ??
    body.status;
  const code = {
    domain: typeof source.domain === "string" ? source.domain : "sylphx.money",
    code: typeof raw === "number" || typeof raw === "string" ? String(raw) : "unconfirmed",
  };
  // Retain the transport wrapper as well as a structured provider error.
  if (typeof httpStatus === "number") code.subcode = { domain: "http", code: String(httpStatus) };
  return code;
}

function responseState(payload) {
  return payload?.purchase_outcome?.status ?? payload?.outcome ?? payload?.state ?? payload?.status;
}

export function checkoutOutcome(payload, httpStatus = null, sessionId = null) {
  const code = responseCode(payload, httpStatus);
  const state = responseState(payload);
  if (payload?.paid === true && typeof sessionId === "string" && sessionId.length > 0) {
    return { status: "succeeded", code, receipt: { Store: { store: "web", data: sessionId } } };
  }
  if (state === "cancelled" || state === "canceled") return { status: "cancelled", code };
  if (state === "declined" || code.code === "payment_declined" || code.code === "card_declined") return { status: "declined", code };
  if (state === "pending" || state === "deferred" || state === "awaiting_approval" ||
      (payload?.paid === false && (state == null || state === "open"))) return { status: "pending", code };
  return { status: "unknown", code };
}

function reportPurchase(root, outcome) {
  root.dispatchEvent(new CustomEvent("keel:money:purchase", { bubbles: true, detail: outcome }));
}

function terminalSession(payload, httpStatus = null) {
  const code = responseCode(payload, httpStatus);
  const state = String(responseState(payload) || "").toLowerCase();
  return ["paid", "complete", "completed", "expired", "cancelled", "canceled", "revoked"].includes(state) ||
    ["checkout_paid", "checkout_complete", "checkout_completed", "checkout_expired", "checkout_cancelled", "checkout_canceled", "checkout_revoked"].includes(code.code) ||
    httpStatus === 410;
}

function setStatus(root, state, message) {
  root.setAttribute("data-keel-money-state", state);
  const statusNode = root.querySelector("[data-keel-money-status]");
  if (statusNode) {
    statusNode.textContent = message || "";
  }
}

export function setButtonDisabled(button, disabled) {
  if (!button) return;
  const isDisabled = Boolean(disabled);
  button.disabled = isDisabled;
  if (isDisabled) {
    if (typeof button.setAttribute === "function") {
      button.setAttribute("disabled", "");
      button.setAttribute("aria-disabled", "true");
      button.setAttribute("data-keel-state", "disabled");
    }
  } else {
    if (typeof button.removeAttribute === "function") {
      button.removeAttribute("disabled");
      button.removeAttribute("aria-disabled");
      if (
        typeof button.getAttribute !== "function" ||
        button.getAttribute("data-keel-state") === "disabled"
      ) {
        button.removeAttribute("data-keel-state");
      }
    }
  }
}

export function resolveFieldControl(root, field, controlSelector = "input, select, textarea") {
  if (!root || typeof root.querySelector !== "function") return null;
  const direct = root.querySelector(
    `input[data-keel-money-field="${field}"], select[data-keel-money-field="${field}"], textarea[data-keel-money-field="${field}"]`,
  );
  if (direct) return direct;
  const node = root.querySelector(`[data-keel-money-field="${field}"]`);
  if (!node) return null;
  const tag = String(node.tagName || node.tag || "").toLowerCase();
  if (tag === "input" || tag === "select" || tag === "textarea") {
    return node;
  }
  if (typeof node.querySelector === "function") {
    const nested = node.querySelector(controlSelector);
    if (nested) return nested;
  }
  return node;
}

export function parseMinorAmount(raw) {
  if (typeof raw === "number") {
    return Number.isSafeInteger(raw) && raw >= 0 ? raw : null;
  }
  if (typeof raw === "string" && /^\d{1,15}$/.test(raw)) {
    const parsed = Number.parseInt(raw, 10);
    return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : null;
  }
  return null;
}

const CURRENCY_STORAGE_KEY = "keel:money:currency";
export const ZERO_DECIMAL_CURRENCIES = new Set(["JPY", "KRW", "VND"]);
const THREE_DECIMAL_CURRENCIES = new Set(["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"]);
const PREFERRED_CURRENCY_ORDER = [
  "USD", "HKD", "TWD", "GBP", "EUR",
  "JPY", "KRW", "MYR", "SGD", "CAD",
  "AUD", "NZD", "THB", "PHP", "IDR", "VND",
];
const CURRENCY_OPTION_LABELS = {
  USD: "USD ($)",
  HKD: "HKD (HK$)",
  TWD: "TWD (NT$)",
  GBP: "GBP (£)",
  EUR: "EUR (€)",
  JPY: "JPY (¥)",
  KRW: "KRW (₩)",
  MYR: "MYR (RM)",
  SGD: "SGD (S$)",
  CAD: "CAD (CA$)",
  AUD: "AUD (A$)",
  NZD: "NZD (NZ$)",
  THB: "THB (฿)",
  PHP: "PHP (₱)",
  IDR: "IDR (Rp)",
  VND: "VND (₫)",
};
const REGION_TO_CURRENCY = {
  HK: "HKD", MO: "HKD",
  TW: "TWD",
  US: "USD", PR: "USD", GU: "USD", VI: "USD",
  GB: "GBP", UK: "GBP", GG: "GBP", JE: "GBP", IM: "GBP",
  AT: "EUR", BE: "EUR", CY: "EUR", DE: "EUR", EE: "EUR",
  ES: "EUR", FI: "EUR", FR: "EUR", GR: "EUR", HR: "EUR",
  IE: "EUR", IT: "EUR", LT: "EUR", LU: "EUR", LV: "EUR",
  MT: "EUR", NL: "EUR", PT: "EUR", SI: "EUR", SK: "EUR",
  AD: "EUR", MC: "EUR", SM: "EUR", VA: "EUR",
  JP: "JPY",
  KR: "KRW",
  MY: "MYR",
  SG: "SGD",
  CA: "CAD",
  AU: "AUD",
  NZ: "NZD",
  TH: "THB",
  PH: "PHP",
  ID: "IDR",
  VN: "VND",
};
const TIMEZONE_TO_CURRENCY = {
  "Asia/Hong_Kong": "HKD",
  "Asia/Macau": "HKD",
  "Asia/Taipei": "TWD",
  "Europe/London": "GBP",
  "Asia/Tokyo": "JPY",
  "Asia/Seoul": "KRW",
  "Asia/Kuala_Lumpur": "MYR",
  "Asia/Kuching": "MYR",
  "Asia/Singapore": "SGD",
  "Asia/Bangkok": "THB",
  "Asia/Manila": "PHP",
  "Asia/Jakarta": "IDR",
  "Asia/Makassar": "IDR",
  "Asia/Jayapura": "IDR",
  "Asia/Ho_Chi_Minh": "VND",
  "Asia/Saigon": "VND",
  "Pacific/Auckland": "NZD",
  "Australia/Sydney": "AUD",
  "Australia/Melbourne": "AUD",
  "Australia/Brisbane": "AUD",
  "Australia/Perth": "AUD",
  "Australia/Adelaide": "AUD",
};

export function currencyExponent(currency) {
  if (typeof currency !== "string") return 2;
  const code = currency.trim().toUpperCase();
  if (ZERO_DECIMAL_CURRENCIES.has(code)) return 0;
  return THREE_DECIMAL_CURRENCIES.has(code) ? 3 : 2;
}

export function detectPreferredCurrencies() {
  const out = [];
  const add = (code) => {
    if (code && !out.includes(code)) out.push(code);
  };
  try {
    const nav = typeof window !== "undefined" ? window.navigator : undefined;
    const langs = Array.isArray(nav?.languages) && nav.languages.length > 0
      ? nav.languages
      : typeof nav?.language === "string" && nav.language
        ? [nav.language]
        : [];
    for (const tag of langs) {
      if (typeof tag !== "string") continue;
      let region = "";
      try {
        if (typeof Intl?.Locale === "function") {
          region = new Intl.Locale(tag).maximize().region || "";
        }
      } catch {
        region = "";
      }
      if (!region) {
        const m = tag.match(/[-_]([A-Za-z]{2})(?:$|[-_])/);
        if (m) region = m[1].toUpperCase();
      }
      const mapped = REGION_TO_CURRENCY[region.toUpperCase()];
      if (mapped) add(mapped);
    }
    const tz = Intl?.DateTimeFormat?.()?.resolvedOptions?.()?.timeZone;
    if (typeof tz === "string" && TIMEZONE_TO_CURRENCY[tz]) {
      add(TIMEZONE_TO_CURRENCY[tz]);
    }
  } catch {
    // Locale inspection unavailable; fall back to catalog default.
  }
  return out;
}

export function formatMinorAmount(rawAmount, currency, quantity = 1) {
  // Keep the catalog parser's accepted input forms; computed totals and per-seat
  // amounts can additionally arrive as exact integers beyond Number's range.
  const unitMinor = typeof rawAmount === "bigint"
    ? (rawAmount >= 0n ? rawAmount : null)
    : parseMinorAmount(rawAmount);
  if (
    unitMinor === null ||
    typeof currency !== "string" ||
    !currency.trim() ||
    !Number.isSafeInteger(quantity) ||
    quantity < 1
  ) {
    return null;
  }
  const totalMinor = BigInt(unitMinor) * BigInt(quantity);
  const code = currency.trim().toUpperCase();
  const exponent = currencyExponent(code);
  const scale = 10n ** BigInt(exponent);
  const fraction = totalMinor % scale;
  const minimumFractionDigits = fraction === 0n ? 0 : exponent;
  const whole = totalMinor / scale;
  const value = fraction === 0n
    ? String(whole)
    : `${whole}.${String(fraction).padStart(exponent, "0")}`;
  try {
    // Intl accepts a decimal string without a lossy conversion to Number.
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: code,
      minimumFractionDigits,
      maximumFractionDigits: exponent,
    }).format(value);
  } catch {
    return `${value} ${code}`;
  }
}

export function supplementalUnitNote(rawNote, unitWord = "seat") {
  if (typeof rawNote !== "string" || !rawNote.trim()) return "";
  const rateTail = `(?:\\/\\s*${unitWord}s?\\b|\\bper\\s+${unitWord}s?\\b)(?:\\s*(?:\\/|\\bper\\b)\\s*[A-Za-z]+)*`;
  const parenRate = new RegExp(`\\s*\\([^()]*\\d[^()]*?${rateTail}[^()]*\\)`, "gi");
  const leadingRate = new RegExp(
    `^[^()·•|—–,;]*\\d[^()·•|—–]*?${rateTail}\\s*[,;:]?\\s*`,
    "i",
  );
  const trailingRate = new RegExp(
    `\\s*[,;:]\\s*[^()·•|—–,;]*\\d[^()·•|—–]*?${rateTail}\\s*$`,
    "i",
  );
  const embeddedRate = new RegExp(`\\d[^()·•|—–]*?${rateTail}`, "i");
  const bareRate = new RegExp(`^${rateTail}$`, "i");
  return rawNote
    .split(/\s*(?:[·•|]|—|–|\s-\s)\s*/)
    .map((part) =>
      part
        .replace(parenRate, "")
        .replace(leadingRate, "")
        .replace(trailingRate, "")
        .trim(),
    )
    .filter((part) => part.length > 0 && !embeddedRate.test(part) && !bareRate.test(part))
    .join(" · ");
}

async function hydrateCatalog(
  root,
  config,
  allowedOptions,
  priceSelect,
  priceDisplay,
  submitButton,
) {
  const origin = config.api_origin.replace(/\/+$/, "");
  const url = `${origin}/v1/money/${config.parent}/catalog`;
  try {
    const response = await fetch(url, {
      method: "GET",
      headers: {
        Authorization: `Bearer ${config.publishable_key}`,
        Accept: "application/json",
      },
    });
    if (!response.ok) {
      setButtonDisabled(submitButton, true);
      setStatus(
        root,
        "error",
        "Live catalog pricing is currently unavailable. Checkout is disabled.",
      );
      return false;
    }
    const catalog = await response.json();
    const products = Array.isArray(catalog?.spec?.products)
      ? catalog.spec.products
      : Array.isArray(catalog?.products)
        ? catalog.products
        : [];
    const product = products.find((item) => item && item.key === config.product);
    if (!product) {
      setButtonDisabled(submitButton, true);
      setStatus(root, "error", "Selected product is not available in the active catalog.");
      return false;
    }
    const catalogPrices = Array.isArray(product.prices) ? product.prices : [];
    const priceByKey = new Map(
      catalogPrices
        .filter((item) => item && !item.archived && typeof item.key === "string")
        .map((item) => [item.key, item]),
    );
    const resolvedEntries = [];
    for (const configured of config.prices) {
      const price = priceByKey.get(configured.key);
      if (!price || !price.unit_amounts || typeof price.unit_amounts !== "object") {
        setButtonDisabled(submitButton, true);
        setStatus(
          root,
          "error",
          `Price "${configured.key}" is not available in the active catalog.`,
        );
        return false;
      }
      const amounts = new Map();
      for (const [rawCode, rawVal] of Object.entries(price.unit_amounts)) {
        const code = String(rawCode || "").trim().toUpperCase();
        if (!code || parseMinorAmount(rawVal) === null) continue;
        amounts.set(code, rawVal);
      }
      const defaultCurrency =
        typeof price.default_currency === "string" && price.default_currency.trim()
          ? price.default_currency.trim().toUpperCase()
          : "";
      if (defaultCurrency && !amounts.has(defaultCurrency)) {
        setButtonDisabled(submitButton, true);
        setStatus(
          root,
          "error",
          `Price "${configured.key}" has no valid catalog unit amount.`,
        );
        return false;
      }
      if (amounts.size === 0) {
        setButtonDisabled(submitButton, true);
        setStatus(
          root,
          "error",
          `Price "${configured.key}" has no valid catalog unit amount.`,
        );
        return false;
      }
      const fallbackCurrency =
        [defaultCurrency, ...amounts.keys()].find(
          (code) =>
            code &&
            amounts.has(code) &&
            formatMinorAmount(amounts.get(code), code, configured.quantity),
        ) ||
        defaultCurrency ||
        amounts.keys().next().value;
      resolvedEntries.push({ configured, price, amounts, defaultCurrency, fallbackCurrency });
    }

    const rawCatalogKeys = new Set();
    for (const entry of resolvedEntries) {
      for (const code of entry.amounts.keys()) {
        rawCatalogKeys.add(code);
      }
    }
    if (rawCatalogKeys.size === 0) {
      setButtonDisabled(submitButton, true);
      setStatus(
        root,
        "error",
        "Configured prices do not expose any valid catalog currency.",
      );
      return false;
    }

    const buildCurrencyEntries = (currency) => {
      if (!currency) return null;
      const nextEntries = [];
      for (const { configured, price, amounts, fallbackCurrency } of resolvedEntries) {
        const entryCurrency = amounts.has(currency) ? currency : fallbackCurrency;
        const rawMinor = amounts.get(entryCurrency);
        const formatted = formatMinorAmount(rawMinor, entryCurrency, configured.quantity);
        if (!formatted) return null;
        const parsedMinor = parseMinorAmount(rawMinor);
        const unitFormatted = formatMinorAmount(rawMinor, entryCurrency, 1);
        const seatCount =
          Number.isSafeInteger(configured.seats) && configured.seats >= 1
            ? configured.seats
            : null;
        const perSeatFormatted =
          seatCount && parsedMinor !== null
            ? formatMinorAmount(
                (BigInt(parsedMinor) * BigInt(configured.quantity) + BigInt(seatCount) / 2n)
                  / BigInt(seatCount),
                entryCurrency,
                1,
              )
            : unitFormatted;
        const interval = price.recurring_interval ? ` / ${price.recurring_interval}` : "";
        nextEntries.push([
          optionSelectorValue(configured),
          {
            ...configured,
            currency_code: entryCurrency,
            formatted_total: formatted,
            formatted_unit: unitFormatted,
            formatted_per_seat: perSeatFormatted,
            interval,
          },
        ]);
      }
      return nextEntries;
    };

    const catalogKeys = new Set();
    for (const code of rawCatalogKeys) {
      if (buildCurrencyEntries(code)) {
        catalogKeys.add(code);
      }
    }
    if (catalogKeys.size === 0) {
      setButtonDisabled(submitButton, true);
      setStatus(
        root,
        "error",
        "Could not format active catalog pricing.",
      );
      return false;
    }

    const availableCurrencies = [];
    for (const code of PREFERRED_CURRENCY_ORDER) {
      if (catalogKeys.has(code)) availableCurrencies.push(code);
    }
    for (const code of catalogKeys) {
      if (!availableCurrencies.includes(code)) availableCurrencies.push(code);
    }

    let queryCurrency = "";
    try {
      const params = new URLSearchParams(window?.location?.search || "");
      const raw = params.get("currency");
      if (raw && raw.trim()) queryCurrency = raw.trim().toUpperCase();
    } catch {
      queryCurrency = "";
    }
    let storedCurrency = "";
    try {
      const saved = window?.localStorage?.getItem(CURRENCY_STORAGE_KEY);
      if (saved && saved.trim()) storedCurrency = saved.trim().toUpperCase();
    } catch {
      storedCurrency = "";
    }
    const localeCandidates = detectPreferredCurrencies();
    const sharedDefaultCurrency = resolvedEntries[0]?.defaultCurrency || "";

    const currencyWrap =
      typeof root.querySelector === "function"
        ? root.querySelector("[data-keel-money-currency-wrap]")
        : null;
    const currencySelect = resolveFieldControl(root, "currency", "select");

    const applyCurrency = (rawCurrency) => {
      const currency = String(rawCurrency || "").trim().toUpperCase();
      if (!currency || !catalogKeys.has(currency)) return false;
      const nextEntries = buildCurrencyEntries(currency);
      if (!nextEntries) return false;
      for (const [selector, optionEntry] of nextEntries) {
        allowedOptions.set(selector, optionEntry);
      }
      if (priceSelect && typeof priceSelect.querySelectorAll === "function") {
        for (const optionEl of priceSelect.querySelectorAll("option")) {
          const optionConfig = allowedOptions.get(optionEl.value);
          if (!optionConfig || !optionConfig.formatted_total) continue;
          const baseLabel = optionEl.getAttribute("data-keel-label") || optionConfig.label;
          optionEl.textContent = baseLabel.includes(optionConfig.formatted_total)
            ? baseLabel
            : `${baseLabel} — ${optionConfig.formatted_total}${optionConfig.interval || ""}`;
        }
      }
      for (const card of queryAll(root, "[data-keel-money-tier-card]")) {
        const selector = card.getAttribute("data-keel-money-tier-card") || "";
        const optionConfig = allowedOptions.get(selector);
        if (!optionConfig) continue;
        const priceEl =
          typeof card.querySelector === "function"
            ? card.querySelector("[data-keel-money-tier-price]")
            : null;
        if (priceEl && optionConfig.formatted_total) {
          priceEl.textContent = `${optionConfig.formatted_total}${optionConfig.interval || ""}`;
        }
        const unitEl =
          typeof card.querySelector === "function"
            ? card.querySelector("[data-keel-money-tier-unit]")
            : null;
        if (unitEl) {
          if (optionConfig.seats && optionConfig.formatted_per_seat) {
            const note = supplementalUnitNote(optionConfig.unit_note, "seat");
            unitEl.textContent = note
              ? `${optionConfig.formatted_per_seat} / seat · ${note}`
              : `${optionConfig.formatted_per_seat} / seat`;
          } else if (
            optionConfig.quantity > 1 &&
            optionConfig.formatted_unit &&
            optionConfig.unit_note
          ) {
            const note = supplementalUnitNote(optionConfig.unit_note, "unit");
            unitEl.textContent = note
              ? `${optionConfig.formatted_unit} / unit · ${note}`
              : `${optionConfig.formatted_unit} / unit`;
          } else if (optionConfig.unit_note) {
            unitEl.textContent = optionConfig.unit_note;
          }
        }
      }
      root.setAttribute("data-keel-money-currency", currency);
      if (currencySelect) {
        currencySelect.value = currency;
      }
      syncTierSelection(root, config, allowedOptions, priceSelect, priceDisplay, submitButton);
      return true;
    };

    const initialCandidates = [
      queryCurrency,
      storedCurrency,
      ...localeCandidates,
      sharedDefaultCurrency,
      ...availableCurrencies,
    ];
    let initialCurrency = "";
    for (const candidate of initialCandidates) {
      if (candidate && catalogKeys.has(candidate) && applyCurrency(candidate)) {
        initialCurrency = candidate;
        break;
      }
    }
    if (!initialCurrency) {
      setButtonDisabled(submitButton, true);
      setStatus(
        root,
        "error",
        "Could not format active catalog pricing.",
      );
      return false;
    }

    const selectCurrency = (rawCurrency) => {
      const chosen = String(rawCurrency || "").trim().toUpperCase();
      const prevCurrency = root.getAttribute("data-keel-money-currency") || initialCurrency;
      if (!applyCurrency(chosen)) {
        if (currencySelect) currencySelect.value = prevCurrency;
        return false;
      }
      try {
        window?.localStorage?.setItem(CURRENCY_STORAGE_KEY, chosen);
      } catch {
        // Storage unavailable; active session still uses selected currency.
      }
      return true;
    };

    if (currencySelect) {
      if (typeof document !== "undefined" && typeof document.createElement === "function") {
        const optionNodes = availableCurrencies.map((code) => {
          const opt = document.createElement("option");
          opt.value = code;
          opt.textContent = CURRENCY_OPTION_LABELS[code] || code;
          opt.selected = code === initialCurrency;
          return opt;
        });
        if (typeof currencySelect.replaceChildren === "function") {
          currencySelect.replaceChildren(...optionNodes);
        } else {
          currencySelect.innerHTML = "";
          for (const opt of optionNodes) currencySelect.appendChild?.(opt);
        }
      }
      currencySelect.value = initialCurrency;
      if (typeof currencySelect.addEventListener === "function") {
        currencySelect.addEventListener("change", () => {
          selectCurrency(currencySelect.value);
        });
      }
    }
    if (currencyWrap) {
      if (availableCurrencies.length > 1) {
        currencyWrap.removeAttribute?.("hidden");
      } else {
        currencyWrap.setAttribute?.("hidden", "");
      }
    }
    root.__keelSelectCurrency = selectCurrency;

    root.setAttribute("data-keel-money-catalog-ready", "1");
    setButtonDisabled(submitButton, false);
    return true;
  } catch {
    setButtonDisabled(submitButton, true);
    setStatus(
      root,
      "error",
      "Could not load live catalog pricing. Checkout is disabled.",
    );
    return false;
  }
}

export const REDEEM_POLL_INTERVAL_MS = 1500;
export const REDEEM_MAX_ATTEMPTS = 14;
const REDEEM_TIMEOUT_MS = 20_000;
const LICENCE_STORAGE_PREFIX = "keel:money:licence:";

// Decoding is only a display hint: the activation consumer verifies signatures.
export function decodeLicenceClaims(token) {
  try {
    if (typeof token !== "string") return null;
    const parts = token.trim().split(".");
    const raw = parts.length === 2 ? parts[0] : parts.length === 3 ? parts[1] : null;
    if (!raw) return null;
    const segment = raw.replace(/-/g, "+").replace(/_/g, "/");
    const padded = segment.padEnd(segment.length + ((4 - (segment.length % 4)) % 4), "=");
    const bytes = Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
    const claims = JSON.parse(new TextDecoder().decode(bytes));
    return claims && typeof claims === "object" ? claims : null;
  } catch { return null; }
}

function presentLicence(root, config, token, autoActivate = false) {
  const product = config.token_product || config.product;
  const tokenNode = root.querySelector("[data-keel-money-licence-token]");
  if (tokenNode) {
    tokenNode.textContent = token;
    tokenNode.removeAttribute("hidden");
  }
  const emit = (name) => {
    if (typeof CustomEvent === "function" && typeof root.dispatchEvent === "function") {
      root.dispatchEvent(new CustomEvent(name, { bubbles: true, detail: { product, token } }));
    }
  };
  const copy = root.querySelector("[data-keel-money-copy-token]");
  if (copy) {
    copy.removeAttribute("hidden");
    copy.onclick = async () => {
      try {
        if (!globalThis.navigator?.clipboard?.writeText) return;
        await navigator.clipboard.writeText(token);
        copy.textContent = "Copied!";
        copy.setAttribute("data-keel-money-copied", "1");
      } catch {
        setStatus(root, root.getAttribute("data-keel-money-state"), "Clipboard unavailable. Select and copy your licence key below.");
      }
    };
  }
  const hasBrowserActivator =
    root.getAttribute("data-keel-money-browser-activator") === "1" ||
    Boolean(typeof window !== "undefined" && window.__keelMoneyBrowserActivator);
  const activate = root.querySelector("[data-keel-money-activate-browser]");
  if (activate) {
    if (hasBrowserActivator) {
      activate.removeAttribute("hidden");
      activate.onclick = () => emit("keel:money:activate");
    } else {
      activate.setAttribute("hidden", "");
    }
  }
  const cliNode = root.querySelector("[data-keel-money-cli-activate]");
  if (cliNode) {
    cliNode.textContent = `${product} pro activate '${token}'`;
    cliNode.removeAttribute("hidden");
  }
  const claims = decodeLicenceClaims(token);
  if (claims && (!claims.product || claims.product === product)) {
    try { window.localStorage.setItem(`${LICENCE_STORAGE_PREFIX}${product}`, JSON.stringify({ ...claims, product, token })); }
    catch { /* Storage can be blocked; activation still works. */ }
  }
  emit("keel:money:licence-ready");
  if (autoActivate) emit("keel:money:activate");
}

function optionSeats(price) {
  return Number.isInteger(price?.seats) && price.seats >= 1
    ? price.seats
    : price?.quantity;
}

export function heldPriceIndex(prices, claims) {
  const explicit = prices.findIndex(
    (p) =>
      optionSelectorValue(p) === claims.optionId ||
      optionSelectorValue(p) === claims.optionKey,
  );
  if (explicit >= 0) return explicit;
  const hasPriceKey =
    typeof claims.priceKey === "string" && claims.priceKey.length > 0;
  if (typeof claims.seats === "number" && claims.seats >= 1) {
    if (hasPriceKey) {
      const exactByKey = prices.findIndex(
        (p) => p.key === claims.priceKey && optionSeats(p) === claims.seats,
      );
      if (exactByKey >= 0) return exactByKey;
    }
    const exact = prices.findIndex((p) => optionSeats(p) === claims.seats);
    if (exact >= 0) return exact;
    let held = -1;
    for (let i = 0; i < prices.length; i += 1) {
      const seats = optionSeats(prices[i]);
      if (typeof seats !== "number" || seats > claims.seats) continue;
      if (held < 0) {
        held = i;
        continue;
      }
      const curSeats = optionSeats(prices[held]);
      const nextRank = prices[i].rank;
      const curRank = prices[held].rank;
      if (nextRank != null || curRank != null) {
        if ((nextRank ?? 0) > (curRank ?? 0)) held = i;
        continue;
      }
      const nextKeyMatch = hasPriceKey && prices[i].key === claims.priceKey;
      const curKeyMatch = hasPriceKey && prices[held].key === claims.priceKey;
      if (nextKeyMatch !== curKeyMatch) {
        if (nextKeyMatch) held = i;
        continue;
      }
      if (seats > curSeats || (seats === curSeats && i > held)) {
        held = i;
      }
    }
    return held;
  }
  const byPrice = hasPriceKey
    ? prices.findIndex((p) => p.key === claims.priceKey)
    : -1;
  if (byPrice >= 0) return byPrice;
  return prices.length ? 0 : -1;
}

function setCheckoutControlsHidden(root, form, hidden) {
  const toggleAttr = (el) => {
    if (!el) return;
    if (hidden) el.setAttribute("hidden", "");
    else el.removeAttribute("hidden");
  };
  toggleAttr(form);
  if (root && typeof root.querySelector === "function") {
    toggleAttr(root.querySelector("[data-keel-money-tier-grid]"));
    toggleAttr(root.querySelector("[data-keel-money-billing-toggle]"));
  }
}

function syncTierSelection(
  root,
  config,
  allowedOptions,
  priceSelect,
  priceDisplay,
  submitButton,
) {
  const selectedValue = priceSelect
    ? priceSelect.value
    : Array.isArray(config?.prices) && config.prices.length > 0
      ? optionSelectorValue(config.prices[0])
      : "";
  const selectedConfig = allowedOptions.get(selectedValue);
  if (priceDisplay) {
    if (priceSelect && priceSelect.selectedOptions && priceSelect.selectedOptions.length > 0) {
      priceDisplay.textContent = priceSelect.selectedOptions[0].textContent;
    } else if (selectedConfig) {
      const total = selectedConfig.formatted_total
        ? ` — ${selectedConfig.formatted_total}${selectedConfig.interval || ""}`
        : "";
      priceDisplay.textContent = `${selectedConfig.label}${total}`;
    }
  }
  for (const card of queryAll(root, "[data-keel-money-tier-card]")) {
    const active = card.getAttribute("data-keel-money-tier-card") === selectedValue;
    card.setAttribute("aria-checked", active ? "true" : "false");
    card.setAttribute("data-keel-selected", active ? "true" : "false");
  }
  if (submitButton && selectedConfig?.formatted_total && config?.cta_label) {
    submitButton.textContent = `${config.cta_label} — ${selectedConfig.formatted_total}${selectedConfig.interval || ""}`;
  }
}

function showOwnedTier(root, config, form, submitButton) {
  if (root.hasAttribute("data-keel-money-return-session")) return;
  let claims;
  try {
    claims = JSON.parse(root.getAttribute("data-keel-money-owned-claims") ||
      window.localStorage.getItem(`${LICENCE_STORAGE_PREFIX}${config.token_product || config.product}`) || "null");
  } catch { return; }
  if (!claims || claims.product !== (config.token_product || config.product)) return;
  const expiry = claims.expiresAt;
  if (expiry != null && (typeof expiry !== "number" || !Number.isFinite(expiry) || expiry <= Date.now())) return;
  const prices = config.prices || [];
  const heldIndex = heldPriceIndex(prices, claims);
  if (heldIndex < 0) return;
  const held = prices[heldIndex];
  const label = claims.label || held.label || config.title;
  setCheckoutControlsHidden(root, form, true);
  submitButton?.setAttribute("hidden", "");
  setButtonDisabled(submitButton, true);
  root.setAttribute("data-keel-money-owned", "1");
  setStatus(root, "owned", `You already own ${label}.`);
  if (typeof claims.token === "string" && claims.token.trim()) {
    presentLicence(root, config, claims.token);
  }
}

async function redeemReturnedSession(root, config) {
  let params;
  try {
    params = new URLSearchParams(window.location.search);
  } catch {
    return;
  }
  const sessionId = params.get("session_id") || params.get("checkout_session");
  if (!sessionId || !sessionId.startsWith("cs_")) return;

  const checkoutForm = root.querySelector('[data-keel-money-form="checkout"]');
  setCheckoutControlsHidden(root, checkoutForm, true);
  root.setAttribute("data-keel-money-return-session", "1");
  setStatus(root, "redeeming", "Confirming your payment…");
  let claimSecret = null;
  try {
    const stored = window.sessionStorage.getItem(`${CLAIM_STORAGE_PREFIX}${sessionId}`);
    if (typeof stored === "string" && stored.trim().length > 0) {
      claimSecret = stored.trim();
    }
  } catch {
    claimSecret = null;
  }

  const origin = config.api_origin.replace(/\/+$/, "");
  const payload = { checkout_session: sessionId };
  if (claimSecret) {
    payload.claim_secret = claimSecret;
  }

  const tokenNode = root.querySelector("[data-keel-money-licence-token]");
  if (tokenNode) {
    tokenNode.textContent = "";
    tokenNode.setAttribute("hidden", "");
  }

  const controller = new AbortController();
  const deadline = Date.now() + REDEEM_TIMEOUT_MS;
  const timeout = setTimeout(() => controller.abort(), REDEEM_TIMEOUT_MS);
  const showPending = () => setStatus(root, "pending",
    "Payment is still being confirmed. Check back in a minute or use licence recovery below." +
    (config.trader?.contact ? ` Need help? Contact ${config.trader.contact}.` : ""));
  const showRetry = () => {
    rotateAttemptNonce();
    setCheckoutControlsHidden(root, checkoutForm, false);
    setStatus(root, "error", "This checkout session is no longer active. You can start a new checkout below.");
  };
  try {
    for (let attempt = 0; attempt < REDEEM_MAX_ATTEMPTS; attempt += 1) {
      if (controller.signal.aborted) {
        reportPurchase(root, { status: "unknown", code: { domain: "keel.checkout", code: "timeout" } });
        showPending();
        return;
      }
      const response = await fetch(`${origin}/v1/money/checkout_sessions:redeem`, {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify(payload),
        signal: controller.signal,
      });
      if (!response.ok) {
        let errorPayload = null;
        try {
          errorPayload = await response.json();
        } catch {
          errorPayload = null;
        }
        const outcome = checkoutOutcome(errorPayload, response.status, sessionId);
        if (terminalSession(errorPayload, response.status) || outcome.status === "declined") {
          showRetry();
          reportPurchase(root, outcome);
          return;
        }
        reportPurchase(root, outcome);
        setStatus(
          root,
          "error",
          "Could not verify checkout session. Use licence recovery below if you already completed payment.",
        );
        return;
      }
      const result = await response.json();
      if (!result || result.paid !== true) {
        const outcome = checkoutOutcome(result, response.status, sessionId);
        // A definitive refusal ends redemption, but must not make clientCreate
        // automatically replace a declined payment like an expired session.
        if (terminalSession(result) || outcome.status === "declined") {
          showRetry();
          reportPurchase(root, outcome);
          return;
        }
        if (attempt + 1 < REDEEM_MAX_ATTEMPTS) {
          const delay = Math.max(0, Math.min(REDEEM_POLL_INTERVAL_MS, deadline - Date.now()));
          await new Promise((resolve) => setTimeout(resolve, delay));
          continue;
        }
        reportPurchase(root, outcome);
        showPending();
        return;
      }
      reportPurchase(root, checkoutOutcome(result, response.status, sessionId));
      rotateAttemptNonce();
      const tokens = Array.isArray(result.licence_tokens) ? result.licence_tokens : [];
      // Never render a licence token without the browser-bound claim_secret from
      // sessionStorage, even if a response payload unexpectedly includes one.
      if (claimSecret && tokens.length > 0 && tokens[0].token && tokenNode) {
        const hasBrowserActivator =
          root.getAttribute("data-keel-money-browser-activator") === "1" ||
          Boolean(typeof window !== "undefined" && window.__keelMoneyBrowserActivator);
        const cliProduct = config.token_product || config.product;
        setStatus(
          root,
          "paid",
          hasBrowserActivator
            ? "Payment verified. Copy your offline licence key below or activate in this browser."
            : `Payment verified. Copy your offline licence key below and run: ${cliProduct} pro activate '<your-token>'`,
        );
        presentLicence(root, config, tokens[0].token, true);
      } else {
        setStatus(
          root,
          "paid",
          "Payment verified. Your licence key has been sent to your email address, or you can resend it below.",
        );
      }
      return;
    }
  } catch {
    reportPurchase(root, { status: "unknown", code: { domain: "keel.checkout", code: controller.signal.aborted ? "timeout" : "network_error" } });
    if (controller.signal.aborted) showPending();
    else setStatus(root, "error", "Network error while verifying checkout session.");
  } finally {
    clearTimeout(timeout);
  }
}

async function redeemRecoveryLink(root, config) {
  let params;
  try {
    params = new URLSearchParams(window.location.search);
  } catch {
    return;
  }
  const recoveryToken = params.get("recovery_token");
  if (!recoveryToken || !recoveryToken.includes(".")) return;

  try {
    const cleanUrl = new URL(window.location.href);
    cleanUrl.searchParams.delete("recovery_token");
    if (window.history && typeof window.history.replaceState === "function") {
      window.history.replaceState(null, "", cleanUrl.toString());
    }
  } catch {
    // Continue even if history.replaceState is unavailable in test DOM.
  }

  setStatus(root, "recovering", "Verifying licence recovery link…");
  const tokenNode = root.querySelector("[data-keel-money-licence-token]");
  if (tokenNode) {
    tokenNode.textContent = "";
    tokenNode.setAttribute("hidden", "");
  }

  const origin = config.api_origin.replace(/\/+$/, "");
  try {
    const response = await fetch(
      `${origin}/v1/money/${config.parent}/licence_tokens:recover`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${config.publishable_key}`,
          "Content-Type": "application/json",
          Accept: "application/json",
        },
        body: JSON.stringify({ recovery_token: recoveryToken }),
      },
    );
    if (!response.ok) {
      setStatus(
        root,
        "error",
        "Recovery link is invalid or has expired. Request a fresh recovery link below.",
      );
      return;
    }
    const result = await response.json();
    const tokens = Array.isArray(result?.licence_tokens) ? result.licence_tokens : [];
    if (tokens.length > 0 && tokens[0].token && tokenNode) {
      presentLicence(root, config, tokens[0].token);
      setStatus(
        root,
        "recovered",
        `Licence verified. Copy your offline licence key below and run: ${config.token_product || config.product} pro activate '<your-token>'`,
      );
    } else {
      setStatus(root, "error", "No active licence was found for this recovery link.");
    }
  } catch {
    setStatus(root, "error", "Network error while verifying recovery link.");
  }
}

export async function mountCheckout(root) {
  if (root.hasAttribute("data-keel-money-mounted")) return;
  root.setAttribute("data-keel-money-mounted", "1");
  root.setAttribute("data-keel-money-catalog-ready", "0");

  const submitButton = root.querySelector('[data-keel-money-submit="checkout"]');
  setButtonDisabled(submitButton, true);

  let config;
  try {
    config = JSON.parse(root.getAttribute("data-keel-money-checkout") || "{}");
  } catch {
    setStatus(root, "failed", "Invalid checkout configuration JSON.");
    return;
  }

  const configError = validateConfig(config);
  if (configError) {
    setStatus(root, "failed", configError);
    return;
  }

  const checkoutEnabled = config.checkout_enabled !== false;
  const allowedOptions = new Map(
    (Array.isArray(config.prices) ? config.prices : []).map((item) => [
      optionSelectorValue(item),
      item,
    ]),
  );
  const priceSelect = resolveFieldControl(root, "price", "select");
  if (Array.isArray(config.prices) && config.prices.length <= 1 && typeof root.querySelector === "function") {
    const singlePriceField = root.querySelector(".sf-price-select-field");
    if (singlePriceField && typeof singlePriceField.setAttribute === "function") {
      singlePriceField.setAttribute("hidden", "");
    }
  }
  const priceDisplay = root.querySelector("[data-keel-money-price-display]");
  const emailInput = resolveFieldControl(root, "email", 'input[type="email"], input');
  const consentInput = resolveFieldControl(root, "consent", 'input[type="checkbox"], input');
  const checkoutForm = root.querySelector('[data-keel-money-form="checkout"]');
  const recoverForm = root.querySelector('[data-keel-money-form="recover"]');
  const recoverEmailInput = resolveFieldControl(
    root,
    "recovery-email",
    'input[type="email"], input',
  );
  const recoverButton = root.querySelector('[data-keel-money-submit="recover"]');

  let checkoutInFlight = false;
  let pendingAttempt = null;
  let recoverInFlight = false;

  try {
    const returnParams = new URLSearchParams(window?.location?.search || "");
    const returnFlag = String(
      returnParams.get("checkout") ||
        returnParams.get("return") ||
        returnParams.get("status") ||
        returnParams.get("error") ||
        "",
    ).toLowerCase();
    if (
      ["cancel", "cancelled", "canceled", "expired"].includes(returnFlag) ||
      returnParams.get("canceled") === "1" ||
      returnParams.get("cancelled") === "1" ||
      returnParams.get("expired") === "1"
    ) {
      rotateAttemptNonce();
      const returnedSession = returnParams.get("session_id") || returnParams.get("checkout_session");
      // A session reference is reconciled below; URL flags must not duplicate
      // its authoritative answer or turn expiry into explicit cancellation.
      if (!returnedSession?.startsWith("cs_")) {
        const expired = returnFlag === "expired" || returnParams.get("expired") === "1";
        setStatus(
          root,
          expired ? "expired" : "cancelled",
          expired
            ? "Your previous checkout session expired before payment completed. You can start a new checkout below whenever you are ready."
            : "Checkout cancelled — no charge was made. You can choose your currency and start a new checkout below whenever you are ready.",
        );
        reportPurchase(root, {
          status: expired ? "unknown" : "cancelled",
          code: { domain: "keel.checkout", code: expired ? "expired_return" : "cancel_return" },
        });
      }
    }
  } catch {
    // Ignore URL parsing issues in headless or test environments.
  }

  const tierCards = queryAll(root, "[data-keel-money-tier-card]");
  const tierGrid =
    typeof root.querySelector === "function"
      ? root.querySelector("[data-keel-money-tier-grid]")
      : null;
  const billingTabs = queryAll(root, "[data-keel-money-billing]");

  const resolveOptionValue = (raw) => {
    if (!raw) return "";
    if (allowedOptions.has(raw)) return raw;
    for (const [selector, item] of allowedOptions.entries()) {
      if (item && item.key === raw) return selector;
    }
    return "";
  };

  const selectOptionValue = (rawValue) => {
    const nextValue = resolveOptionValue(rawValue);
    if (!nextValue) return false;
    if (priceSelect) {
      priceSelect.value = nextValue;
      if (priceSelect.options) {
        for (const opt of Array.from(priceSelect.options)) {
          opt.selected = opt.value === nextValue;
        }
      }
    }
    const targetCard = tierCards.find(
      (item) => (item.getAttribute("data-keel-money-tier-card") || "") === nextValue,
    );
    const targetBilling = targetCard ? targetCard.getAttribute("data-keel-billing") || "" : "";
    if (targetBilling && (billingTabs.length > 0 || isCardHidden(targetCard))) {
      const alreadyActive =
        billingTabs.length > 0 &&
        billingTabs.some(
          (tab) =>
            tab.getAttribute("data-keel-money-billing") === targetBilling &&
            tab.getAttribute("aria-selected") === "true",
        ) &&
        !isCardHidden(targetCard);
      if (!alreadyActive) {
        applyBillingCadence(
          billingTabs,
          "data-keel-money-billing",
          tierCards,
          tierGrid,
          targetBilling,
        );
      }
    }
    syncTierSelection(root, config, allowedOptions, priceSelect, priceDisplay, submitButton);
    return true;
  };
  root.__keelSelectOption = selectOptionValue;

  if (priceSelect) {
    priceSelect.addEventListener("change", () => {
      if (!selectOptionValue(priceSelect.value)) {
        syncTierSelection(root, config, allowedOptions, priceSelect, priceDisplay, submitButton);
      }
    });
  }

  for (const card of tierCards) {
    card.addEventListener("click", () => {
      selectOptionValue(card.getAttribute("data-keel-money-tier-card") || "");
    });
    card.addEventListener("keydown", (event) => {
      const key = event?.key;
      if (key === "Enter" || key === " ") {
        event.preventDefault?.();
        selectOptionValue(card.getAttribute("data-keel-money-tier-card") || "");
        return;
      }
      if (
        key === "ArrowRight" ||
        key === "ArrowDown" ||
        key === "ArrowLeft" ||
        key === "ArrowUp"
      ) {
        const visibleCards = tierCards.filter((item) => !isCardHidden(item));
        const currentIdx = visibleCards.indexOf(card);
        if (currentIdx < 0 || visibleCards.length < 2) return;
        event.preventDefault?.();
        const delta = key === "ArrowRight" || key === "ArrowDown" ? 1 : -1;
        const nextCard =
          visibleCards[(currentIdx + delta + visibleCards.length) % visibleCards.length];
        selectOptionValue(nextCard.getAttribute("data-keel-money-tier-card") || "");
        nextCard.focus?.();
      }
    });
  }

  const activateBillingTab = (cadence) => {
    if (!cadence) return;
    const currentValue = priceSelect ? priceSelect.value : "";
    const { firstMatchingCard, currentStillVisible } = applyBillingCadence(
      billingTabs,
      "data-keel-money-billing",
      tierCards,
      tierGrid,
      cadence,
      "data-keel-money-tier-card",
      currentValue,
    );
    if (!currentStillVisible && firstMatchingCard) {
      selectOptionValue(firstMatchingCard.getAttribute("data-keel-money-tier-card") || "");
    }
  };
  bindBillingTablist(billingTabs, "data-keel-money-billing", activateBillingTab);

  if (checkoutForm && checkoutEnabled) {
    checkoutForm.addEventListener("submit", async (event) => {
      event.preventDefault();
      if (root.getAttribute("data-keel-money-owned") === "1") return;
      if (checkoutInFlight) return;
      if (root.getAttribute("data-keel-money-catalog-ready") !== "1") {
        setStatus(
          root,
          "error",
          "Live catalog pricing is unavailable. Checkout is disabled.",
        );
        return;
      }
      const chosenSelector = priceSelect
        ? priceSelect.value
        : optionSelectorValue(config.prices[0]);
      const option = allowedOptions.get(chosenSelector);
      if (!option || !Number.isInteger(option.quantity) || option.quantity < 1) {
        setStatus(root, "error", "Refused unknown plan or pack selection.");
        return;
      }
      const buyerEmail = emailInput ? emailInput.value.trim() : "";
      if (!buyerEmail || !buyerEmail.includes("@")) {
        setStatus(root, "error", "Enter a valid email address to receive your licence and receipt.");
        return;
      }
      if (!consentInput || !consentInput.checked) {
        setStatus(root, "error", "Please accept the immediate-supply terms before continuing.");
        return;
      }

      const attribution = extractAttributionMetadata();
      const originAdjustedSuccess = resolveReturnUrl(
        config.success_url,
        config.allowed_origins,
      );
      const originAdjustedCancel = resolveReturnUrl(
        config.cancel_url,
        config.allowed_origins,
      );
      const resolvedSuccessUrl = appendAttributionParams(originAdjustedSuccess, attribution);
      const resolvedCancelUrl = appendAttributionParams(originAdjustedCancel, attribution);
      const attemptFields = [
        config.parent,
        config.product,
        optionSelectorValue(option),
        option.key,
        option.quantity,
        option.currency_code,
        buyerEmail.toLowerCase(),
        config.consent_version,
        config.consent_text,
        resolvedSuccessUrl,
        resolvedCancelUrl,
        attribution,
      ];
      const fingerprint = JSON.stringify(attemptFields);

      checkoutInFlight = true;
      setButtonDisabled(submitButton, true);
      setStatus(root, "submitting", "Starting secure checkout…");
      const origin = config.api_origin.replace(/\/+$/, "");
      let redirecting = false;
      let purchaseOutcome = null;

      const sendClientCreate = async () => {
        const currentNonce = getOrCreateAttemptNonce();
        if (
          !pendingAttempt ||
          pendingAttempt.fingerprint !== fingerprint ||
          pendingAttempt.nonce !== currentNonce
        ) {
          const key = await deriveIdempotencyKey(attemptFields, currentNonce);
          pendingAttempt = { fingerprint, nonce: currentNonce, idempotencyKey: key };
        }
        const requestBody = {
          product: config.product,
          price: option.key,
          quantity: option.quantity,
          currency_code: option.currency_code,
          buyer_email: buyerEmail,
          consent: {
            terms_accepted: true,
            terms_version: config.consent_version,
            terms_text: config.consent_text,
          },
          success_url: resolvedSuccessUrl,
          cancel_url: resolvedCancelUrl,
        };
        if (Object.keys(attribution).length > 0) {
          requestBody.metadata = attribution;
        }
        const response = await fetch(
          `${origin}/v1/money/${config.parent}/checkout_sessions:clientCreate`,
          {
            method: "POST",
            headers: {
              Authorization: `Bearer ${config.publishable_key}`,
              "Content-Type": "application/json",
              Accept: "application/json",
              "Idempotency-Key": pendingAttempt.idempotencyKey,
            },
            body: JSON.stringify(requestBody),
          },
        );
        let payload = null;
        try { payload = await response.json(); } catch { /* Malformed provider answer. */ }
        return { response, payload };
      };

      const isTerminalSession = (session) => {
        if (!session || typeof session !== "object") return false;
        const state = String(responseState(session) || "").toLowerCase();
        if (
          state === "paid" ||
          state === "complete" ||
          state === "completed" ||
          state === "expired" ||
          state === "cancelled" ||
          state === "canceled"
        ) {
          return true;
        }
        if (typeof session.expire_time === "string" && session.expire_time.length > 0) {
          const expMs = Date.parse(session.expire_time);
          if (Number.isFinite(expMs) && expMs <= Date.now()) {
            return true;
          }
        }
        return false;
      };

      try {
        let { response, payload: session } = await sendClientCreate();
        if (response.ok ? isTerminalSession(session) : terminalSession(session, response.status)) {
          rotateAttemptNonce();
          pendingAttempt = null;
          ({ response, payload: session } = await sendClientCreate());
        }
        const createOutcome = checkoutOutcome(session, response.status);
        if (!response.ok || createOutcome.status === "declined") {
          purchaseOutcome = createOutcome;
          setStatus(root, "error", "Checkout request was refused.");
          return;
        }
        if (!session || !isHttpsUrl(session.url)) {
          purchaseOutcome = { status: "unknown", code: createOutcome.code };
          setStatus(root, "error", "Checkout session did not return a valid redirect URL.");
          return;
        }
        if (typeof session.claim_secret === "string" && session.claim_secret.trim().length > 0) {
          try {
            if (session.processor_session_id) {
              window.sessionStorage.setItem(
                `${CLAIM_STORAGE_PREFIX}${session.processor_session_id}`,
                session.claim_secret.trim(),
              );
            }
            if (session.uid) {
              window.sessionStorage.setItem(
                `${CLAIM_STORAGE_PREFIX}${session.uid}`,
                session.claim_secret.trim(),
              );
            }
          } catch {
            // Storage unavailable; email delivery and recovery still work.
          }
        }
        redirecting = true;
        setStatus(root, "redirecting", "Redirecting to checkout…");
        window.location.assign(session.url);
      } catch {
        purchaseOutcome = { status: "unknown", code: { domain: "keel.checkout", code: "network_error" } };
        setStatus(root, "error", "Network error while starting checkout.");
      } finally {
        checkoutInFlight = false;
        if (!redirecting && submitButton && root.getAttribute("data-keel-money-catalog-ready") === "1") {
          setButtonDisabled(submitButton, false);
        }
        if (purchaseOutcome) reportPurchase(root, purchaseOutcome);
      }
    });
  }

  if (recoverForm) {
    recoverForm.addEventListener("submit", async (event) => {
      event.preventDefault();
      if (recoverInFlight) return;
      const recoveryEmail = recoverEmailInput ? recoverEmailInput.value.trim() : "";
      if (!recoveryEmail || !recoveryEmail.includes("@")) {
        setStatus(root, "error", "Enter the email address used at purchase.");
        return;
      }
      recoverInFlight = true;
      setButtonDisabled(recoverButton, true);
      setStatus(root, "recovering", "Requesting licence recovery…");
      const origin = config.api_origin.replace(/\/+$/, "");
      try {
        const response = await fetch(
          `${origin}/v1/money/${config.parent}/licence_tokens:recover`,
          {
            method: "POST",
            headers: {
              Authorization: `Bearer ${config.publishable_key}`,
              "Content-Type": "application/json",
              Accept: "application/json",
            },
            body: JSON.stringify({
              email: recoveryEmail,
              token_product: config.token_product || config.product,
              recovery_url: resolveReturnUrl(
                config.recovery_url ||
                  config.success_url
                    .replace(/([?&])session_id=\{CHECKOUT_SESSION_ID\}(&|$)/, (_, prefix, suffix) =>
                      suffix ? prefix : "",
                    )
                    .replace(/[?&]$/, ""),
                config.allowed_origins,
              ),
            }),
          },
        );
        if (!response.ok) {
          setStatus(root, "error", "Could not submit recovery request. Please try again later.");
          return;
        }
        setStatus(
          root,
          "recovered",
          "If an active licence exists for that email, we have sent a recovery link to your inbox.",
        );
      } catch {
        setStatus(root, "error", "Network error while requesting licence recovery.");
      } finally {
        recoverInFlight = false;
        setButtonDisabled(recoverButton, false);
      }
    });
  }

  if (typeof fetch === "function") {
    const tasks = [];
    if (checkoutEnabled) {
      tasks.push(
        hydrateCatalog(
          root,
          config,
          allowedOptions,
          priceSelect,
          priceDisplay,
          submitButton,
        ),
      );
    }
    tasks.push(
      redeemReturnedSession(root, config),
      redeemRecoveryLink(root, config),
    );
    await Promise.all(tasks);
    showOwnedTier(root, config, checkoutForm, submitButton);
  }
}

function bindBillingTablist(tabs, attrName, onActivate) {
  for (const tab of tabs) {
    tab.addEventListener("click", () => {
      onActivate(tab.getAttribute(attrName) || "");
    });
    tab.addEventListener("keydown", (event) => {
      const key = event?.key;
      if (key === "Enter" || key === " ") {
        event.preventDefault?.();
        onActivate(tab.getAttribute(attrName) || "");
        return;
      }
      if (
        key === "ArrowRight" ||
        key === "ArrowDown" ||
        key === "ArrowLeft" ||
        key === "ArrowUp" ||
        key === "Home" ||
        key === "End"
      ) {
        if (tabs.length < 2) return;
        const idx = tabs.indexOf(tab);
        if (idx < 0) return;
        let nextIdx = idx;
        if (key === "ArrowRight" || key === "ArrowDown") {
          nextIdx = (idx + 1) % tabs.length;
        } else if (key === "ArrowLeft" || key === "ArrowUp") {
          nextIdx = (idx - 1 + tabs.length) % tabs.length;
        } else if (key === "Home") {
          nextIdx = 0;
        } else if (key === "End") {
          nextIdx = tabs.length - 1;
        }
        event.preventDefault?.();
        const nextTab = tabs[nextIdx];
        onActivate(nextTab.getAttribute(attrName) || "");
        nextTab.focus?.();
      }
    });
  }
}

function isCardHidden(card) {
  return Boolean(
    card &&
      (card.hasAttribute?.("hidden") ||
        card.getAttribute?.("data-keel-billing-hidden") === "true"),
  );
}

function applyBillingCadence(
  tabs,
  tabAttr,
  cards,
  grid,
  cadence,
  cardValueAttr = "",
  currentValue = "",
) {
  if (!cadence) {
    return { firstMatchingCard: null, currentStillVisible: false, visibleCount: 0 };
  }
  for (const tab of tabs) {
    const active = tab.getAttribute(tabAttr) === cadence;
    tab.setAttribute("aria-selected", active ? "true" : "false");
    tab.setAttribute("data-keel-active", active ? "true" : "false");
    tab.setAttribute("tabindex", active ? "0" : "-1");
  }
  let firstMatchingCard = null;
  let currentStillVisible = false;
  let visibleCount = 0;
  for (const card of cards) {
    const cardBilling = card.getAttribute("data-keel-billing") || "";
    const visible = !cardBilling || cardBilling === cadence;
    if (visible) {
      visibleCount += 1;
      card.removeAttribute("hidden");
      card.removeAttribute("data-keel-billing-hidden");
      card.removeAttribute("aria-hidden");
      if (card.getAttribute?.("role") === "radio") {
        card.setAttribute("tabindex", "0");
      }
      if (!firstMatchingCard) firstMatchingCard = card;
      if (cardValueAttr && (card.getAttribute(cardValueAttr) || "") === currentValue) {
        currentStillVisible = true;
      }
    } else {
      card.setAttribute("hidden", "");
      card.setAttribute("data-keel-billing-hidden", "true");
      card.setAttribute("aria-hidden", "true");
      if (card.getAttribute?.("role") === "radio") {
        card.setAttribute("tabindex", "-1");
      }
    }
  }
  if (grid && typeof grid.setAttribute === "function") {
    grid.setAttribute("data-keel-visible-count", String(visibleCount));
  }
  return { firstMatchingCard, currentStillVisible, visibleCount };
}

function selectStorefrontContextPrice(host, selectPrice) {
  const checkoutRoots = queryAll(host, "[data-keel-money-checkout]");
  if (typeof host.hasAttribute === "function" && host.hasAttribute("data-keel-money-checkout")) {
    checkoutRoots.unshift(host);
  }
  let handledByMountedCheckout = false;
  for (const checkoutRoot of checkoutRoots) {
    if (typeof checkoutRoot.__keelSelectOption === "function") {
      if (checkoutRoot.__keelSelectOption(selectPrice)) {
        handledByMountedCheckout = true;
      }
    }
  }
  if (!handledByMountedCheckout) {
    const allCards = queryAll(host, "[data-keel-money-tier-card]");
    const targetCard = allCards.find((card) => {
      const val = card.getAttribute("data-keel-money-tier-card") || "";
      return val === selectPrice || val.split(":")[0] === selectPrice;
    });
    if (targetCard) {
      const targetBilling = targetCard.getAttribute?.("data-keel-billing") || "";
      if (targetBilling) {
        const tabs = queryAll(host, "[data-keel-money-billing]");
        const grid =
          typeof host.querySelector === "function"
            ? host.querySelector("[data-keel-money-tier-grid]")
            : null;
        applyBillingCadence(tabs, "data-keel-money-billing", allCards, grid, targetBilling);
      }
      targetCard.click?.();
    }
  }
  for (const plan of queryAll(host, "[data-keel-storefront-plan]")) {
    if (plan.getAttribute("data-keel-storefront-plan") === selectPrice) {
      const planBilling = plan.getAttribute("data-keel-billing") || "";
      if (planBilling) {
        for (const toggle of queryAll(host, "[data-keel-storefront-billing-toggle]")) {
          toggle.__keelSelectCadence?.(planBilling);
        }
      }
    }
  }
}

export function mountStorefrontInteractive(docRoot, explicitSearch) {
  if (!docRoot || typeof docRoot.querySelectorAll !== "function") return;
  for (const toggle of queryAll(docRoot, "[data-keel-storefront-billing-toggle]")) {
    if (toggle.hasAttribute("data-keel-billing-mounted")) continue;
    toggle.setAttribute("data-keel-billing-mounted", "1");
    const sectionRoot = toggle.parentElement || docRoot;
    const plansGrid =
      typeof sectionRoot.querySelector === "function"
        ? sectionRoot.querySelector("#storefront-plans-grid, .sf-plans-grid")
        : null;
    const tabs = queryAll(toggle, "[data-keel-storefront-billing]");
    const plans = queryAll(sectionRoot, "[data-keel-storefront-plan]");
    const selectCadence = (cadence) => {
      applyBillingCadence(tabs, "data-keel-storefront-billing", plans, plansGrid, cadence);
    };
    toggle.__keelSelectCadence = selectCadence;
    bindBillingTablist(tabs, "data-keel-storefront-billing", selectCadence);
  }
  let params = null;
  try {
    const search =
      typeof explicitSearch === "string"
        ? explicitSearch
        : typeof window !== "undefined"
          ? window.location?.search || ""
          : "";
    params = new URLSearchParams(search);
  } catch {
    params = null;
  }
  if (params) {
    for (const host of queryAll(docRoot, "[data-keel-storefront-contexts]")) {
      if (host.hasAttribute("data-keel-contexts-mounted")) continue;
      host.setAttribute("data-keel-contexts-mounted", "1");
      let rules = [];
      try {
        rules = JSON.parse(host.getAttribute("data-keel-storefront-contexts") || "[]");
      } catch {
        rules = [];
      }
      if (!Array.isArray(rules)) continue;
      const matched = rules.find(
        (rule) =>
          rule &&
          typeof rule.param === "string" &&
          rule.param.length > 0 &&
          typeof rule.match === "string" &&
          rule.match.length > 0 &&
          params.get(rule.param) === rule.match,
      );
      if (!matched) continue;
      if (typeof matched.eyebrow === "string" && matched.eyebrow.length > 0) {
        const eyebrowEl =
          typeof host.querySelector === "function"
            ? host.querySelector("#storefront-eyebrow")
            : null;
        if (eyebrowEl) eyebrowEl.textContent = matched.eyebrow;
      }
      if (typeof matched.lead === "string" && matched.lead.length > 0) {
        const leadEl =
          typeof host.querySelector === "function"
            ? host.querySelector("#storefront-lead")
            : null;
        if (leadEl) leadEl.textContent = matched.lead;
      }
      if (typeof matched.roi_callout === "string" && matched.roi_callout.length > 0) {
        const roiEl =
          typeof host.querySelector === "function"
            ? host.querySelector("#storefront-roi")
            : null;
        if (roiEl) roiEl.textContent = matched.roi_callout;
      }
      if (
        typeof matched.select_price === "string" &&
        matched.select_price.length > 0 &&
        !params.get("session_id")
      ) {
        selectStorefrontContextPrice(host, matched.select_price);
      }
      if (typeof matched.highlight === "string" && matched.highlight.length > 0) {
        for (const card of queryAll(host, "[data-keel-bento-id]")) {
          if (card.getAttribute("data-keel-bento-id") === matched.highlight) {
            card.setAttribute("data-keel-highlighted", "true");
          }
        }
      }
    }
  }
}
if (typeof document !== "undefined") {
  for (const element of document.querySelectorAll("[data-keel-money-checkout]")) {
    mountCheckout(element);
  }
  mountStorefrontInteractive(document);
}
