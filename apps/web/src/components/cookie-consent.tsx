import { A, useLocation } from '@solidjs/router';
import { createContext, createEffect, createSignal, onCleanup, onMount, Show, useContext, type ParentProps } from 'solid-js';
import { clearGoogleCookies, startGoogle, stopGoogle, track, type GoogleConsent } from '@/lib/analytics';
import { consentFor, consentKey, parseChoice, traceRegion, type Choice, type Region } from '@/lib/consent';
import { canonicalOrigin } from '@/lib/app-origin';
import { checkoutParameters, explicitAnalytics } from '@/lib/funnel';
import { clickCookie, clickIds } from '@/lib/attribution';

const CookieContext = createContext<{
  open: (from: HTMLButtonElement) => void;
  checkoutChoice: () => Choice;
  beginCheckout: (currency: string) => Promise<void>;
}>();

function savedChoice() {
  try { return parseChoice(localStorage.getItem(consentKey)); } catch { return null; }
}

function writeMeasurementCookie(name: string, value: string, age: number) {
  try {
    document.cookie = name + '=' + value + '; Path=/; Max-Age=' + age + '; SameSite=Lax' + (location.protocol === 'https:' ? '; Secure' : '');
  } catch { /* unavailable cookie storage never blocks checkout */ }
}

export function CookieConsent(props: ParentProps) {
  const route = useLocation();
  const [choice, setChoice] = createSignal<Choice | null>(null);
  const [region, setRegion] = createSignal<Region>();
  const [gpc, setGpc] = createSignal(false);
  const [manual, setManual] = createSignal(false);
  let active: GoogleConsent | undefined;
  let lastPath: string | undefined;
  let trigger: HTMLButtonElement | undefined;
  let firstAction: HTMLButtonElement | undefined;
  const [canSave, setCanSave] = createSignal(false);
  const hasGpc = () => (navigator as Navigator & {globalPrivacyControl?: boolean}).globalPrivacyControl === true;
  const eligibleHost = () => location.origin === canonicalOrigin || ['localhost', '127.0.0.1'].includes(location.hostname);
  const visible = () => manual() || (region() === 'opt-in' && choice() === null);

  onMount(() => {
    stopGoogle();
    try { setChoice(parseChoice(localStorage.getItem(consentKey))); setCanSave(true); } catch { /* fail closed when preferences are inaccessible */ }
    setGpc(hasGpc());
    const controller = new AbortController();
    const settle = (value: Region) => { if (!controller.signal.aborted) setRegion(value); };
    if (!eligibleHost()) settle('opt-in');
    else fetch('/cdn-cgi/trace', {credentials:'omit', referrerPolicy:'no-referrer', redirect:'error', cache:'no-store', signal:AbortSignal.any([controller.signal, AbortSignal.timeout(1800)])})
      .then(async response => settle(response.ok ? traceRegion(await response.text(), location.hostname) : 'opt-in'))
      .catch(() => settle('opt-in'));
    const sync = (event: StorageEvent) => {
      if (event.key === consentKey || event.key === null) {
        setChoice(savedChoice());
      }
    };
    const restore = (event: PageTransitionEvent) => {
      if (event.persisted) {
        stopGoogle();
        // Start in a fresh document so a BFCache restore cannot resume an old tag.
        location.reload();
      }
    };
    window.addEventListener('storage', sync);
    window.addEventListener('pageshow', restore);
    onCleanup(() => {
      controller.abort();
      window.removeEventListener('storage', sync);
      window.removeEventListener('pageshow', restore);
    });
  });

  createEffect(() => {
    const resolved = region();
    const pathname = route.pathname;
    const search = route.search;
    const current = choice();
    const privacySignal = gpc();
    if (!resolved) return;
    const consent = consentFor(resolved, current, privacySignal);
    const allowed = eligibleHost() && canSave() && consent.analytics_storage === 'granted';
    if (!allowed || consent.ad_storage !== 'granted' || consent.ad_user_data !== 'granted') {
      if (document.cookie.split(';').some(cookie => cookie.trimStart().startsWith(clickCookie + '='))) writeMeasurementCookie(clickCookie, '', 0);
    } else {
      const params = new URLSearchParams(search);
      const ids = clickIds(key => params.getAll(key).length === 1 ? params.get(key) : null);
      if (Object.keys(ids).length) writeMeasurementCookie(clickCookie, encodeURIComponent(JSON.stringify(ids)), 30 * 86_400);
    }
    if (active && (!allowed || active.ad_storage !== consent.ad_storage)) {
      stopGoogle();
      active = undefined;
      clearGoogleCookies(!allowed);
      // The executed script may still own callbacks. Replace the entire page
      // after a saved change instead of sending denied-mode pings to Google.
      if (canSave()) location.reload();
      return;
    }
    if (!allowed) { clearGoogleCookies(true); return; }
    if (!active) {
      if (consent.ad_storage === 'denied') clearGoogleCookies(false);
      startGoogle(consent, pathname);
      active = consent;
    }
    if (pathname !== lastPath) {
      lastPath = pathname;
      track('page_view', pathname);
      if (pathname === '/pricing' || pathname === '/pro') track('pricing_view', pathname);
    }
  });

  // Reserve the painted bar height, including wrapping and the safe-area inset.
  function reserveSpace(bar: HTMLElement) {
    const root = document.documentElement;
    const measure = () => root.style.setProperty('--cookie-space', Math.ceil(bar.getBoundingClientRect().height) + 'px');
    measure();
    const resize = new ResizeObserver(measure);
    resize.observe(bar);
    onCleanup(() => { resize.disconnect(); root.style.removeProperty('--cookie-space'); });
  }

  function choose(next: Choice) {
    try { localStorage.setItem(consentKey, next); setCanSave(true); } catch {
      // Prefer an unset choice to an old grant if storage ran out of space.
      try { localStorage.removeItem(consentKey); } catch { /* disable measurement in this document */ }
      setCanSave(false);
    }
    // The server can honor a restrictive choice even if a stale page submits
    // a broader one. An unset cookie never grants measurement.
    writeMeasurementCookie(consentKey, canSave() ? next : 'reject', 31_536_000);
    setChoice(next);
    close();
  }
  function close() {
    setManual(false);
    trigger?.focus();
    trigger = undefined;
  }
  const context = {
    open(from: HTMLButtonElement) {
      trigger = from;
      setManual(true);
      queueMicrotask(() => firstAction?.focus());
    },
    checkoutChoice(): Choice {
      const resolved = region();
      if (!resolved || !eligibleHost() || !canSave() || hasGpc()) return 'reject';
      try {
        // Read at submit time: another tab may just have withdrawn consent.
        const saved = parseChoice(localStorage.getItem(consentKey));
        return saved === 'analytics' || saved === 'all' ? saved : 'reject';
      } catch { return 'reject'; }
    },
    async beginCheckout(currency: string) {
      // Regional page-view defaults are not an opt-in to funnel events. Read
      // the persisted choice again in case a different tab withdrew it.
      if (active?.analytics_storage !== 'granted' || !canSave() || !explicitAnalytics(savedChoice(), hasGpc())) return;
      const parameters = checkoutParameters(currency, route.pathname, route.search);
      if (!parameters) return;
      await new Promise<void>(resolve => {
        const done = () => { clearTimeout(timeout); resolve(); };
        const timeout = setTimeout(done, 250);
        if (!track('begin_checkout', route.pathname, {...parameters, event_callback: done, event_timeout: 250})) done();
      });
    },
  };
  return <CookieContext.Provider value={context}>{props.children}
    <Show when={visible()}>
      <aside ref={reserveSpace} class="cookie-panel" aria-labelledby="cookie-heading" onKeyDown={event => { if (manual() && event.key === 'Escape') close(); }}>
        <div class="cookie-row"><div><h2 id="cookie-heading">Cookie settings</h2>
          <p>Optional analytics and ads. <A href="/privacy" class="inline-link">Privacy notice</A></p>
          {gpc() && <p>Global Privacy Control is on. Optional tracking stays off.</p>}
        </div>
        <div class="cookie-actions">
          <button ref={firstAction} class="button" type="button" onClick={() => choose('reject')}>Reject optional</button>
          <button class="button" type="button" onClick={() => choose('analytics')}>Analytics only</button>
          <button class="button" type="button" onClick={() => choose('all')}>Allow analytics &amp; ads</button>
          {manual() && <button class="cookie-close" type="button" onClick={close}>Close without changing</button>}
        </div></div>
      </aside>
    </Show>
  </CookieContext.Provider>;
}

export function useCookieConsent() { return useContext(CookieContext)!; }
