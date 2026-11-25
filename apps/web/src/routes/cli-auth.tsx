import { A, createAsync, useSearchParams } from '@solidjs/router';
import { useCookieConsent } from '@/components/cookie-consent';
import { PageMeta } from '@/components/page-meta';
import { cliAccount } from '@/lib/auth-pages';
import { challengePattern, statePattern } from '@/lib/cli-params';
import { single } from '@/lib/request-query';

export default function CliAuth() {
  const [search] = useSearchParams();
  const cookies = useCookieConsent();
  const account = createAsync(() => cliAccount(), { deferStream: true });
  const params = () => {
    const port = single(search.port), state = single(search.state), challenge = single(search.challenge);
    if (port && /^\d{1,5}$/.test(port) && Number(port) >= 1024 && Number(port) <= 65535 && state && statePattern.test(state) && challenge && challengePattern.test(challenge)) return {port, state, challenge};
  };
  function consent() {
    const active = account();
    if (!active) return null;
    if (!active.configured) return <p class="notice">Sign-in isn’t connected in this environment.</p>;
    const details = params();
    if (!details) return <p class="notice">Start again with <code>anymd login</code> in your terminal.</p>;
    if (!active.label) return <A class="button button-primary" href={'/sign-in?next=' + encodeURIComponent('/cli-auth?' + new URLSearchParams(details))}>Sign in to continue</A>;
    return <form class="panel stack" action="/api/cli-auth" method="post" onSubmit={() => cookies.trackBehavioral('cli_authorized', {entry_surface:'cli_auth'})}>
      <input type="hidden" name="port" value={details.port} /><input type="hidden" name="state" value={details.state} /><input type="hidden" name="challenge" value={details.challenge} />
      <p>Connect as <strong>{active.label}</strong> and sync your offline Pro access to this computer.</p>
      <p>Only continue if you ran <code>anymd login</code> and your terminal shows this code: <strong><code>{details.state.slice(0, 8)}</code></strong>.</p>
      <button class="button button-primary" type="submit">Connect this CLI</button>
      <A href="/dashboard" class="inline-link">Cancel</A>
    </form>;
  }
  return <section class="wrap page-shell auth-shell">
    {/* Keep a same-origin form Origin without sending the consent query in Referer. */}
    <PageMeta title="Connect the CLI" robots="noindex, nofollow" referrer="strict-origin" />
    <header class="page-heading"><span class="eyebrow">ACCOUNT / CLI</span><h1>Connect the Anymd CLI.</h1></header>
    {consent()}
  </section>;
}
