import { A, useSearchParams } from '@solidjs/router';
import { PageMeta } from '@/components/page-meta';
import { CliLogin, LegacyRecovery } from '@/components/commerce';
import { single } from '@/lib/request-query';

export default function Page() {
  const [query] = useSearchParams();
  const receipt = () => single(query.session_id);
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
