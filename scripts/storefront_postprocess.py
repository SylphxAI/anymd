#!/usr/bin/env python3
"""Post-process generated Keel storefront HTML and client runtime scripts.

Keeps /pricing SEO metadata (deduplicated title, og:image, Twitter card,
paid SoftwareApplication JSON-LD), above-the-fold checkout placement without a
docs sidebar, and full click-ID + UTM attribution forwarding (gclid, gbraid,
wbraid, fbclid, utm_*) consistent across both pinned and upgraded Keel builds.
"""

import html as html_lib
import json
from pathlib import Path
import re
import sys

SITE_URL = "https://sylphxai.github.io/anymd/"
SITE_NAME = "anymd Pro"
DEFAULT_OG_IMAGE = f"{SITE_URL}og-pro.png"
OG_IMAGE_ALT = "anymd Pro: verifiable video evidence and cite-check for AI agents"
DEFAULT_PRICING_DESCRIPTION = (
    "Let your agent show its evidence. Exact video frames with timestamps and hashes, "
    "and offline cite-checks. One-time 100-year offline licence — $29 USD across 16 local currencies at checkout."
)
DEFAULT_RECOVER_DESCRIPTION = "Retrieve an existing anymd Pro offline licence by email."
DEFAULT_OFFER_DESCRIPTION = (
    "Exact video frames with timestamps and hashes, and offline cite-check. "
    "One-time 100-year offline licence — $29 USD across 16 local currencies at checkout."
)
LEGACY_OFFER_DESCRIPTION = (
    "Exact video frames with timestamps and hashes, and offline cite-check. "
    "Current one-time price comes from the live Money catalogue."
)

STOREFRONT_CSS_PATH = Path(__file__).resolve().parent / "storefront.css"

STOREFRONT_NAV_ITEMS = (
    ("/anymd/", "Home"),
    ("/anymd/guide/getting-started", "Docs"),
    ("/anymd/playground", "Playground"),
    ("/anymd/pricing", "Pricing"),
    ("/anymd/recover", "Recover licence"),
    ("https://github.com/SylphxAI/anymd", "GitHub"),
)

STOREFRONT_FOOTER_LINKS = (
    '<nav class="anymd-sf-footer-links" aria-label="Legal and contact">'
    '<a href="/anymd/recover">Recover licence</a>'
    '<a href="/anymd/legal/pro-terms">anymd Pro terms</a>'
    '<a href="/anymd/legal/privacy">Privacy</a>'
    '<a href="mailto:hi@sylphx.com">hi@sylphx.com</a>'
    "</nav>"
)


def load_storefront_style() -> str:
    """Load the storefront stylesheet wrapped in <style data-anymd-storefront>."""
    css = STOREFRONT_CSS_PATH.read_text(encoding="utf-8").strip()
    return f"<style data-anymd-storefront>\n{css}\n</style>"


def build_storefront_nav(current_href: str) -> str:
    """Build storefront top navigation with aria-current on the active route."""
    links = []
    for href, label in STOREFRONT_NAV_ITEMS:
        current = ' aria-current="page"' if href == current_href else ""
        links.append(f'<a href="{href}"{current}>{label}</a>')
    return '<nav class="anymd-sf-nav" aria-label="Storefront navigation">' + "".join(links) + "</nav>"

MONEY_ALLOWLIST_OLD = """export const UTM_ALLOWLIST = [
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_content",
  "utm_term",
];"""

MONEY_ALLOWLIST_NEW = """export const CLICK_ID_ALLOWLIST = [
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
}"""

MONEY_EXTRACT_OLD = """export function extractAttributionMetadata(search) {
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
}"""

MONEY_EXTRACT_NEW = """export function extractAttributionMetadata(search) {
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
}"""

MONEY_APPEND_OLD = """  const entries = UTM_ALLOWLIST.filter(
    (k) => typeof attribution[k] === "string" && attribution[k].trim().length > 0,
  ).map((k) => [k, attribution[k].trim().slice(0, MAX_UTM_VALUE_LENGTH)]);"""

MONEY_APPEND_NEW = """  const entries = ATTRIBUTION_ALLOWLIST.filter(
    (k) => typeof attribution[k] === "string" && attribution[k].trim().length > 0,
  ).map((k) => [k, attribution[k].trim().slice(0, MAX_UTM_VALUE_LENGTH)]);"""

MONEY_ALLOWLIST_PREV = """export const ATTRIBUTION_ALLOWLIST = [
  ...UTM_ALLOWLIST,
  ...CLICK_ID_ALLOWLIST,
];"""

MONEY_ALLOWLIST_NEXT = """export const ATTRIBUTION_ALLOWLIST = [
  ...UTM_ALLOWLIST,
  ...CLICK_ID_ALLOWLIST,
  "entry_surface",
];"""

MONEY_MOUNT_MARKER = '  root.setAttribute("data-keel-money-catalog-ready", "0");'
MONEY_MOUNT_PERSIST = (
    '  root.setAttribute("data-keel-money-catalog-ready", "0");\n'
    "  extractAttributionMetadata();"
)


def parse_frontmatter(md_path: Path) -> dict[str, str]:
    """Read scalar YAML frontmatter values from a Markdown source file."""
    text = md_path.read_text(encoding="utf-8")
    if not text.startswith("---\n"):
        return {}
    end = text.find("\n---\n", 4)
    if end == -1:
        return {}
    result: dict[str, str] = {}
    for line in text[4:end].splitlines():
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        result[key.strip()] = value.strip().strip('"').strip("'")
    return result


def parse_ga4_measurement_id(config_json: Path) -> str:
    """Read non-placeholder ga4MeasurementId from pro/config.json."""
    if not config_json.is_file():
        return ""
    try:
        data = json.loads(config_json.read_text(encoding="utf-8"))
    except Exception:
        return ""
    ga4 = data.get("ga4MeasurementId")
    if not isinstance(ga4, str):
        return ""
    ga4 = ga4.strip()
    if not ga4 or "XXXX" in ga4:
        return ""
    return ga4


def _build_ga4_consent_banner() -> str:
    """Build the top-of-page flow-positioned cookie consent banner."""
    return """<div class="anymd-sf-consent" id="anymd-sf-consent" role="region" aria-label="Cookie consent" hidden data-anymd-ga4="1">
  <span>
    This page uses Google Analytics measurement when enabled to see how people find us and improve checkout. Depending on your choice, Google may set cookies and process identifiers such as your IP address. We never send your name or email. Sylphx Limited is the controller.
    <a href="/anymd/legal/privacy">Privacy</a>
  </span>
  <span class="anymd-sf-consent-actions">
    <button type="button" data-anymd-consent="all" aria-pressed="false">Accept all</button>
    <button type="button" data-anymd-consent="analytics" aria-pressed="false">Analytics only</button>
    <button type="button" data-anymd-consent="none" aria-pressed="false">Reject</button>
  </span>
</div>"""


def _build_ga4_storefront_script(ga4_id: str) -> str:
    """Build default-deny consent-gated GA4 script for prebuilt pages."""
    safe_id = json.dumps(ga4_id)
    return f"""<script data-anymd-ga4="1">
(() => {{
  const GA4_ID = {safe_id};
  const STORE_KEY = "anymd_pro_consent";
  const ATTR_KEY = "anymd_pro_attribution";
  const PURCHASE_PREFIX = "anymd_pro_purchase_";
  const ALL_DENIED = {{
    ad_storage: "denied",
    ad_user_data: "denied",
    ad_personalization: "denied",
    analytics_storage: "denied",
  }};
  let gtagLoaded = false;
  let lastPaidSessionId = "";

  function setGaDisable(disabled) {{
    if (GA4_ID) window["ga-disable-" + GA4_ID] = disabled;
  }}
  function isAnalyticsCookieName(name) {{
    return name === "_ga" || name.startsWith("_ga_") || name === "_gid" || name.startsWith("_gat");
  }}
  function currentHostname() {{
    if (typeof location.hostname === "string" && location.hostname) return location.hostname;
    try {{
      if (typeof location.origin === "string" && location.origin && location.origin !== "null") {{
        return new URL(location.origin).hostname;
      }}
    }} catch {{}}
    return "";
  }}
  function cookieDomains(hostname) {{
    const domains = new Set([""]);
    const host = (hostname || "").trim();
    if (!host) return Array.from(domains);
    domains.add(host);
    if (host.includes(".")) domains.add("." + host);
    if (!/^\\d+(?:\\.\\d+){{3}}$/.test(host) && !host.includes(":")) {{
      const parts = host.split(".").filter(Boolean);
      for (let i = 1; i < parts.length - 1; i++) {{
        const suffix = parts.slice(i).join(".");
        domains.add(suffix);
        domains.add("." + suffix);
      }}
    }}
    return Array.from(domains);
  }}
  function cookiePaths() {{
    const paths = new Set(["/", "/anymd", "/anymd/"]);
    if (typeof location.pathname === "string" && location.pathname.startsWith("/")) {{
      paths.add(location.pathname);
      const dir = location.pathname.replace(/\\/[^/]*$/, "/");
      if (dir) paths.add(dir);
    }}
    return Array.from(paths);
  }}
  function purgeAnalyticsCookies() {{
    if (typeof document.cookie !== "string" || !document.cookie) return;
    const names = new Set();
    for (const part of document.cookie.split(";")) {{
      const eq = part.indexOf("=");
      const name = (eq === -1 ? part : part.slice(0, eq)).trim();
      if (name && isAnalyticsCookieName(name)) names.add(name);
    }}
    if (names.size === 0) return;
    const domains = cookieDomains(currentHostname());
    const paths = cookiePaths();
    const expired = "expires=Thu, 01 Jan 1970 00:00:00 GMT; max-age=0";
    for (const name of names) {{
      for (const path of paths) {{
        for (const domain of domains) {{
          const domainAttr = domain ? "; domain=" + domain : "";
          document.cookie = name + "=; " + expired + "; path=" + path + domainAttr;
        }}
      }}
    }}
  }}
  if (!GA4_ID) {{
    purgeAnalyticsCookies();
    return;
  }}

  function readConsent() {{
    try {{
      const v = localStorage.getItem(STORE_KEY);
      return v === "all" || v === "analytics" || v === "none" ? v : null;
    }} catch {{ return null; }}
  }}
  function saveConsent(choice) {{
    try {{ localStorage.setItem(STORE_KEY, choice); }} catch {{}}
  }}
  function hasConsent(choice = readConsent()) {{
    return choice === "all" || choice === "analytics";
  }}
  function readEntrySurface() {{
    try {{
      const q = new URLSearchParams(location.search).get("entry_surface");
      if (q && q.trim()) return q.trim();
      const raw = sessionStorage.getItem(ATTR_KEY);
      if (raw) {{
        const parsed = JSON.parse(raw);
        if (parsed && typeof parsed.entry_surface === "string" && parsed.entry_surface.trim()) {{
          return parsed.entry_surface.trim();
        }}
      }}
    }} catch {{}}
    return location.pathname.includes("/recover") ? "recover_page" : "pricing_page";
  }}
  const UNIT_VALUES = {{USD:29,HKD:229,TWD:959,GBP:21.99,EUR:26.99,JPY:4639,KRW:38900,MYR:118.99,SGD:37.99,CAD:41.99,AUD:42.99,NZD:52.99,THB:989,PHP:1859,IDR:529000,VND:749000}};
  function readCheckoutCurrency() {{
    try {{
      if (typeof document !== "undefined" && typeof document.querySelector === "function") {{
        const sel = document.querySelector('[data-keel-money-field="currency"]');
        const v = sel && typeof sel.value === "string" ? sel.value.trim().toUpperCase() : "";
        if (v && Object.prototype.hasOwnProperty.call(UNIT_VALUES, v)) return v;
      }}
      const q = new URLSearchParams(location.search || "").get("currency");
      if (q) {{
        const c = q.trim().toUpperCase();
        if (Object.prototype.hasOwnProperty.call(UNIT_VALUES, c)) return c;
      }}
      if (typeof localStorage !== "undefined" && typeof localStorage.getItem === "function") {{
        const s = (localStorage.getItem("keel:money:currency") || "").trim().toUpperCase();
        if (s && Object.prototype.hasOwnProperty.call(UNIT_VALUES, s)) return s;
      }}
    }} catch {{}}
    return "USD";
  }}
  function readOption() {{
    const sel = document.querySelector('[data-keel-money-field="price"]');
    return sel && typeof sel.value === "string" && sel.value.trim() ? sel.value.trim() : "anymd_pro";
  }}
  function dimensions(trigger) {{
    return {{
      entry_surface: readEntrySurface(),
      plan: "pro",
      plan_sku: "anymd_pro",
      billing_interval: "one_time",
      option: readOption(),
      checkout_trigger: trigger,
    }};
  }}
  function sanitizedPageLocation() {{
    const s = location.search || "";
    if (s.includes("session_id=") || s.includes("recovery_token=")) {{
      return location.origin + location.pathname;
    }}
    return location.href;
  }}
  function consentPayload(choice) {{
    const ad = choice === "all" ? "granted" : "denied";
    return {{
      ad_storage: ad,
      ad_user_data: ad,
      ad_personalization: ad,
      analytics_storage: choice === "none" ? "denied" : "granted",
    }};
  }}
  function ensureGtag(choice) {{
    if (!hasConsent(choice)) {{
      if (choice === "none") {{
        setGaDisable(true);
        purgeAnalyticsCookies();
      }}
      return false;
    }}
    setGaDisable(false);
    window.dataLayer = window.dataLayer || [];
    if (typeof window.gtag !== "function") {{
      window.gtag = function () {{ window.dataLayer.push(arguments); }};
    }}
    if (gtagLoaded) {{
      window.gtag("consent", "update", consentPayload(choice));
      return true;
    }}
    gtagLoaded = true;
    window.gtag("consent", "default", {{ ...ALL_DENIED, wait_for_update: 500 }});
    window.gtag("consent", "update", consentPayload(choice));
    window.gtag("set", "ads_data_redaction", true);
    window.gtag("js", new Date());
    window.gtag("config", GA4_ID, {{
      send_page_view: false,
      page_location: sanitizedPageLocation(),
    }});
    window.gtag("event", "page_view", {{
      send_to: GA4_ID,
      ...dimensions("page_view"),
      page_location: sanitizedPageLocation(),
    }});
    const src = "https://www.googletagmanager.com/gtag/js?id=" + encodeURIComponent(GA4_ID);
    if (!document.querySelector('script[src="' + src + '"]')) {{
      const s = document.createElement("script");
      s.async = true;
      s.src = src;
      document.head.appendChild(s);
    }}
    return true;
  }}
  function emitPurchase(sessionId) {{
    if (!sessionId || !/^cs_[A-Za-z0-9_]+$/.test(sessionId)) return;
    lastPaidSessionId = sessionId;
    if (!ensureGtag(readConsent())) return;
    const key = PURCHASE_PREFIX + sessionId;
    try {{
      if (sessionStorage.getItem(key)) return;
      sessionStorage.setItem(key, "1");
    }} catch {{}}
    const currency = readCheckoutCurrency();
    const val = UNIT_VALUES[currency] || 29;
    window.gtag("event", "purchase", {{
      transaction_id: sessionId,
      value: val,
      currency: currency,
      ...dimensions("stripe_redirect"),
      items: [{{ item_id: "anymd_pro", price: val, quantity: 1 }}],
    }});
  }}

  const banner = document.getElementById("anymd-sf-consent");
  function syncBanner(forceOpen) {{
    const choice = readConsent();
    if (banner) {{
      banner.hidden = forceOpen ? false : choice !== null;
      for (const btn of banner.querySelectorAll("button[data-anymd-consent]")) {{
        btn.setAttribute("aria-pressed", btn.getAttribute("data-anymd-consent") === choice ? "true" : "false");
      }}
    }}
  }}
  if (banner) {{
    for (const btn of banner.querySelectorAll("button[data-anymd-consent]")) {{
      btn.addEventListener("click", () => {{
        const choice = btn.getAttribute("data-anymd-consent");
        if (choice !== "all" && choice !== "analytics" && choice !== "none") return;
        saveConsent(choice);
        syncBanner(false);
        if (hasConsent(choice)) {{
          ensureGtag(choice);
          if (lastPaidSessionId) emitPurchase(lastPaidSessionId);
        }} else {{
          setGaDisable(true);
          if (typeof window.gtag === "function") {{
            window.gtag("consent", "update", ALL_DENIED);
          }}
          purgeAnalyticsCookies();
        }}
      }});
    }}
  }}
  document.addEventListener("click", (ev) => {{
    const t = ev.target;
    if (t instanceof Element && t.closest("[data-anymd-open-cookie]")) {{
      ev.preventDefault();
      syncBanner(true);
      window.scrollTo({{ top: 0, behavior: "auto" }});
    }}
  }});
  document.addEventListener("submit", (ev) => {{
    const form = ev.target;
    if (!(form instanceof Element) || form.getAttribute("data-keel-money-form") !== "checkout") return;
    if (!ensureGtag(readConsent())) return;
    const currency = readCheckoutCurrency();
    const val = UNIT_VALUES[currency] || 29;
    window.gtag("event", "begin_checkout", {{
      currency: currency,
      value: val,
      ...dimensions("pricing_checkout_form"),
      items: [{{ item_id: "anymd_pro", item_name: "anymd Pro", price: val, quantity: 1 }}],
      transport_type: "beacon",
    }});
  }}, true);
  document.addEventListener("keel:money:purchase", (ev) => {{
    const detail = ev && ev.detail;
    if (!detail || detail.status !== "succeeded") return;
    const tx = (detail.receipt && detail.receipt.Store && detail.receipt.Store.data)
      || new URLSearchParams(location.search).get("session_id")
      || new URLSearchParams(location.search).get("checkout_session")
      || "";
    emitPurchase(tx);
  }});

  syncBanner(false);
  ensureGtag(readConsent());
}})();
</script>"""


def inject_storefront_ga4(html_text: str, ga4_id: str) -> str:
    """Inject default-deny consent-gated GA4 + cookie settings link cleanly without static gtag scripts."""
    if not ga4_id:
        return html_text
    cookie_btn = '<span class="anymd-sf-cookie-wrap"><button type="button" class="anymd-sf-cookie-link" data-anymd-open-cookie="1">Cookie settings and Your Privacy Choices</button></span>'
    if "data-anymd-open-cookie" not in html_text:
        if 'id="storefront-footer-inner"' in html_text:
            footer_inner = extract_balanced_tag(html_text, '<div id="storefront-footer-inner"', "div")
            if footer_inner is not None:
                inner_html, s, e = footer_inner
                html_text = html_text[:s] + inner_html[:-len("</div>")] + cookie_btn + "</div>" + html_text[e:]
        elif '<footer id="docs-footer"' in html_text:
            footer = extract_balanced_tag(html_text, '<footer id="docs-footer"', "footer")
            if footer is not None:
                f_html, s, e = footer
                html_text = html_text[:s] + f_html[:-len("</footer>")] + cookie_btn + "</footer>" + html_text[e:]
    existing_banner = extract_balanced_tag(html_text, '<div class="anymd-sf-consent"', "div")
    if existing_banner is not None:
        _, bs, be = existing_banner
        html_text = html_text[:bs] + html_text[be:]
    html_text = re.sub(r'\s*<script data-anymd-ga4="1">.*?</script>', "", html_text, flags=re.DOTALL)
    banner = _build_ga4_consent_banner()
    if '<div id="storefront-root"' in html_text:
        html_text = html_text.replace('<div id="storefront-root"', f'{banner}<div id="storefront-root"', 1)
    elif '<div id="docs-root"' in html_text:
        html_text = html_text.replace('<div id="docs-root"', f'{banner}<div id="docs-root"', 1)
    elif "<body>" in html_text:
        html_text = html_text.replace("<body>", f"<body>\n{banner}", 1)
    script = _build_ga4_storefront_script(ga4_id)
    if "</body>" in html_text:
        html_text = html_text.replace("</body>", f"{script}\n</body>", 1)
    return html_text


def parse_offer_description(docs_toml: Path) -> str:
    """Read [money.offers.pro] description from docs.toml if present."""
    if not docs_toml.is_file():
        return DEFAULT_OFFER_DESCRIPTION
    in_pro_offer = False
    for line in docs_toml.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_pro_offer = stripped == "[money.offers.pro]"
            continue
        if in_pro_offer and stripped.startswith("description"):
            match = re.match(r'^description\s*=\s*"([^"]*)"', stripped)
            if match:
                return match.group(1)
    return DEFAULT_OFFER_DESCRIPTION


def _set_meta_content(html_text: str, attr_name: str, attr_value: str, content: str) -> str:
    """Replace existing <meta attr_name=attr_value content=...> content cleanly."""
    escaped = html_lib.escape(content, quote=True)
    pattern = re.compile(
        rf'(<meta\s+{re.escape(attr_name)}="{re.escape(attr_value)}"\s+content=")[^"]*(">)',
        re.IGNORECASE,
    )
    return pattern.sub(lambda m: f"{m.group(1)}{escaped}{m.group(2)}", html_text, count=1)


def _replace_offer_description(html_text: str, offer_description: str) -> str:
    """Scrub internal catalogue wording in rendered checkout copy and JSON payloads."""
    return html_text.replace(LEGACY_OFFER_DESCRIPTION, offer_description)


def deduplicate_title(value: str, site_name: str = SITE_NAME) -> str:
    """Strip redundant trailing ' · <site_name>' when title already starts with it."""
    suffix = f" · {site_name}"
    if value.lower().startswith(site_name.lower()) and value.endswith(suffix):
        return value[: -len(suffix)].strip()
    return value


def extract_balanced_tag(document: str, start_marker: str, tag_name: str) -> tuple[str, int, int] | None:
    """Extract an HTML element starting at start_marker with balanced nesting."""
    start = document.find(start_marker)
    if start == -1:
        return None
    open_pat = re.compile(rf"<{tag_name}\b", re.IGNORECASE)
    close_pat = re.compile(rf"</{tag_name}>", re.IGNORECASE)
    pos = start
    depth = 0
    while pos < len(document):
        next_open = open_pat.search(document, pos)
        next_close = close_pat.search(document, pos)
        if next_close is None:
            return None
        if next_open is not None and next_open.start() < next_close.start():
            depth += 1
            pos = next_open.end()
        else:
            depth -= 1
            pos = next_close.end()
            if depth == 0:
                return document[start:pos], start, pos
    return None


def _strip_docs_chrome(html_text: str) -> str:
    """Remove docs sidebar, TOC, edit link, and pager from storefront pages."""
    for marker, tag_name in (
        ('<nav id="docs-nav"', "nav"),
        ('<nav id="docs-toc"', "nav"),
        ('<a id="docs-edit"', "a"),
        ('<nav id="docs-pager"', "nav"),
    ):
        extracted = extract_balanced_tag(html_text, marker, tag_name)
        if extracted is not None:
            _, s, e = extracted
            html_text = html_text[:s] + html_text[e:]
    return html_text


def _inject_storefront_style(html_text: str) -> str:
    """Insert or replace the inline storefront stylesheet at the end of <head>."""
    style_tag = load_storefront_style()
    if "<style data-anymd-storefront>" in html_text:
        return re.sub(
            r"<style data-anymd-storefront>.*?</style>",
            lambda _m: style_tag,
            html_text,
            count=1,
            flags=re.DOTALL,
        )
    if "</head>" in html_text:
        return html_text.replace("</head>", f"{style_tag}\n</head>", 1)
    return html_text


def _inject_storefront_nav(html_text: str, current_href: str) -> str:
    """Insert or refresh storefront top navigation inside the active header."""
    nav_html = build_storefront_nav(current_href)
    if 'class="anymd-sf-nav"' in html_text:
        existing = extract_balanced_tag(html_text, '<nav class="anymd-sf-nav"', "nav")
        if existing is not None:
            _, s, e = existing
            return html_text[:s] + nav_html + html_text[e:]
        return html_text
    actions = extract_balanced_tag(html_text, '<div id="storefront-header-actions"', "div")
    if actions is not None:
        act_html, s, e = actions
        updated = act_html[: -len("</div>")] + nav_html + "</div>"
        return html_text[:s] + updated + html_text[e:]
    header = extract_balanced_tag(html_text, '<header id="docs-header"', "header")
    if header is not None:
        hdr_html, s, e = header
        updated = hdr_html.replace("</header>", f"{nav_html}</header>", 1)
        return html_text[:s] + updated + html_text[e:]
    return html_text


def _ensure_storefront_footer_links(html_text: str) -> str:
    """Ensure legal/recovery footer links sit cleanly inside the storefront footer."""
    if "anymd-sf-footer-links" in html_text:
        return html_text
    if 'id="storefront-footer-inner"' in html_text:
        footer_inner = extract_balanced_tag(html_text, '<div id="storefront-footer-inner"', "div")
        if footer_inner is not None:
            inner_html, s, e = footer_inner
            if '<span id="storefront-footer-copy"' in inner_html:
                updated = inner_html.replace(
                    '<span id="storefront-footer-copy"',
                    f'{STOREFRONT_FOOTER_LINKS}<span id="storefront-footer-copy"',
                    1,
                )
            else:
                updated = inner_html[: -len("</div>")] + STOREFRONT_FOOTER_LINKS + "</div>"
            return html_text[:s] + updated + html_text[e:]
    if '<footer id="docs-footer"' in html_text and "anymd Pro terms" not in html_text:
        footer = extract_balanced_tag(html_text, '<footer id="docs-footer"', "footer")
        if footer is not None:
            f_html, s, e = footer
            cookie_wrap = extract_balanced_tag(f_html, '<p><span class="anymd-sf-cookie-wrap"', "p")
            if cookie_wrap is not None:
                cw_html, cs, ce = cookie_wrap
                inner_span = cw_html[len("<p>") : -len("</p>")]
                f_html = f_html[:cs] + inner_span + f_html[ce:]
            updated = f_html.replace(
                ">", f">{STOREFRONT_FOOTER_LINKS}", 1
            )
            return html_text[:s] + updated + html_text[e:]
    return html_text


def postprocess_pricing_html(
    html_text: str,
    frontmatter: dict[str, str],
    offer_description: str | None = None,
    ga4_id: str = "",
) -> str:
    """Apply SEO metadata, JSON-LD, and above-the-fold storefront layout fixes."""
    og_image = frontmatter.get("image") or DEFAULT_OG_IMAGE
    raw_title = frontmatter.get("title") or (
        "anymd Pro: verifiable video evidence and cite-check for AI agents"
    )
    clean_title = deduplicate_title(raw_title)
    description = frontmatter.get("description") or DEFAULT_PRICING_DESCRIPTION
    resolved_offer = offer_description or DEFAULT_OFFER_DESCRIPTION

    # 1. Deduplicate <title> and og:title.
    html_text = re.sub(
        r"<title>([^<]+)</title>",
        lambda m: f"<title>{html_lib.escape(deduplicate_title(html_lib.unescape(m.group(1))), quote=False)}</title>",
        html_text,
        count=1,
    )
    html_text = re.sub(
        r'<meta property="og:title" content="([^"]+)">',
        lambda m: f'<meta property="og:title" content="{html_lib.escape(deduplicate_title(html_lib.unescape(m.group(1))), quote=True)}">',
        html_text,
        count=1,
    )
    html_text = html_text.replace(
        '<meta property="og:type" content="article">',
        '<meta property="og:type" content="website">',
    )
    html_text = _set_meta_content(html_text, "name", "description", description)
    html_text = _set_meta_content(html_text, "property", "og:description", description)
    html_text = _set_meta_content(html_text, "name", "twitter:description", description)

    # 2. Inject og:image, Twitter cards, and paid SoftwareApplication JSON-LD idempotently.
    escaped_title = html_lib.escape(clean_title, quote=True)
    escaped_desc = html_lib.escape(description, quote=True)
    escaped_img = html_lib.escape(og_image, quote=True)
    json_ld = json.dumps(
        {
            "@context": "https://schema.org",
            "@graph": [
                {
                    "@type": "Organization",
                    "name": "Sylphx",
                    "legalName": "Sylphx Limited",
                    "url": "https://sylphx.com",
                    "sameAs": ["https://github.com/SylphxAI", "https://sylphx.com"],
                },
                {
                    "@type": "WebSite",
                    "name": "anymd",
                    "url": SITE_URL,
                    "description": description,
                },
                {
                    "@type": "SoftwareApplication",
                    "name": "anymd Pro",
                    "applicationCategory": "DeveloperApplication",
                    "operatingSystem": "macOS, Linux, Windows",
                    "url": f"{SITE_URL}pricing",
                    "isAccessibleForFree": False,
                    "description": description,
                },
            ],
        },
        separators=(",", ":"),
    )
    extra_head_tags = [
        ('property="og:image"', f'<meta property="og:image" content="{escaped_img}">'),
        ('property="og:image:alt"', f'<meta property="og:image:alt" content="{OG_IMAGE_ALT}">'),
        ('property="og:image:width"', '<meta property="og:image:width" content="1280">'),
        ('property="og:image:height"', '<meta property="og:image:height" content="640">'),
        ('name="twitter:card"', '<meta name="twitter:card" content="summary_large_image">'),
        ('name="twitter:site"', '<meta name="twitter:site" content="@sylphxai">'),
        ('name="twitter:title"', f'<meta name="twitter:title" content="{escaped_title}">'),
        ('name="twitter:description"', f'<meta name="twitter:description" content="{escaped_desc}">'),
        ('name="twitter:image"', f'<meta name="twitter:image" content="{escaped_img}">'),
        ('type="application/ld+json"', f'<script type="application/ld+json">{json_ld}</script>'),
    ]
    if 'type="application/ld+json"' in html_text:
        html_text = re.sub(
            r'<script type="application/ld\+json">.*?</script>',
            lambda _m: f'<script type="application/ld+json">{json_ld}</script>',
            html_text,
            count=1,
            flags=re.DOTALL,
        )
    missing_tags = [tag for marker, tag in extra_head_tags if marker not in html_text]
    if missing_tags:
        injection = "\n".join(missing_tags) + "\n"
        if "<!--/keel:head-->" in html_text:
            html_text = html_text.replace("<!--/keel:head-->", f"{injection}<!--/keel:head-->", 1)
        else:
            html_text = html_text.replace("</head>", f"{injection}</head>", 1)

    # 3. Clean storefront chrome, citrus stylesheet, and navigation.
    html_text = _strip_docs_chrome(html_text)
    if 'data-anymd-storefront="1"' not in html_text:
        if '<div id="storefront-root"' in html_text:
            html_text = html_text.replace(
                '<div id="storefront-root"', '<div id="storefront-root" data-anymd-storefront="1"', 1
            )
        elif '<div id="docs-root"' in html_text:
            html_text = html_text.replace(
                '<div id="docs-root"', '<div id="docs-root" data-anymd-storefront="1"', 1
            )
    if "data-anymd-storefront" in html_text:
        html_text = _inject_storefront_style(html_text)
    html_text = _inject_storefront_nav(html_text, "/anymd/pricing")

    # Move #money-checkout-pricing above #why-it-matters if currently placed after #doc.
    why_idx = html_text.find('<h2 id="why-it-matters"')
    checkout_extracted = extract_balanced_tag(
        html_text, '<section id="money-checkout-pricing"', "section"
    )
    if checkout_extracted is not None and why_idx != -1:
        checkout_html, c_start, c_end = checkout_extracted
        if c_start > why_idx:
            # Remove checkout from below the prose, then split #doc right before #why-it-matters.
            html_without_checkout = html_text[:c_start] + html_text[c_end:]
            why_idx_after = html_without_checkout.find('<h2 id="why-it-matters"')
            if why_idx_after != -1:
                replacement = (
                    f'</div>{checkout_html}<div id="doc-details" data-keel-layout="stack" class="kac36fe1ac774">'
                )
                html_text = (
                    html_without_checkout[:why_idx_after]
                    + replacement
                    + html_without_checkout[why_idx_after:]
                )

    html_text = _ensure_storefront_footer_links(html_text)

    if "anymd-wallet-trust" not in html_text:
        submit_pat = re.compile(
            r'(<button\s+id="money-checkout-pricing-submit"[^>]*>.*?</button>)',
            re.DOTALL,
        )
        html_text = submit_pat.sub(
            lambda m: f'<div class="anymd-submit-wrap">{m.group(1)}<span class="anymd-wallet-trust">Apple Pay · Google Pay · Link</span></div>',
            html_text,
            count=1,
        )

    html_text = _replace_offer_description(html_text, resolved_offer)
    return inject_storefront_ga4(html_text, ga4_id)


def postprocess_recover_html(
    html_text: str,
    frontmatter: dict[str, str],
    offer_description: str | None = None,
    ga4_id: str = "",
) -> str:
    """Apply black-box copy and SEO metadata cleanup to recover.html."""
    description = frontmatter.get("description") or DEFAULT_RECOVER_DESCRIPTION
    resolved_offer = offer_description or DEFAULT_OFFER_DESCRIPTION
    html_text = html_text.replace(
        '<meta property="og:type" content="article">',
        '<meta property="og:type" content="website">',
    )
    html_text = _set_meta_content(html_text, "name", "description", description)
    html_text = _set_meta_content(html_text, "property", "og:description", description)
    html_text = re.sub(
        r"(<span[^>]*>)Money\s*(</span>\s*<span[^>]*>)sends(</span>)",
        r"\1we \2send\3",
        html_text,
    )
    html_text = html_text.replace(
        "If a paid licence exists, Money sends",
        "If a paid licence exists, we send",
    )
    html_text = _strip_docs_chrome(html_text)
    if 'data-anymd-storefront="1"' not in html_text and '<div id="docs-root"' in html_text:
        html_text = html_text.replace(
            '<div id="docs-root"', '<div id="docs-root" data-anymd-storefront="1"', 1
        )
    html_text = _inject_storefront_style(html_text)
    html_text = _inject_storefront_nav(html_text, "/anymd/recover")
    html_text = _ensure_storefront_footer_links(html_text)
    html_text = _replace_offer_description(html_text, resolved_offer)
    return inject_storefront_ga4(html_text, ga4_id)


def postprocess_money_js(js_text: str) -> str:
    """Ensure checkout runtime forwards gclid, gbraid, wbraid, fbclid, utm_*, and entry_surface."""
    if "CLICK_ID_ALLOWLIST" not in js_text:
        if MONEY_ALLOWLIST_OLD not in js_text:
            raise ValueError("Unable to locate UTM_ALLOWLIST in Money runtime")
        js_text = js_text.replace(MONEY_ALLOWLIST_OLD, MONEY_ALLOWLIST_NEW, 1)
    elif '"entry_surface"' not in js_text and MONEY_ALLOWLIST_PREV in js_text:
        js_text = js_text.replace(MONEY_ALLOWLIST_PREV, MONEY_ALLOWLIST_NEXT, 1)
    if MONEY_EXTRACT_OLD in js_text:
        js_text = js_text.replace(MONEY_EXTRACT_OLD, MONEY_EXTRACT_NEW, 1)
    if MONEY_APPEND_OLD in js_text:
        js_text = js_text.replace(MONEY_APPEND_OLD, MONEY_APPEND_NEW, 1)
    if "extractAttributionMetadata();" not in js_text and MONEY_MOUNT_MARKER in js_text:
        js_text = js_text.replace(MONEY_MOUNT_MARKER, MONEY_MOUNT_PERSIST, 1)
    js_text = js_text.replace("Verifying payment status with Sylphx Money…", "Verifying payment status…")
    js_text = js_text.replace("Checkout request was refused by Sylphx Money.", "Checkout request was refused.")
    return js_text


def postprocess_dir(output_dir: Path, pricing_md: Path) -> None:
    """Post-process pricing.html, recover.html, and _keel/money.*.js in output_dir."""
    storefront_dir = pricing_md.parent
    offer_description = parse_offer_description(storefront_dir / "docs.toml")
    ga4_id = parse_ga4_measurement_id(storefront_dir.parent / ".vitepress" / "pro" / "config.json")
    frontmatter = parse_frontmatter(pricing_md) if pricing_md.is_file() else {}
    pricing_html_path = output_dir / "pricing.html"
    if pricing_html_path.is_file():
        updated_html = postprocess_pricing_html(
            pricing_html_path.read_text(encoding="utf-8"), frontmatter, offer_description, ga4_id
        )
        pricing_html_path.write_text(updated_html, encoding="utf-8")

    recover_md = storefront_dir / "recover.md"
    recover_fm = parse_frontmatter(recover_md) if recover_md.is_file() else {}
    recover_html_path = output_dir / "recover.html"
    if recover_html_path.is_file():
        updated_recover = postprocess_recover_html(
            recover_html_path.read_text(encoding="utf-8"), recover_fm, offer_description, ga4_id
        )
        recover_html_path.write_text(updated_recover, encoding="utf-8")

    keel_dir = output_dir / "_keel"
    if keel_dir.is_dir():
        for money_js in sorted(keel_dir.glob("money.*.js")):
            updated_js = postprocess_money_js(money_js.read_text(encoding="utf-8"))
            money_js.write_text(updated_js, encoding="utf-8")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: storefront_postprocess.py STOREFRONT_DIR")
    root = Path(__file__).resolve().parent.parent
    output_dir = Path(sys.argv[1]).resolve()
    postprocess_dir(output_dir, root / "docs" / "storefront" / "pricing.md")


if __name__ == "__main__":
    main()