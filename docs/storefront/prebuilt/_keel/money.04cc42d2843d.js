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
export const UTM_ALLOWLIST = [
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_content",
  "utm_term",
];
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
  const out = {};
  let rawSearch = search;
  if (typeof rawSearch !== "string") {
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
  for (const key of UTM_ALLOWLIST) {
    const raw = params.get(key);
    if (typeof raw !== "string") continue;
    const trimmed = raw.trim();
    if (!trimmed) continue;
    out[key] = trimmed.slice(0, MAX_UTM_VALUE_LENGTH);
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
  const entries = UTM_ALLOWLIST.filter(
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

export function formatMinorAmount(rawAmount, currency, quantity = 1) {
  const unitMinor = parseMinorAmount(rawAmount);
  if (
    unitMinor === null ||
    typeof currency !== "string" ||
    !currency.trim() ||
    !Number.isInteger(quantity) ||
    quantity < 1
  ) {
    return null;
  }
  const totalMinor = unitMinor * quantity;
  if (!Number.isSafeInteger(totalMinor)) return null;
  const code = currency.trim().toUpperCase();
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: code,
    }).format(totalMinor / 100);
  } catch {
    return `${(totalMinor / 100).toFixed(2)} ${code}`;
  }
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
      const currency = price.default_currency || Object.keys(price.unit_amounts)[0];
      const formatted = formatMinorAmount(
        currency ? price.unit_amounts[currency] : undefined,
        currency,
        configured.quantity,
      );
      if (!formatted) {
        setButtonDisabled(submitButton, true);
        setStatus(
          root,
          "error",
          `Price "${configured.key}" has no valid catalog unit amount.`,
        );
        return false;
      }
    }
    if (priceSelect) {
      for (const optionEl of priceSelect.querySelectorAll("option")) {
        const optionConfig = allowedOptions.get(optionEl.value);
        if (!optionConfig) continue;
        const price = priceByKey.get(optionConfig.key);
        if (!price || !price.unit_amounts) continue;
        const currency = price.default_currency || Object.keys(price.unit_amounts)[0];
        const formatted = formatMinorAmount(
          currency ? price.unit_amounts[currency] : undefined,
          currency,
          optionConfig.quantity,
        );
        if (formatted) {
          const interval = price.recurring_interval ? ` / ${price.recurring_interval}` : "";
          const baseLabel = optionEl.getAttribute("data-keel-label") || optionConfig.label;
          optionEl.textContent = `${baseLabel} — ${formatted}${interval}`;
        }
      }
      if (priceDisplay && priceSelect.selectedOptions.length > 0) {
        priceDisplay.textContent = priceSelect.selectedOptions[0].textContent;
      }
    }
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

async function redeemReturnedSession(root, config) {
  let params;
  try {
    params = new URLSearchParams(window.location.search);
  } catch {
    return;
  }
  const sessionId = params.get("session_id") || params.get("checkout_session");
  if (!sessionId || !sessionId.startsWith("cs_")) return;

  setStatus(root, "redeeming", "Verifying payment status with Sylphx Money…");
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

  try {
    const response = await fetch(`${origin}/v1/money/checkout_sessions:redeem`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      let errorPayload = null;
      try {
        errorPayload = await response.json();
      } catch {
        errorPayload = null;
      }
      const detailItems = [
        ...(Array.isArray(errorPayload?.details) ? errorPayload.details : []),
        ...(Array.isArray(errorPayload?.error?.details) ? errorPayload.error.details : []),
      ];
      const combinedReason = [
        errorPayload?.reason,
        typeof errorPayload?.error === "string"
          ? errorPayload.error
          : errorPayload?.error?.reason || errorPayload?.error?.code,
        errorPayload?.code,
        errorPayload?.message,
        errorPayload?.error?.message,
        ...detailItems.flatMap((item) => [item?.reason, item?.code, item?.message]),
      ]
        .filter((v) => typeof v === "string" && v.length > 0)
        .join(" ")
        .toLowerCase();
      if (
        response.status === 410 ||
        combinedReason.includes("expired") ||
        combinedReason.includes("cancel") ||
        combinedReason.includes("revoked")
      ) {
        rotateAttemptNonce();
      }
      setStatus(
        root,
        "error",
        "Could not verify checkout session. Use licence recovery below if you already completed payment.",
      );
      return;
    }
    const result = await response.json();
    if (!result || result.paid !== true) {
      const state = String(
        result?.state || result?.status || result?.reason || "",
      ).toLowerCase();
      if (state.includes("expired") || state.includes("cancel")) {
        rotateAttemptNonce();
      }
      setStatus(
        root,
        "pending",
        "Payment is not yet confirmed. Once payment completes, refresh or use licence recovery.",
      );
      return;
    }
    rotateAttemptNonce();
    const tokens = Array.isArray(result.licence_tokens) ? result.licence_tokens : [];
    // Never render a licence token without the browser-bound claim_secret from
    // sessionStorage, even if a response payload unexpectedly includes one.
    if (claimSecret && tokens.length > 0 && tokens[0].token && tokenNode) {
      tokenNode.textContent = tokens[0].token;
      tokenNode.removeAttribute("hidden");
      setStatus(root, "paid", "Payment verified. Copy your offline licence key below.");
    } else {
      setStatus(
        root,
        "paid",
        "Payment verified. Your licence key has been sent to your email address, or you can resend it below.",
      );
    }
  } catch {
    setStatus(root, "error", "Network error while verifying checkout session.");
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
      tokenNode.textContent = tokens[0].token;
      tokenNode.removeAttribute("hidden");
      setStatus(root, "recovered", "Licence verified. Copy your offline licence key below.");
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
      returnParams.get("return") ||
        returnParams.get("status") ||
        returnParams.get("error") ||
        "",
    ).toLowerCase();
    if (
      returnFlag.includes("cancel") ||
      returnFlag.includes("expired") ||
      returnParams.get("canceled") === "1" ||
      returnParams.get("cancelled") === "1" ||
      returnParams.get("expired") === "1"
    ) {
      rotateAttemptNonce();
    }
  } catch {
    // Ignore URL parsing issues in headless or test environments.
  }

  if (priceSelect && priceDisplay) {
    priceSelect.addEventListener("change", () => {
      if (priceSelect.selectedOptions.length > 0) {
        priceDisplay.textContent = priceSelect.selectedOptions[0].textContent;
      }
    });
  }

  if (checkoutForm && checkoutEnabled) {
    checkoutForm.addEventListener("submit", async (event) => {
      event.preventDefault();
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
      const resolvedSuccessUrl = appendAttributionParams(config.success_url, attribution);
      const resolvedCancelUrl = appendAttributionParams(config.cancel_url, attribution);
      const attemptFields = [
        config.parent,
        config.product,
        optionSelectorValue(option),
        option.key,
        option.quantity,
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
        return fetch(
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
      };

      const isTerminalSession = (session) => {
        if (!session || typeof session !== "object") return false;
        const state = String(session.state || session.status || "").toLowerCase();
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
        let response = await sendClientCreate();
        if (!response.ok) {
          let errBody = null;
          try {
            errBody = await response.json();
          } catch {
            errBody = null;
          }
          const detailItems = [
            ...(Array.isArray(errBody?.details) ? errBody.details : []),
            ...(Array.isArray(errBody?.error?.details) ? errBody.error.details : []),
          ];
          const reason = [
            errBody?.reason,
            typeof errBody?.error === "string"
              ? errBody.error
              : errBody?.error?.reason || errBody?.error?.code,
            errBody?.code,
            errBody?.message,
            errBody?.error?.message,
            ...detailItems.flatMap((item) => [item?.reason, item?.code, item?.message]),
          ]
            .filter((v) => typeof v === "string" && v.length > 0)
            .join(" ")
            .toLowerCase();
          if (
            response.status === 409 ||
            response.status === 410 ||
            reason.includes("expired") ||
            reason.includes("cancel") ||
            reason.includes("complete") ||
            reason.includes("paid")
          ) {
            rotateAttemptNonce();
            pendingAttempt = null;
            response = await sendClientCreate();
          }
        }
        if (!response.ok) {
          setStatus(root, "error", "Checkout request was refused by Sylphx Money.");
          return;
        }
        let session = await response.json();
        if (isTerminalSession(session)) {
          rotateAttemptNonce();
          pendingAttempt = null;
          response = await sendClientCreate();
          if (!response.ok) {
            setStatus(root, "error", "Checkout request was refused by Sylphx Money.");
            return;
          }
          session = await response.json();
        }
        if (!session || !isHttpsUrl(session.url)) {
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
        setStatus(root, "error", "Network error while starting checkout.");
      } finally {
        checkoutInFlight = false;
        if (!redirecting && submitButton && root.getAttribute("data-keel-money-catalog-ready") === "1") {
          setButtonDisabled(submitButton, false);
        }
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
              recovery_url:
                config.recovery_url ||
                config.success_url
                  .replace(/([?&])session_id=\{CHECKOUT_SESSION_ID\}(&|$)/, (_, prefix, suffix) =>
                    suffix ? prefix : "",
                  )
                  .replace(/[?&]$/, ""),
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
    const tasks = [
      redeemReturnedSession(root, config),
      redeemRecoveryLink(root, config),
    ];
    if (checkoutEnabled) {
      tasks.unshift(
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
    await Promise.all(tasks);
  }
}

if (typeof document !== "undefined") {
  for (const element of document.querySelectorAll("[data-keel-money-checkout]")) {
    mountCheckout(element);
  }
}
