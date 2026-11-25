import { A, useSearchParams } from '@solidjs/router';
import { createEffect, createMemo, createSignal, onCleanup, onMount } from 'solid-js';
import { useCookieConsent } from '@/components/cookie-consent';
import { trackPurchaseConversion } from '@/lib/analytics';
import { liveCheckoutSession, type PurchaseConversion } from '@/lib/google-ads';
import { PageMeta } from '@/components/page-meta';
import { CliLogin, LegacyRecovery } from '@/components/commerce';
import { single } from '@/lib/request-query';

export default function Page() {
  const [query] = useSearchParams();
  const consent = useCookieConsent();
  const [receipt, setReceipt] = createSignal(single(query.session_id));
  const [ready, setReady] = createSignal(false);
  onMount(() => {
    const url = new URL(location.href);
    if (url.searchParams.has('session_id')) {
      const ids = url.searchParams.getAll('session_id');
      setReceipt(ids.length === 1 ? ids[0] : undefined);
      url.searchParams.delete('session_id');
      history.replaceState(history.state, '', url);
    }
    setReady(true);
  });
  const adsConsent = createMemo(consent.purchaseAds);
  const analyticsConsent = createMemo(consent.analyticsAllowed);
  createEffect(() => {
    const sid = receipt();
    const ads = adsConsent();
    const analytics = analyticsConsent();
    if (!ready() || !liveCheckoutSession(sid) || (!ads && !analytics)) return;
    const controller = new AbortController();
    onCleanup(() => controller.abort());
    const current = () => !controller.signal.aborted && consent.purchaseAds() === ads && consent.analyticsAllowed() === analytics;
    void Promise.resolve().then(async () => {
      const key = 'anymd.ads.claims';
      let id: string | undefined;
      let seen: string[] = [];
      if (ads) {
        // A bounded, per-tab cache keeps raw processor IDs out of storage.
        try {
          const hash = await crypto.subtle.digest('SHA-256', new TextEncoder().encode('anymd-ads-return:' + sid));
          id = Array.from(new Uint8Array(hash).slice(0, 16), x => x.toString(16).padStart(2, '0')).join('');
          seen = (sessionStorage.getItem(key) ?? '').split('.').filter(entry => /^[a-f0-9]{32}$/.test(entry));
        } catch { /* The signed server cookie still deduplicates. */ }
      }
      const claimAds = Boolean(ads) && (!id || !seen.includes(id));
      if (!current() || (!claimAds && !analytics)) return;
      const response = await fetch('/api/anymd/purchase-conversion', {
        method:'POST', headers:{'Content-Type':'application/json'},
        body:JSON.stringify({session_id:sid, analytics_consent:analytics, ads_consent:claimAds}),
        cache:'no-store', redirect:'error', referrerPolicy:'strict-origin', signal:controller.signal,
      });
      if (!claimAds || !ads || response.status !== 200) return;
      const result: PurchaseConversion = await response.json();
      if (!current() || !trackPurchaseConversion(ads, result) || !id) return;
      try { sessionStorage.setItem(key, [id, ...seen].slice(0, 32).join('.')); }
      catch { /* The signed server cookie still deduplicates. */ }
    }).catch(() => { /* Measurement never blocks recovery or activation. */ });
  });
  return <section class="wrap page-shell">
    <PageMeta title="Activate Anymd Pro" robots="noindex" />
    <header class="page-heading"><span class="eyebrow">ANYMD PRO</span><h1>Activate Anymd Pro</h1>
      <p>After your one-time purchase clears, Pro is available on the account you used at checkout. There’s no subscription.</p>
    </header>
    <LegacyRecovery />
    <div class="panel stack max-w-3xl">
      <CliLogin />
      <p class="muted">If Pro doesn’t show up yet, <A class="inline-link" href="/dashboard">check your plan</A> before paying again.</p>
    </div>
    {receipt() && <p class="mt-6 muted">Bought Pro before account checkout? <A class="inline-link" href={'/recover?session_id=' + encodeURIComponent(receipt() ?? '')}>Link a legacy purchase</A>.</p>}
  </section>;
}
