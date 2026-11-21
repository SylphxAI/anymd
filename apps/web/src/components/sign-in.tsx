import { ArrowRight, Fingerprint, Mail } from 'lucide-solid';
import { createEffect, createSignal, on, Show } from 'solid-js';
import type { Capabilities } from '@/lib/auth-capabilities';
import { GithubMark } from './github-mark';
import { GoogleMark } from './session';

export async function authRequest(path: string, body: object, errorMessage = 'We couldn’t complete sign-in. Check your code or try another sign-in option.') {
  const response = await fetch('/api/auth/' + path, { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(body) });
  if (!response.ok) {
    if (response.status === 429) throw new Error('Too many attempts. Wait a minute, then try again.');
    throw new Error(errorMessage);
  }
  return response.json();
}

const bytes = (v: string) => Uint8Array.from(atob(v.replace(/-/g,'+').replace(/_/g,'/')), c => c.charCodeAt(0));
const base64url = (v: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(v))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');

export function SignInForm(props: {next:string; error?:string; initialAuth:Capabilities & {authConfigured:boolean}}) {
  const providers = () => props.initialAuth.providers;
  const methods = () => props.initialAuth.methods;
  const [creating, setCreating] = createSignal(false);
  const [accepted, setAccepted] = createSignal(false);
  const [email, setEmail] = createSignal('');
  const [code, setCode] = createSignal('');
  const [step, setStep] = createSignal(false);
  const [pending, setPending] = createSignal(false);
  const [localNotice, setNotice] = createSignal<string>();
  createEffect(on(() => props.error, () => setNotice(undefined), { defer: true }));
  const notice = () => localNotice() ?? (props.error === 'auth_unconfigured'
    ? 'Sign-in hasn’t been enabled in this environment.'
    : props.error ? 'Sign-in did not complete. Try again or choose another method.' : '');
  const hasEmail = () => methods().password || methods().emailOtp;
  const providerQuery = () => '?next=' + encodeURIComponent(props.next);
  return <div class="panel stack auth-panel">
    {!props.initialAuth.authConfigured ? <p class="notice" role="status">Sign-in isn’t connected in this environment. You can still convert files in Studio and use the docs.</p> :
      !providers().length && !methods().passkey && !hasEmail() && <p class="notice" role="status">No sign-in methods are available. Reload to try again.</p>}
    {providers().includes('google') && <a class="button button-primary" href={'/api/auth/google' + providerQuery()} rel="external"><GoogleMark /> Continue with Google</a>}
    {providers().includes('github') && <a class="button" href={'/api/auth/github' + providerQuery()} rel="external"><GithubMark size={17} /> Continue with GitHub</a>}
    {providers().length > 0 && (hasEmail() || methods().passkey) && <div class="auth-rule"><span>{hasEmail() ? 'or use email' : 'or use a passkey'}</span></div>}
    {methods().passkey === true && <button type="button" class="button" disabled={pending()} onClick={async () => {
      if (pending()) return;
      const next = props.next;
      if (!window.PublicKeyCredential || !navigator.credentials) { setNotice('Passkeys require a compatible browser and a secure connection.'); return; }
      setNotice(''); setPending(true);
      try {
        const challenge = await authRequest('passkey-begin', {});
        const credential = await navigator.credentials.get({publicKey:{
          challenge:bytes(challenge.challenge), rpId:challenge.rp_id, timeout:60_000, userVerification:'required',
          allowCredentials:(challenge.allowed_credential_ids ?? []).map((id:string) => ({id:bytes(id), type:'public-key'})),
        }});
        if (!(credential instanceof PublicKeyCredential) || !(credential.response instanceof AuthenticatorAssertionResponse)) throw new Error('passkey cancelled');
        const proof = credential.response;
        const result = await authRequest('passkey-verify',{next, challenge_id:challenge.challenge_id, credential_id:credential.id, client_data_json:base64url(proof.clientDataJSON), authenticator_data:base64url(proof.authenticatorData), signature:base64url(proof.signature), user_handle:proof.userHandle ? base64url(proof.userHandle) : ''});
        location.assign(result.next);
      } catch { setNotice('Couldn’t use a passkey for this site. You can try again or sign in another way.'); }
      finally { setPending(false); }
    }}><Fingerprint size={18} /> Use a passkey</button>}
    {methods().password === true && <form class="stack" method="post" onSubmit={async (event) => {
      event.preventDefault();
      if (pending()) return;
      const signingUp = creating();
      const next = props.next;
      setPending(true); setNotice(''); setAccepted(false);
      const data = new FormData(event.currentTarget);
      try {
        const result = await authRequest(signingUp ? 'password-sign-up' : 'password-sign-in',
          {email:data.get('email'), password:data.get('password'), next}, signingUp
            ? 'Couldn’t create an account. Check your email address and use a password of at least 12 characters.'
            : 'Couldn’t sign in. Check your email and password or try another sign-in option.');
        if (signingUp) { setAccepted(true); setCreating(false); }
        else location.assign(result.next);
      } catch (err) { setNotice(err instanceof Error ? err.message : 'Unable to sign in.'); }
      finally { setPending(false); }
    }}>
      <label class="field-label">Email address<input class="field" name="email" type="email" required autocomplete="username" placeholder="you@example.com" maxLength={254} /></label>
      <label class="field-label">Password<Show when={creating() ? 'new' : 'existing'} keyed>{(mode) => <input class="field" name="password" type="password" required autocomplete={mode === 'new' ? 'new-password' : 'current-password'} minLength={mode === 'new' ? 12 : undefined} maxLength={1024} />}</Show></label>
      <button class="button" type="submit" disabled={pending()}>{creating() ? 'Create account' : 'Sign in with password'} <ArrowRight size={16} /></button>
      {methods().signUp === true && <button type="button" disabled={pending()} class="inline-link justify-self-start" onClick={() => {setCreating(!creating());setNotice('');setAccepted(false);}}>{creating() ? 'Back to sign in' : 'Create an account'}</button>}
    </form>}
    {accepted() && <p class="notice" role="status">Check your inbox for a verification email if you’re creating a new account. Then sign in with your password.</p>}
    {methods().emailOtp === true && <form class="stack" onSubmit={async (event) => {
      event.preventDefault();
      if (pending()) return;
      const verifying = step();
      const submittedEmail = email();
      const next = props.next;
      setPending(true); setNotice('');
      try {
        const result = await authRequest('sign-in', verifying ? {code:code(),next} : {email:submittedEmail,next});
        if (verifying) location.assign(result.next);
        else { setEmail(submittedEmail); setCode(''); setStep(true); }
      } catch (err) { setNotice(err instanceof Error ? err.message : 'Unable to sign in.'); }
      finally { setPending(false); }
    }}>
      {step() ? <label class="field-label">Code sent to {email()}<input class="field" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" required maxLength={6} value={code()} onInput={(ev)=>setCode(ev.currentTarget.value)} /></label> : <label class="field-label">Email address<input class="field" type="email" required autocomplete="email" placeholder="you@example.com" maxLength={254} value={email()} onInput={(ev)=>setEmail(ev.currentTarget.value)} /></label>}
      <button class="button" type="submit" disabled={pending()}><Mail size={16} /> {step() ? 'Verify code' : 'Send sign-in code'} <ArrowRight size={16} /></button>
      {step() && <button type="button" disabled={pending()} class="inline-link justify-self-start" onClick={() => {setStep(false);setNotice('');}}>Use a different email</button>}
    </form>}
    {notice() && <p class="notice" role="alert">{notice()}</p>}
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
