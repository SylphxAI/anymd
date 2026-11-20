'use client';

import { ArrowRight, ArrowUpRight, Check, Terminal } from 'lucide-react';
import Link from 'next/link';
import { useEffect, useRef, useState } from 'react';
import { CopyButton } from './copy';
import { SignOut, useSession } from './session';
import { RegisterPasskey } from './sign-in';
import { currencyOptions, consentText } from '@/lib/catalog';

type Account = { plan: 'Free OSS' | 'Anymd Pro'; entitlementActive: boolean; features: string[] };
const nativeConfig = JSON.stringify({ mcpServers: { anymd: { command: 'npx', args: ['-y', '@sylphx/anymd'] } } }, null, 2);
const messages: Record<string, string> = {
  no_customer: 'There is no billing record for this account yet. If you bought Pro before signing in, link your purchase first.',
  not_found: 'We couldn’t find a purchase to link. Check the link from your receipt, or contact support.',
  already_claimed: 'This purchase is already linked. Check your plan or contact support if it’s linked to the wrong account.',
  auth_unconfigured: 'Sign-in is not connected to this environment yet.',
  checkout_unconfigured: 'Checkout isn’t connected to this environment yet. No charge was made.',
  billing_unconfigured: 'Payments are not connected to this environment yet.',
  unauthorized: 'Your session expired. Sign in and try again.',
};

async function request(path: string, data: object) {
  const res = await fetch('/api/anymd/' + path, { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(data) });
  const result = await res.json();
  if (!res.ok) throw new Error(messages[result.code ?? result.error] ?? 'We couldn’t complete that request. If you already paid, check your account before trying again.');
  return result;
}

function checkoutIntent() {
  return Array.from(crypto.getRandomValues(new Uint8Array(32)), (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export function Checkout() {
  const session = useSession();
  const [currency, setCurrency] = useState('USD');
  const intent = useRef<string>(null);
  const [consent, setConsent] = useState(false);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState('');
  const price = currencyOptions.find((item) => item.code === currency)?.label;
  return <div className="stack">
    <div><div className="eyebrow">ONE-TIME PAYMENT</div><div className="price">{price}</div><p className="muted">Tax included. No subscription.</p></div>
    <label><span className="mb-1 block text-sm font-semibold">Billing currency</span><select className="field" aria-label="Billing currency" value={currency} onChange={(event) => {intent.current = null; setConsent(false); setCurrency(event.target.value);}}>{currencyOptions.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.label}</option>)}</select></label>
    {session && session !== 'error' && session.user ? <>
      <label className="terms-control"><input type="checkbox" checked={consent} onChange={(event) => setConsent(event.target.checked)} /><span>{consentText} This applies to the 14-day right to cancel where available. Your other statutory rights are unaffected.</span></label>
      <button className="button button-primary" disabled={pending || !consent} type="button" onClick={async () => {
        setPending(true); setNotice('');
        try { intent.current ??= checkoutIntent(); const result = await request('checkout', {currency, consent, checkoutIntent:intent.current}); if (!result.url) throw new Error('Checkout did not return a link.'); location.assign(result.url); }
        catch (err) { setNotice(err instanceof Error ? err.message : 'Checkout is unavailable.'); setPending(false); }
      }}>{pending ? 'Opening checkout…' : 'Continue to secure checkout'} <ArrowRight size={16} /></button>
    </> : <Link className="button button-primary" href="/sign-in?next=/pricing">Sign in to buy Pro <ArrowRight size={16} /></Link>}
    {notice && <p role="alert" className="notice">{notice}</p>}
    <p className="muted text-sm">See the <Link className="inline-link" href="/legal/pro-terms">Pro terms</Link> for licence, delivery and cancellation details.</p>
  </div>;
}

export function AccountPanel({ billing = false }: { billing?: boolean }) {
  const session = useSession();
  const user = session && session !== 'error' ? session.user : null;
  const [account, setAccount] = useState<Account | 'error' | null>(null);
  const [notice, setNotice] = useState('');
  const [pending, setPending] = useState(false);
  useEffect(() => {
    if (!user) return;
    const controller = new AbortController();
    fetch('/api/anymd/account', {cache:'no-store', signal:controller.signal})
      .then(async (res) => { if (!res.ok) throw new Error(); setAccount(await res.json()); })
      .catch(() => { if (!controller.signal.aborted) setAccount('error'); });
    return () => controller.abort();
  }, [user]);
  if (!session) return <p role="status" className="notice">Checking your session…</p>;
  if (session === 'error') return <p role="alert" className="notice">Sign-in is temporarily unavailable. Reload to try again.</p>;
  if (!user) return <section className="panel stack"><h2>Sign in to see your plan</h2><p className="muted">{session.authConfigured ? 'See Pro access, billing and purchase recovery in one place.' : 'This environment isn’t connected to sign-in yet. Studio and the docs are open to everyone.'}</p><Link href={billing ? '/sign-in?next=/billing' : '/sign-in?next=/dashboard'} className="button button-primary justify-self-start">Sign in <ArrowRight size={16} /></Link></section>;
  return <div className="columns">
    <section className="panel stack">
      <span className="eyebrow"><Check size={16} /> YOUR PLAN</span>
      <h2>{account && account !== 'error' ? account.plan : 'Plan status'}</h2>
      <p className="muted">Signed in as {user.email ?? user.name ?? 'your Anymd account'}.</p>
      {account === 'error' && <p role="alert" className="notice">We couldn’t check your Pro access. No changes were made. Reload to check again.</p>}
      {!account && <p role="status" className="muted">Checking Pro access…</p>}
      {account && account !== 'error' && account.plan === 'Free OSS' && <Link href="/pricing" className="button button-primary justify-self-start">Upgrade to Anymd Pro <ArrowRight size={16} /></Link>}
      {account && account !== 'error' && account.plan === 'Anymd Pro' && <p className="notice">Your account has Anymd Pro access.</p>}
      <div className="flex flex-wrap gap-2"><button disabled={pending} type="button" className="button" onClick={async () => {
        setNotice(''); setPending(true);
        try { const result = await request('billing-portal', {}); if (!result.url) throw new Error('The billing portal did not return a link.'); location.assign(result.url); }
        catch (err) { setNotice(err instanceof Error ? err.message : 'Billing is unavailable.'); setPending(false); }
      }}>{pending ? 'Opening billing…' : 'Manage billing and invoices'} <ArrowUpRight size={15} /></button><SignOut /></div>
      {notice && <p role="status" className="notice">{notice}</p>}
      <RegisterPasskey />
      <Link href="/recover" className="inline-link">Link a previous purchase</Link>
    </section>
    <section className="panel stack"><span className="eyebrow"><Terminal size={16} /> LOCAL TOOLS</span><h2>Connect CLI or MCP</h2><p className="muted">Use this MCP configuration in Claude Desktop, Cursor or Windsurf. Local file conversion requires no account.</p>
      <pre className="code-box"><code>{nativeConfig}</code></pre><div className="flex flex-wrap items-center gap-3"><CopyButton text={nativeConfig} label="Copy MCP config" /><Link href="/docs" className="inline-link">Install the CLI</Link></div>
      <p className="muted text-sm">Browser sign-in for the CLI isn’t available yet. Existing offline Pro activations keep working.</p>
    </section>
  </div>;
}

export function Claim({ checkoutSessionId }: { checkoutSessionId?: string }) {
  const session = useSession();
  const [notice, setNotice] = useState('');
  const [claimed, setClaimed] = useState(false);
  const [pending, setPending] = useState(false);
  if (!checkoutSessionId) return <section className="panel stack"><h2>Open your receipt link</h2><p>Open the Anymd link from your purchase receipt, then sign in to link the purchase to your account. If the link is missing, <a className="inline-link" href="mailto:hi@sylphx.com">contact support</a> with your receipt.</p></section>;
  const next = '/recover?session_id=' + encodeURIComponent(checkoutSessionId);
  return <section className="panel stack"><h2>{claimed ? 'Purchase linked' : 'Link this purchase'}</h2>
    {claimed ? <Link href="/dashboard" className="button button-primary justify-self-start">View your plan <ArrowRight size={16} /></Link> :
      session && session !== 'error' && session.user ? <><p>Link this purchase to {session.user.email ?? 'your signed-in account'}.</p><button disabled={pending} type="button" className="button button-primary justify-self-start" onClick={async () => {
        setPending(true); setNotice('');
        try { const result = await request('claim', { checkoutSessionId }); if (!result.claimed) throw new Error('This purchase has not linked. Check your receipt or contact support.'); setClaimed(true); }
        catch (err) { setNotice(err instanceof Error ? err.message : 'Could not link this purchase.'); }
        finally { setPending(false); }
      }}>{pending ? 'Linking…' : 'Link purchase to my account'}</button></> : <Link href={'/sign-in?next=' + encodeURIComponent(next)} className="button button-primary justify-self-start">Sign in to link purchase <ArrowRight size={16} /></Link>}
    {notice && <p role="alert" className="notice">{notice}</p>}
  </section>;
}

// Previously issued Money recovery links remain usable without moving an
// email-bound order onto a different principal. This is a compatibility path,
// not the normal activation flow for an account purchase.
export function LegacyRecovery() {
  const secret = useRef('');
  const [state, setState] = useState<{present: boolean; tokens?:string[]; error?:string}>({present:false});
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const url = new URL(location.href);
    const proof = url.searchParams.get('recovery_token');
    if (!proof) return;
    secret.current = proof;
    url.searchParams.delete('recovery_token');
    history.replaceState(history.state, '', url);
    setState({present:true});
  }, []);
  if (!state.present) return null;
  return <section className="panel stack mb-4"><h2>Restore an offline licence</h2><p className="muted">This is a recovery link for a previous purchase. It does not transfer your order to a new account.</p>
    {!state.tokens && <button className="button button-primary justify-self-start" disabled={busy} type="button" onClick={async () => {
      setBusy(true);
      try {
        const result = await request('recover-legacy', {recoveryToken:secret.current});
        secret.current = '';
        setState({present:true,tokens:result.tokens});
      } catch { setState({present:true,error:'We couldn’t open this licence link. It may have expired. Contact hi@sylphx.com for help.'}); }
      finally { setBusy(false); }
    }}>{busy ? 'Checking…' : 'Open my recovery link'}</button>}
    {state.error && <p className="notice" role="alert">{state.error}</p>}
    {state.tokens?.length === 0 && <p className="notice">No active licences were found for this link.</p>}
    {state.tokens?.map((token, index) => <div key={index}><CopyButton text={`anymd pro activate '${token}'`} label={`Copy activation command ${index + 1}`} /></div>)}
  </section>;
}
