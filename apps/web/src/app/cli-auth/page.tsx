import type { Metadata } from 'next';
import Link from 'next/link';
import { headers } from 'next/headers';
import { authConfig, session } from '@/lib/auth';
import { challengePattern, statePattern } from '@/lib/cli-grant';

// Include only the origin on the same-origin form POST; no-referrer would
// force Origin: null in Chromium. The callback redirect sends no referrer.
export const metadata: Metadata = {title:'Connect the CLI', robots:{index:false, follow:false}, referrer:'strict-origin'};

export default async function CliAuth({searchParams}: {searchParams:Promise<Record<string, string | string[] | undefined>>}) {
  const {port, state, challenge} = await searchParams;
  const valid = typeof port === 'string' && /^\d{1,5}$/.test(port) && Number(port) >= 1024 && Number(port) <= 65535 &&
    typeof state === 'string' && statePattern.test(state) && typeof challenge === 'string' && challengePattern.test(challenge);
  const config = authConfig();
  const active = valid && config ? await session(new Request('https://anymd.sylphx.com/cli-auth', {headers: await headers()}), config).catch(() => null) : null;
  return <section className="wrap page-shell auth-shell"><header className="page-heading"><span className="eyebrow">ACCOUNT / CLI</span><h1>Connect the Anymd CLI.</h1></header>
    {!config ? <p className="notice">Sign-in isn’t connected in this environment.</p> : !valid ? <p className="notice">Start again with <code>anymd login</code> in your terminal.</p> : !active ?
      <Link className="button button-primary" href={'/sign-in?next=' + encodeURIComponent('/cli-auth?' + new URLSearchParams({port, state, challenge}))}>Sign in to continue</Link> :
      <form className="panel stack" action="/api/cli-auth" method="post">
        <input type="hidden" name="port" value={port} /><input type="hidden" name="state" value={state} /><input type="hidden" name="challenge" value={challenge} />
        <p>Connect as <strong>{active.user.email || active.user.name || active.user.id}</strong> and sync your offline Pro access to this computer.</p>
        <p>Only continue if you ran <code>anymd login</code> and your terminal shows this code: <strong><code>{state.slice(0, 8)}</code></strong>.</p>
        <button className="button button-primary" type="submit">Connect this CLI</button>
        <Link href="/dashboard" className="inline-link">Cancel</Link>
      </form>}
  </section>;
}
