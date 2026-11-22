import { ArrowRight, Fingerprint } from 'lucide-solid';
import { createEffect, createSignal, onCleanup, onMount, Show } from 'solid-js';
import type { Capabilities } from '@/lib/auth-capabilities';
import { completeCode, maskEmail, normalizeCode, resendDelaySeconds, signInEntry } from '@/lib/sign-in-flow';
import { GithubMark } from './github-mark';
import { GoogleMark } from './session';

class AuthError extends Error {
  constructor(message: string, readonly status: number, readonly retryAfter: number) { super(message); }
}

async function authRequest(path: string, body: object, errorMessage = 'We couldn’t complete sign-in. Try again or choose another method.', signal?: AbortSignal) {
  const response = await fetch('/api/auth/' + path, { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(body), signal });
  if (!response.ok) {
    const retryAfter = Number(response.headers.get('Retry-After')) || resendDelaySeconds;
    throw new AuthError(response.status === 429 ? 'Too many attempts. Wait a moment, then try again.' : errorMessage, response.status, retryAfter);
  }
  return response.json();
}

const bytes = (v: string) => Uint8Array.from(atob(v.replace(/-/g,'+').replace(/_/g,'/')), c => c.charCodeAt(0));
const base64url = (v: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(v))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');

async function provePasskey(next: string, signal: AbortSignal, conditional = false) {
  const challenge = await authRequest('passkey-begin', {}, undefined, signal);
  const credential = await navigator.credentials.get({ signal, ...(conditional ? { mediation: 'conditional' as const } : {}), publicKey: {
    challenge: bytes(challenge.challenge), rpId: challenge.rp_id, timeout: conditional ? 600_000 : 60_000, userVerification: 'required',
    allowCredentials: conditional ? [] : (challenge.allowed_credential_ids ?? []).map((id: string) => ({ id: bytes(id), type: 'public-key' })),
  } });
  if (!(credential instanceof PublicKeyCredential) || !(credential.response instanceof AuthenticatorAssertionResponse)) throw new Error('Passkey not available.');
  const proof = credential.response;
  return authRequest('passkey-verify', {
    next, challenge_id: challenge.challenge_id, credential_id: credential.id,
    client_data_json: base64url(proof.clientDataJSON), authenticator_data: base64url(proof.authenticatorData),
    signature: base64url(proof.signature), user_handle: proof.userHandle ? base64url(proof.userHandle) : '',
  }, undefined, signal);
}

export function SignInForm(props: {next:string; error?:string; initialAuth:Capabilities & {authConfigured:boolean}}) {
  const methods = () => props.initialAuth.methods;
  const entry = () => signInEntry(methods());
  const [email, setEmail] = createSignal('');
  const [step, setStep] = createSignal<'email' | 'proof' | 'password'>('email');
  const [code, setCode] = createSignal('');
  const [codeSent, setCodeSent] = createSignal(false);
  const [remaining, setRemaining] = createSignal(0);
  const [pending, setPending] = createSignal(false);
  const [passkeysSupported, setPasskeysSupported] = createSignal(false);
  const [localNotice, setNotice] = createSignal<string>();
  let passkeyRequest: AbortController | undefined;
  let cooldownTimer: ReturnType<typeof setInterval> | undefined;
  let emailField: HTMLInputElement | undefined;
  let codeField: HTMLInputElement | undefined;
  let passwordField: HTMLInputElement | undefined;
  let stepHeading: HTMLHeadingElement | undefined;
  const notice = () => localNotice() ?? (props.error === 'auth_unconfigured'
    ? 'Sign-in hasn’t been enabled in this environment.'
    : props.error ? 'Sign-in did not complete. Try again or choose another method.' : '');
  const providerQuery = () => '?next=' + encodeURIComponent(props.next);
  const [embedded, setEmbedded] = createSignal(false);
  const google = () => props.initialAuth.providers.includes('google') && !embedded();
  const github = () => props.initialAuth.providers.includes('github');
  const canPasskey = () => methods().passkey && passkeysSupported();

  onMount(() => {
    setEmbedded(window.top !== window.self || /Instagram|FBAN|FBAV|MicroMessenger|Line\//i.test(navigator.userAgent));
    setPasskeysSupported(Boolean(window.PublicKeyCredential && navigator.credentials));
  });
  createEffect(() => {
    if (step() !== 'email' || !canPasskey()) return;
    const next = props.next;
    const controller = new AbortController();
    passkeyRequest = controller;
    onCleanup(() => controller.abort());
    void (async () => {
      try {
        if (await PublicKeyCredential.isConditionalMediationAvailable?.() && !controller.signal.aborted) {
          const result = await provePasskey(next, controller.signal, true);
          if (!controller.signal.aborted) location.assign(result.next);
        }
      } catch { /* Autofill is optional; explicit sign-in remains available. */ }
    })();
  });
  onCleanup(() => { passkeyRequest?.abort(); clearInterval(cooldownTimer); });

  function startCooldown(seconds: number) {
    clearInterval(cooldownTimer);
    const deadline = Date.now() + seconds * 1000;
    const tick = () => {
      const left = Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
      setRemaining(left);
      if (!left) clearInterval(cooldownTimer);
    };
    tick();
    cooldownTimer = setInterval(tick, 250);
  }

  async function sendCode() {
    if (pending() || remaining() || !methods().emailOtp) return;
    setNotice(''); setPending(true);
    let sent = false;
    try {
      await authRequest('sign-in', { email: email() }, 'We couldn’t send a code. Try again or choose another method.');
      setCode(''); setCodeSent(true); sent = true;
      startCooldown(resendDelaySeconds);
    } catch (err) {
      if (err instanceof AuthError && err.status === 429) startCooldown(Math.max(resendDelaySeconds, err.retryAfter));
      setNotice(err instanceof AuthError ? err.message : 'We couldn’t send a code. Check your connection and try again.');
    } finally {
      setPending(false);
      if (sent) codeField?.focus();
    }
  }

  async function verifyCode(value: string) {
    if (pending() || !completeCode.test(value)) return;
    setNotice(''); setPending(true);
    try {
      const result = await authRequest('sign-in', { code: value, next: props.next }, 'That code couldn’t be verified. Check it or request a new one.');
      location.assign(result.next);
    } catch (err) { setNotice(err instanceof AuthError ? err.message : 'Check your connection and try again.'); }
    finally { setPending(false); }
  }

  async function usePasskey() {
    if (pending() || !canPasskey()) return;
    const controller = new AbortController();
    passkeyRequest = controller;
    setNotice(''); setPending(true);
    try {
      const result = await provePasskey(props.next, controller.signal);
      if (!controller.signal.aborted) location.assign(result.next);
    } catch {
      if (!controller.signal.aborted) setNotice('Couldn’t sign in with a passkey. Try again or choose another method.');
    } finally { setPending(false); }
  }

  function changeEmail() {
    setNotice(''); setCode(''); setCodeSent(false); setStep('email');
    emailField?.focus();
  }

  return <div class="panel stack auth-panel">
    <Show when={step() === 'email'} fallback={<>
      <h2 class="font-semibold" tabindex="-1" ref={stepHeading}>Verify it’s you</h2>
      <Show when={step() === 'password'} fallback={<>
        <Show when={codeSent()}>
          <p class="muted" role="status">Sent to {maskEmail(email())}</p>
          <form class="stack" method="post" onSubmit={(ev) => { ev.preventDefault(); void verifyCode(code()); }}>
            <label class="field-label">6-digit code<input class="field" ref={codeField} name="code" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" required readOnly={pending()} value={code()} onInput={(ev) => {
              const value = normalizeCode(ev.currentTarget.value);
              ev.currentTarget.value = value;
              if (value === code()) return;
              setCode(value);
              if (completeCode.test(value)) void verifyCode(value);
            }} /></label>
            <button class="button button-primary" type="submit" disabled={pending() || !completeCode.test(code())}>Verify code</button>
          </form>
        </Show>
        <Show when={methods().emailOtp}>
          <button type="button" class="inline-link justify-self-start" disabled={pending() || remaining() > 0} onClick={() => void sendCode()}>
            {codeSent() ? 'Resend' : 'Send a code'}{remaining() ? ' (' + remaining() + 's)' : ''}
          </button>
        </Show>
        <Show when={canPasskey()}><button type="button" class="button" disabled={pending()} onClick={() => void usePasskey()}><Fingerprint size={16} /> Use a passkey</button></Show>
        <Show when={methods().password}><button type="button" class="inline-link justify-self-start" disabled={pending()} onClick={() => {
          setNotice(''); setStep('password'); passwordField?.focus();
        }}>Use password instead</button></Show>
        <Show when={!canPasskey() && entry() === 'other' && !methods().password}><p class="notice">Passkeys need a compatible browser and a secure connection.</p></Show>
      </>}>
        <p class="muted">For accounts that already have a password.</p>
        <form class="stack" method="post" onSubmit={async (ev) => {
          ev.preventDefault();
          if (pending()) return;
          const password = new FormData(ev.currentTarget).get('password');
          setNotice(''); setPending(true);
          try {
            const result = await authRequest('password-sign-in', { email: email(), password, next: props.next }, 'Check your email and password, then try again.');
            location.assign(result.next);
          } catch (err) { setNotice(err instanceof AuthError ? err.message : 'Check your connection and try again.'); }
          finally { setPending(false); }
        }}>
          <input type="hidden" name="username" autocomplete="username" value={email()} />
          <label class="field-label">Password<input class="field" ref={passwordField} type="password" name="password" autocomplete="current-password" required /></label>
          <button class="button button-primary" type="submit" disabled={pending()}>Sign in with password</button>
        </form>
        <Show when={methods().emailOtp || canPasskey()}><button type="button" class="inline-link justify-self-start" disabled={pending()} onClick={() => {
          setNotice(''); setStep('proof'); if (codeSent()) codeField?.focus(); else stepHeading?.focus();
        }}>Use another method</button></Show>
      </Show>
      <button type="button" class="inline-link justify-self-start" disabled={pending()} onClick={changeEmail}>Change email</button>
    </>}>
      <Show when={google()}><a class="button button-primary" href={'/api/auth/google' + providerQuery()} rel="external"><GoogleMark /> Continue with Google</a></Show>
      <Show when={github()}><a class="button" href={'/api/auth/github' + providerQuery()} rel="external"><GithubMark size={17} /> Continue with GitHub</a></Show>
      <Show when={entry() !== 'unavailable'} fallback={<p class="notice" role="status">{!props.initialAuth.authConfigured
        ? 'Sign-in isn’t connected in this environment. You can still convert files in Studio and use the docs.'
        : google() || github() ? 'Email sign-in is currently unavailable.' : 'No sign-in methods are available. Try again in a supported browser.'}</p>}>
        <Show when={google() || github()}><div class="auth-rule"><span>or use email</span></div></Show>
        <form class="stack" method="post" onSubmit={(ev) => {
          ev.preventDefault();
          if (pending()) return;
          // Read the control as well as input events: some autofill does not dispatch input.
          setEmail(emailField!.value.trim());
          passkeyRequest?.abort(); setNotice(''); setStep('proof'); stepHeading?.focus();
          if (entry() === 'email_otp') void sendCode();
        }}>
          <label class="field-label">Email address<input class="field" ref={emailField} name="email" type="email" required autocomplete="username webauthn" inputmode="email" placeholder="you@example.com" maxLength={254} value={email()} onInput={(ev) => setEmail(ev.currentTarget.value)} /></label>
          <button class="button button-primary" type="submit">Continue <ArrowRight size={16} /></button>
        </form>
      </Show>
    </Show>
    <Show when={notice()}><p class="notice" role="alert">{notice()}</p></Show>
  </div>;
}

export function RegisterPasskey() {
  const [pending, setPending] = createSignal(false);
  const [notice, setNotice] = createSignal('');
  return <div><button class="button" type="button" disabled={pending()} onClick={async () => {
    if (pending()) return;
    setPending(true);setNotice('');
    try {
      if (!window.PublicKeyCredential || !navigator.credentials) throw new Error();
      const c = await authRequest('passkey-register-begin', {});
      const credential = await navigator.credentials.create({publicKey:{
        rp: {id:c.rp_id,name:'Anymd'}, user:{id:bytes(c.user_id),name:c.user_name,displayName:c.user_name},
        challenge:bytes(c.challenge), pubKeyCredParams:c.public_key_algorithms.map((alg:number) => ({alg,type:'public-key'})),
        authenticatorSelection:{residentKey:'required',userVerification:'required'}, timeout:60_000, attestation:'none',
      }});
      if (!(credential instanceof PublicKeyCredential) || !(credential.response instanceof AuthenticatorAttestationResponse)) throw new Error();
      await authRequest('passkey-register-verify',{challenge_id:c.challenge_id,credential_id:credential.id,client_data_json:base64url(credential.response.clientDataJSON),attestation_object:base64url(credential.response.attestationObject),transports:credential.response.getTransports()});
      setNotice('Passkey saved. You can use it next time you sign in.');
    } catch { setNotice('Passkey not saved. Sign in again for a fresh session, then retry on a compatible device.'); }
    finally { setPending(false); }
  }}><Fingerprint size={16} /> Add a passkey</button>{notice() && <p class="notice mt-2" role="status">{notice()}</p>}</div>;
}
