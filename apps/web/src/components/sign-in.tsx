'use client';
import { ArrowRight, Fingerprint, Mail } from 'lucide-react';
import { useState } from 'react';
import { GithubMark } from "./github-mark";
import { GoogleMark, useSession } from './session';

export async function authRequest(path: string, body: object) {
  const response = await fetch('/api/auth/' + path, { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(body) });
  if (!response.ok) {
    if (response.status === 429) throw new Error('Too many attempts. Wait a minute, then try again.');
    throw new Error('We couldn’t complete sign-in. Check your code or try another sign-in option.');
  }
  return response.json();
}

const bytes = (v: string) => Uint8Array.from(atob(v.replace(/-/g,'+').replace(/_/g,'/')), c => c.charCodeAt(0));
const base64url = (v: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(v))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');

export function SignInForm({next, error}: {next:string; error?:string}) {
  const state = useSession(true);
  const providers = state && state !== 'error' && state.authConfigured ? state.providers ?? [] : [];
  const configured = Boolean(state && state !== 'error' && state.authConfigured);
  const [email, setEmail] = useState('');
  const [code, setCode] = useState('');
  const [step, setStep] = useState(false);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState(error === 'auth_unconfigured' ? 'Sign-in hasn’t been enabled in this environment.' : error ? 'Sign-in did not complete. Try again or choose another method.' : '');
  const providerQuery = '?next=' + encodeURIComponent(next);
  return <div className="panel stack auth-panel">
    {state === 'error' ? <p className="notice" role="alert">Sign-in is temporarily unavailable. Reload to try again.</p> : state && !state.authConfigured ? <p className="notice" role="status">Sign-in isn’t connected in this environment. You can still convert files in Studio and use the docs.</p> : null}
    {providers.includes('google') && <a className="button button-primary" href={'/api/auth/google' + providerQuery}><GoogleMark /> Continue with Google</a>}
    {providers.includes('github') && <a className="button" href={'/api/auth/github' + providerQuery}><GithubMark size={17} /> Continue with GitHub</a>}
    <button type="button" className="button" disabled={!configured || pending} onClick={async () => {
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
    }}><Fingerprint size={18} /> Use a passkey</button>
    <div className="auth-rule"><span>or use email</span></div>
    <form className="stack" onSubmit={async (event) => {
      event.preventDefault(); setPending(true); setNotice('');
      try {
        const result = await authRequest('sign-in', step ? {code,next} : {email,next});
        if (step) location.assign(result.next);
        else { setStep(true); setCode(''); }
      } catch (err) { setNotice(err instanceof Error ? err.message : 'Unable to sign in.'); }
      finally { setPending(false); }
    }}>
      {step ? <label className="field-label">Code sent to {email}<input className="field" autoComplete="one-time-code" inputMode="numeric" pattern="[0-9]{6}" required maxLength={6} value={code} onChange={(ev)=>setCode(ev.target.value)} /></label> : <label className="field-label">Email address<input className="field" type="email" required autoComplete="email" placeholder="you@example.com" maxLength={254} value={email} onChange={(ev)=>setEmail(ev.target.value)} /></label>}
      <button className="button" type="submit" disabled={!configured || pending}><Mail size={16} /> {step ? 'Verify code' : 'Send sign-in code'} <ArrowRight size={16} /></button>
      {step && <button type="button" className="inline-link justify-self-start" onClick={() => {setStep(false);setNotice('');}}>Use a different email</button>}
    </form>
    {notice && <p className="notice" role="alert">{notice}</p>}
  </div>;
}

export function RegisterPasskey() {
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState('');
  return <div><button className="button" type="button" disabled={pending} onClick={async () => {
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
  }}><Fingerprint size={16} /> Add a passkey</button>{notice && <p className="notice mt-2" role="status">{notice}</p>}</div>;
}
