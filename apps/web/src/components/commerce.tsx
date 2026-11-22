import { ArrowRight, ArrowUpRight, Check, Terminal } from 'lucide-solid';
import { A } from '@solidjs/router';
import { createEffect, createSignal, For, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';
import { useCookieConsent } from './cookie-consent';
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
  const analytics = useCookieConsent();
  const session = useSession();
  const [currency, setCurrency] = createSignal('USD');
  let intent: string | undefined;
  const [consent, setConsent] = createSignal(false);
  const [pending, setPending] = createSignal(false);
  const [notice, setNotice] = createSignal('');
  const price = () => currencyOptions.find((item) => item.code === currency())?.label;
  const user = () => {
    const state = session();
    return state && state !== 'error' && state.user;
  };
  return <div class="stack">
    <div><div class="eyebrow">ONE-TIME PAYMENT</div><div class="price">{price()}</div><p class="muted">Tax included. No subscription.</p></div>
    <label><span class="mb-1 block text-sm font-semibold">Billing currency</span><select class="field" aria-label="Billing currency" value={currency()} onChange={(event) => {intent = undefined; setConsent(false); setCurrency(event.currentTarget.value);}}><For each={currencyOptions}>{(item) => <option value={item.code}>{item.code} · {item.label}</option>}</For></select></label>
    <Show when={user()} fallback={<A class="button button-primary" href="/sign-in?next=/pricing">Sign in to buy Pro <ArrowRight size={16} /></A>}>
      <label class="terms-control"><input type="checkbox" checked={consent()} onChange={(event) => setConsent(event.currentTarget.checked)} /><span>{consentText} This applies to the 14-day right to cancel where available. Your other statutory rights are unaffected.</span></label>
      <button class="button button-primary" disabled={pending() || !consent()} type="button" onClick={async () => {
        if (pending() || !consent() || !user()) return;
        setPending(true); setNotice('');
        try {
          intent ??= checkoutIntent();
          const code = currency();
          const result = await request('checkout', {currency:code, consent:consent(), checkoutIntent:intent, measurementConsent:analytics.checkoutChoice()});
          if (!result.url) throw new Error('Checkout did not return a link.');
          // Measurement must never prevent purchase.
          await analytics.beginCheckout(code).catch(() => undefined);
          location.assign(result.url);
        }
        catch (err) { setNotice(err instanceof Error ? err.message : 'Checkout is unavailable.'); setPending(false); }
      }}>{pending() ? 'Opening checkout…' : 'Continue to secure checkout'} <ArrowRight size={16} /></button>
    </Show>
    {notice() && <p role="alert" class="notice">{notice()}</p>}
    <p class="muted text-sm">See the <A class="inline-link" href="/legal/pro-terms">Pro terms</A> for licence, delivery and cancellation details.</p>
  </div>;
}

export function AccountPanel(props: { billing?: boolean }) {
  const session = useSession(true);
  const activeSession = () => {
    const state = session();
    return state === 'error' ? null : state;
  };
  const user = () => activeSession()?.user;
  const [account, setAccount] = createSignal<Account | 'error' | null>(null);
  const [notice, setNotice] = createSignal('');
  const [pending, setPending] = createSignal(false);
  const plan = () => {
    const state = account();
    return state && state !== 'error' ? state.plan : undefined;
  };
  createEffect(on(user, (signedInUser) => {
    setAccount(null);
    if (!signedInUser) return;
    const controller = new AbortController();
    onCleanup(() => controller.abort());
    fetch('/api/anymd/account', {cache:'no-store', signal:controller.signal})
      .then(async (res) => {
        if (!res.ok) throw new Error();
        const account = await res.json();
        if (!controller.signal.aborted) setAccount(account);
      })
      .catch(() => { if (!controller.signal.aborted) setAccount('error'); });
  }));
  return <Switch>
    <Match when={!session()}><p role="status" class="notice">Checking your session…</p></Match>
    <Match when={session() === 'error'}><p role="alert" class="notice">Sign-in is temporarily unavailable. Reload to try again.</p></Match>
    <Match when={!user()}><section class="panel stack"><h2>Sign in to see your plan</h2><p class="muted">{activeSession()?.authConfigured ? 'See Pro access, billing and purchase recovery in one place.' : 'This environment isn’t connected to sign-in yet. Studio and the docs are open to everyone.'}</p><A href={props.billing ? '/sign-in?next=/billing' : '/sign-in?next=/dashboard'} class="button button-primary justify-self-start">Sign in <ArrowRight size={16} /></A></section></Match>
    <Match when={user()}>
      <div class="columns">
        <section class="panel stack">
          <span class="eyebrow"><Check size={16} /> YOUR PLAN</span>
          <h2>{plan() ?? 'Plan status'}</h2>
          <p class="muted">Signed in as {user()?.email ?? user()?.name ?? 'your Anymd account'}.</p>
          {account() === 'error' && <p role="alert" class="notice">We couldn’t check your Pro access. No changes were made. Reload to check again.</p>}
          {!account() && <p role="status" class="muted">Checking Pro access…</p>}
          {plan() === 'Free OSS' && <A href="/pricing" class="button button-primary justify-self-start">Upgrade to Anymd Pro <ArrowRight size={16} /></A>}
          {plan() === 'Anymd Pro' && <p class="notice">Your account has Anymd Pro access.</p>}
          <div class="flex flex-wrap gap-2"><button disabled={pending()} type="button" class="button" onClick={async () => {
            if (pending() || !user()) return;
            setNotice(''); setPending(true);
            try { const result = await request('billing-portal', {}); if (!result.url) throw new Error('The billing portal did not return a link.'); location.assign(result.url); }
            catch (err) { setNotice(err instanceof Error ? err.message : 'Billing is unavailable.'); setPending(false); }
          }}>{pending() ? 'Opening billing…' : 'Manage billing and invoices'} <ArrowUpRight size={15} /></button><SignOut /></div>
          {notice() && <p role="status" class="notice">{notice()}</p>}
          {activeSession()?.methods?.passkey === true && <RegisterPasskey />}
          <A href="/recover" class="inline-link">Link a previous purchase</A>
        </section>
        <section class="panel stack"><span class="eyebrow"><Terminal size={16} /> LOCAL TOOLS</span><h2>Connect CLI or MCP</h2><p class="muted">Use this MCP configuration in Claude Desktop, Cursor or Windsurf. Local file conversion requires no account.</p>
          <pre class="code-box"><code>{nativeConfig}</code></pre><div class="flex flex-wrap items-center gap-3"><CopyButton text={nativeConfig} label="Copy MCP config" /><A href="/docs" class="inline-link">Install the CLI</A></div>
          <p class="muted text-sm">Browser sign-in for the CLI isn’t available yet. Existing offline Pro activations keep working.</p>
        </section>
      </div>
    </Match>
  </Switch>;
}

export function Claim(props: { checkoutSessionId?: string }) {
  return <Show keyed when={props.checkoutSessionId} fallback={<section class="panel stack"><h2>Open your receipt link</h2><p>Open the Anymd link from your purchase receipt, then sign in to link the purchase to your account. If the link is missing, <a class="inline-link" href="mailto:hi@sylphx.com">contact support</a> with your receipt.</p></section>}>
    {(checkoutSessionId) => <ClaimPurchase checkoutSessionId={checkoutSessionId} />}
  </Show>;
}

function ClaimPurchase(props: { checkoutSessionId: string }) {
  const session = useSession();
  const user = () => {
    const state = session();
    return state && state !== 'error' ? state.user : null;
  };
  const [notice, setNotice] = createSignal('');
  const [claimed, setClaimed] = createSignal(false);
  const [pending, setPending] = createSignal(false);
  const next = () => '/recover?session_id=' + encodeURIComponent(props.checkoutSessionId);
  return <section class="panel stack"><h2>{claimed() ? 'Purchase linked' : 'Link this purchase'}</h2>
    {claimed() ? <A href="/dashboard" class="button button-primary justify-self-start">View your plan <ArrowRight size={16} /></A> :
      <Show when={user()} fallback={<A href={'/sign-in?next=' + encodeURIComponent(next())} class="button button-primary justify-self-start">Sign in to link purchase <ArrowRight size={16} /></A>}>
        <p>Link this purchase to {user()?.email ?? 'your signed-in account'}.</p><button disabled={pending()} type="button" class="button button-primary justify-self-start" onClick={async () => {
          if (pending() || !user()) return;
          setPending(true); setNotice('');
          try { const result = await request('claim', { checkoutSessionId: props.checkoutSessionId }); if (!result.claimed) throw new Error('This purchase has not linked. Check your receipt or contact support.'); setClaimed(true); }
          catch (err) { setNotice(err instanceof Error ? err.message : 'Could not link this purchase.'); }
          finally { setPending(false); }
        }}>{pending() ? 'Linking…' : 'Link purchase to my account'}</button>
      </Show>}
    {notice() && <p role="alert" class="notice">{notice()}</p>}
  </section>;
}

// Previously issued Money recovery links remain usable without moving an
// email-bound order onto a different principal. This is a compatibility path,
// not the normal activation flow for an account purchase.
export function LegacyRecovery() {
  let secret = '';
  onCleanup(() => { secret = ''; });
  const [state, setState] = createSignal<{present: boolean; tokens?:string[]; error?:string}>({present:false});
  const [busy, setBusy] = createSignal(false);
  onMount(() => {
    const url = new URL(location.href);
    const proof = url.searchParams.get('recovery_token');
    if (!proof) return;
    secret = proof;
    url.searchParams.delete('recovery_token');
    history.replaceState(history.state, '', url);
    setState({present:true});
  });
  return <Show when={state().present}><section class="panel stack mb-4"><h2>Restore an offline licence</h2><p class="muted">This is a recovery link for a previous purchase. It does not transfer your order to a new account.</p>
    {!state().tokens && <button class="button button-primary justify-self-start" disabled={busy()} type="button" onClick={async () => {
      if (busy() || !secret) return;
      setBusy(true);
      try {
        const result = await request('recover-legacy', {recoveryToken:secret});
        secret = '';
        setState({present:true,tokens:result.tokens});
      } catch { setState({present:true,error:'We couldn’t open this licence link. It may have expired. Contact hi@sylphx.com for help.'}); }
      finally { setBusy(false); }
    }}>{busy() ? 'Checking…' : 'Open my recovery link'}</button>}
    {state().error && <p class="notice" role="alert">{state().error}</p>}
    {state().tokens?.length === 0 && <p class="notice">No active licences were found for this link.</p>}
    <For each={state().tokens}>{(token, index) => <div><CopyButton text={`anymd pro activate '${token}'`} label={`Copy activation command ${index() + 1}`} /></div>}</For>
  </section></Show>;
}
