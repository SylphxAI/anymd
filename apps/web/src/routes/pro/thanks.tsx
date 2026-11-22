import { useSearchParams } from '@solidjs/router';
import { createEffect, on, onCleanup } from 'solid-js';
import { Recover } from '@/components/recover-page';
import { useCookieConsent } from '@/components/cookie-consent';
import { checkoutContext } from '@/lib/analytics';
import { single } from '@/lib/request-query';

export default function Page() {
  const analytics = useCookieConsent();
  const [query] = useSearchParams();
  createEffect(on([analytics.ready, () => single(query.session_id)], ([allowed, processorSessionId]) => {
    if (!allowed || !processorSessionId) return;
    const checkoutId = checkoutContext(processorSessionId);
    if (!checkoutId) return;
    const controller = new AbortController();
    onCleanup(() => controller.abort());
    fetch('/api/anymd/purchase-measurement', {
      method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({checkoutId, processorSessionId}),
      cache:'no-store', signal:controller.signal,
    }).then(async response => {
      if (!response.ok) return;
      const {purchase} = await response.json();
      if (purchase && !controller.signal.aborted) analytics.purchase(purchase, processorSessionId);
    }).catch(() => { /* Measurement is optional and never blocks recovery. */ });
  }));
  return <Recover title="Link your Pro purchase" />;
}
