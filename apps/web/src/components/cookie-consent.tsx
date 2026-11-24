import { A, useLocation } from '@solidjs/router';
import { createContext, createEffect, createSignal, onCleanup, onMount, Show, useContext, type ParentProps } from 'solid-js';
import { bindCheckoutAttribution, createSignalBrowser, detectSignalRegion, resolveEffectiveConsent, type SignalConsent } from '@sylphx/sdk/signal';
import { clearGoogleCookies, measurementId, startGoogle, stopGoogle, track } from '@/lib/analytics';
import { consentKey, savedSignalChoice, signalChoiceKey, signalChoices, signalStorage, type Choice } from '@/lib/consent';
import { canonicalOrigin } from '@/lib/app-origin';
import { localeFromPath } from '@/lib/marketing-locale';
import { checkoutParameters } from '@/lib/funnel';
import type { EntrySurface } from '@/lib/entry-surface';
import { parseSignupSignal, signupCookie, type SignupSignal } from '@/lib/signup';

type CheckoutBinding = {checkoutSessionId: string; apiKey: string; baseUrl: string; gpc: boolean};
type SignalBrowser = ReturnType<typeof createSignalBrowser>;
const CookieContext = createContext<{
  carryCampaign: () => boolean;
  open: (from: HTMLButtonElement) => void;
  bindCheckout: (binding?: CheckoutBinding) => Promise<void>;
  beginCheckout: (currency: string, entrySurface: EntrySurface) => Promise<void>;
}>();

function migrateChoice() {
  const legacy = localStorage.getItem(consentKey);
  if (legacy !== null && localStorage.getItem(signalChoiceKey) === null) {
    // Existing decisions must not revert to regional defaults on upgrade.
    localStorage.setItem(signalChoiceKey, savedSignalChoice(legacy) ?? 'denied');
  }
}

function hasChoice() {
  try { return localStorage.getItem(signalChoiceKey) !== null; } catch { return false; }
}

function writeMeasurementCookie(name: string, value: string, age: number) {
  try {
    document.cookie = name + '=' + value + '; Path=/; Max-Age=' + age + '; SameSite=Lax' + (location.protocol === 'https:' ? '; Secure' : '');
  } catch { /* unavailable cookie storage never blocks checkout */ }
}

function takeSignup() {
  const prefix = signupCookie + '=';
  try {
    const read = () => document.cookie.split(';').map(cookie => cookie.trim()).find(cookie => cookie.startsWith(prefix));
    const cookie = read();
    if (!cookie) return;
    writeMeasurementCookie(signupCookie, '', 0);
    // Do not emit if the browser didn't clear the signal.
    if (!read()) return parseSignupSignal(cookie.slice(prefix.length));
  } catch { /* unavailable storage should not block the landing page */ }
}

export function CookieConsent(props: ParentProps) {
  const route = useLocation();
  const zh = () => localeFromPath(route.pathname) === 'zh-TW';
  const [browser, setBrowser] = createSignal<SignalBrowser>();
  const [changed, setChanged] = createSignal(0);
  const [strict, setStrict] = createSignal(false);
  const [gpc, setGpc] = createSignal(false);
  const [manual, setManual] = createSignal(false);
  let active: SignalConsent | undefined;
  let lastPath: string | undefined;
  let pendingSignup: SignupSignal | undefined;
  let canReload = true;
  let trigger: HTMLButtonElement | undefined;
  let firstAction: HTMLButtonElement | undefined;
  const eligibleHost = () => location.origin === canonicalOrigin || ['localhost', '127.0.0.1'].includes(location.hostname);
  const visible = () => { changed(); return manual() || (strict() && !hasChoice()); };

  onMount(() => {
    stopGoogle();
    const headerGpc = document.documentElement.dataset.signalGpc === '1';
    setGpc(resolveEffectiveConsent({storedChoice:'granted', gpc:headerGpc}).analytics_storage === 'denied');
    let disposed = false;
    let instance: SignalBrowser | undefined;
    const sync = (event: StorageEvent) => {
      if (event.key === signalChoiceKey || event.key === null) setChanged(value => value + 1);
    };
    const restore = (event: PageTransitionEvent) => {
      if (event.persisted) { stopGoogle(); location.reload(); }
    };
    window.addEventListener('storage', sync);
    window.addEventListener('pageshow', restore);
    onCleanup(() => {
      disposed = true;
      instance?.dispose();
      window.removeEventListener('storage', sync);
      window.removeEventListener('pageshow', restore);
    });
    const eligible = eligibleHost();
    void (eligible ? detectSignalRegion() : Promise.resolve(undefined)).then(region => {
      if (disposed) return;
      pendingSignup = takeSignup();
      setStrict(resolveEffectiveConsent({region}).analytics_storage === 'denied' && !gpc());
      let storageUsable = true;
      try { migrateChoice(); } catch { storageUsable = false; }
      instance = createSignalBrowser({region:region ?? 'XX', gpc:headerGpc || !eligible || !storageUsable, storageKey:signalStorage});
      setBrowser(instance);
    });
  });

  createEffect(() => {
    const signal = browser();
    const pathname = route.pathname;
    changed();
    void route.search; // A search-only navigation can carry a new campaign touch.
    if (!signal) return;
    const consent = signal.getConsent();
    signal.capture();
    const allowed = consent.analytics_storage === 'granted';
    if (active && (!allowed || active.ad_storage !== consent.ad_storage)) {
      stopGoogle();
      active = undefined;
      clearGoogleCookies(!allowed);
      // Executed tags may still own callbacks. Never send a denied-mode ping.
      if (canReload) location.reload();
      return;
    }
    if (!allowed) { pendingSignup = undefined; clearGoogleCookies(true); return; }
    if (!active) {
      if (consent.ad_storage === 'denied') clearGoogleCookies(false);
      startGoogle(consent, pathname);
      active = consent;
    }
    if (pendingSignup && hasChoice()) track('sign_up', pathname, pendingSignup);
    pendingSignup = undefined;
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
    const signal = browser();
    if (!signal) return;
    signal.setConsent(signalChoices[next]);
    // Preserve the Auth handoff and a rollback's ability to read the choice.
    const persisted = (() => {
      try {
        if (localStorage.getItem(signalChoiceKey) !== signalChoices[next]) return false;
        localStorage.setItem(consentKey, next);
        return true;
      } catch { return false; }
    })();
    canReload = persisted;
    if (!persisted) signal.setConsent('denied');
    writeMeasurementCookie(consentKey, persisted ? next : 'reject', 31_536_000);
    setChanged(value => value + 1);
    close();
  }
  function close() {
    setManual(false);
    trigger?.focus();
    trigger = undefined;
  }
  const context = {
    carryCampaign() {
      const signal = browser();
      changed();
      return !signal || (strict() && !hasChoice());
    },
    open(from: HTMLButtonElement) {
      trigger = from;
      setManual(true);
      queueMicrotask(() => firstAction?.focus());
    },
    async bindCheckout(binding?: CheckoutBinding) {
      if (!binding) return;
      const {checkoutSessionId, apiKey, baseUrl, gpc} = binding;
      const checkout = {checkoutSessionId};
      const options = {apiKey, baseUrl, gaMeasurementId:measurementId, pageLocation:new URL('/pricing', canonicalOrigin), signal:AbortSignal.timeout(750)};
      const signal = browser();
      if (gpc || !signal) await bindCheckoutAttribution(checkout, {...options, privacy:{storedChoice:'denied'}});
      else await signal.bindCheckoutAttribution(checkout, options);
    },
    async beginCheckout(currency: string, entrySurface: EntrySurface) {
      if (!active || browser()?.getConsent().analytics_storage !== 'granted') return;
      const parameters = checkoutParameters(currency, entrySurface);
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
        <div class="cookie-row"><div class="cookie-summary"><h2 id="cookie-heading">{zh() ? 'Cookie 設定' : 'Cookie settings'}</h2>
          <p>{zh() ? '選用分析與廣告。' : 'Optional analytics & ads. '}<A href="/privacy" class="inline-link">{zh() ? '隱私權說明' : 'Privacy notice'}</A></p>
          {gpc() && <p>{zh() ? '已啟用全球隱私權控制（GPC）。選用追蹤維持關閉。' : 'Global Privacy Control is on. Optional tracking stays off.'}</p>}
        </div>
        <div class="cookie-actions">
          <button ref={firstAction} class="button" type="button" disabled={!browser()} onClick={() => choose('reject')}>{zh() ? '拒絕選用項目' : 'Reject optional'}</button>
          <button class="button" type="button" disabled={!browser()} onClick={() => choose('analytics')}>{zh() ? '僅限分析' : 'Analytics only'}</button>
          <button class="button" type="button" disabled={!browser()} onClick={() => choose('all')}>{zh() ? '允許分析與廣告' : 'Allow analytics & ads'}</button>
          {manual() && <button class="cookie-close" type="button" onClick={close}>{zh() ? '關閉，不變更設定' : 'Close without changing'}</button>}
        </div></div>
      </aside>
    </Show>
  </CookieContext.Provider>;
}

export function useCookieConsent() { return useContext(CookieContext)!; }
