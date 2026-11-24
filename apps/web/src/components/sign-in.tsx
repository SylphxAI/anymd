import type { EntryConfig } from '@sylphx/auth/client';
import { createSignInController } from '@sylphx/auth-solid';
import { ArrowRight, Fingerprint } from 'lucide-solid';
import { createEffect, createSignal, onMount, Show } from 'solid-js';
import { authRoute } from '@/lib/auth-route';
import { completeCode, maskEmail, normalizeCode } from '@/lib/sign-in-flow';
import { GithubMark } from './github-mark';
import { GoogleMark } from './session';

export function SignInForm(props: {next:string; error?:string; initialAuth:{authConfigured:boolean; entry:EntryConfig}}) {
  const auth = createSignInController({
    next: props.next,
    search: props.error ? '?' + new URLSearchParams({error:props.error}) : '',
    loginPath: '/sign-in',
    routePrefix: authRoute,
    initialConfig: props.initialAuth.entry,
    conditionalPasskey: true,
  });
  let codeField: HTMLInputElement | undefined;
  let emailField: HTMLInputElement | undefined;
  const [embedded, setEmbedded] = createSignal(false);
  const [ready, setReady] = createSignal(false);
  const google = () => auth.config()?.providers.includes('google') && !embedded();
  const github = () => auth.config()?.providers.includes('github');
  const query = () => '?' + new URLSearchParams({ next: props.next });
  const notice = () => auth.errorCode() === 'invalid_code' ? 'That code couldn’t be verified. Check it or request a new one.'
    : auth.error() || (props.error === 'auth_unconfigured' ? 'Sign-in hasn’t been enabled in this environment.' : '');
  onMount(() => {
    setEmbedded(window.top !== window.self || /Instagram|FBAN|FBAV|MicroMessenger|Line\//i.test(navigator.userAgent));
    setReady(true);
  });
  createEffect(() => { if (auth.step() === 'otp') queueMicrotask(() => codeField?.focus()); });

  return <div class="panel stack auth-panel">
    <Show when={auth.step() === 'choose'}>
      <Show when={google()}><a class="button button-primary" href={authRoute + '/google' + query()} rel="external"><GoogleMark /> Continue with Google</a></Show>
      <Show when={github()}><a class="button" href={authRoute + '/github' + query()} rel="external"><GithubMark size={17} /> Continue with GitHub</a></Show>
      <Show when={auth.config()?.otp || auth.config()?.passkey} fallback={<p class="notice" role="status">{!props.initialAuth.authConfigured
        ? 'Sign-in isn’t connected in this environment. You can still convert files in Studio and use the docs.'
        : google() || github() ? 'Email sign-in is currently unavailable.' : 'No sign-in methods are available. Try again in a supported browser.'}</p>}>
        <Show when={auth.config()?.otp || auth.canPasskey()} fallback={<p class="notice">Passkeys need a compatible browser and a secure connection.</p>}>
          <Show when={google() || github()}><div class="auth-rule"><span>or use email</span></div></Show>
          <form class="stack" method="post" onSubmit={(ev) => {
            ev.preventDefault();
            auth.setEmail(emailField!.value.trim());
            void (auth.config()?.otp ? auth.continueWithEmail('otp') : auth.passkey());
          }}>
            <label class="field-label">Email address<input class="field" ref={emailField} name="email" type="email" required autocomplete={auth.config()?.passkey ? 'username webauthn' : 'username'} inputmode="email" placeholder="you@example.com" maxLength={254} value={auth.email()} onInput={ev => auth.setEmail(ev.currentTarget.value)} /></label>
            <button class="button button-primary" type="submit" disabled={!ready() || !!auth.busy()}>Continue <ArrowRight size={16} /></button>
          </form>
          <Show when={auth.config()?.passkey && auth.canPasskey()}><button type="button" class="button" disabled={!!auth.busy()} onClick={() => void auth.passkey()}><Fingerprint size={16} /> Use a passkey</button></Show>
        </Show>
      </Show>
    </Show>
    <Show when={auth.step() === 'otp'}>
      <h2 class="font-semibold">Verify it’s you</h2>
      <p class="muted" role="status">Sent to {maskEmail(auth.email())}</p>
      <form class="stack" method="post" onSubmit={ev => { ev.preventDefault(); void auth.verifyCode(); }}>
        <label class="field-label">6-digit code<input class="field" ref={codeField} name="code" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" required readOnly={!!auth.busy()} value={auth.code()} onInput={ev => {
          const value = normalizeCode(ev.currentTarget.value);
          ev.currentTarget.value = value;
          auth.setCode(value);
          if (completeCode.test(value)) void auth.verifyCode(value);
        }} /></label>
        <button class="button button-primary" type="submit" disabled={!!auth.busy() || !completeCode.test(auth.code())}>Verify code</button>
      </form>
      <button type="button" class="inline-link justify-self-start" disabled={!!auth.busy() || auth.wait() > 0} onClick={() => void auth.sendCode()}>Resend{auth.wait() ? ' (' + auth.wait() + 's)' : ''}</button>
      <button type="button" class="inline-link justify-self-start" disabled={!!auth.busy()} onClick={() => { auth.go('choose'); queueMicrotask(() => emailField?.focus()); }}>Change email</button>
    </Show>
    <Show when={auth.step() === 'mfa'}>
      <h2 class="font-semibold">Verify it’s you</h2>
      <form class="stack" method="post" onSubmit={ev => { ev.preventDefault(); void auth.submitMfa(String(new FormData(ev.currentTarget).get('factor') ?? '')); }}>
        <label class="field-label">{auth.recovery() ? 'Recovery code' : 'Authenticator code'}<input class="field" name="factor" autocomplete="one-time-code" required /></label>
        <button class="button button-primary" type="submit" disabled={!!auth.busy()}>Verify code</button>
      </form>
      <button type="button" class="inline-link justify-self-start" onClick={() => auth.setRecovery(value => !value)}>{auth.recovery() ? 'Use an authenticator' : 'Use a recovery code'}</button>
    </Show>
    <Show when={notice()}><p class="notice" role="alert">{notice()}</p></Show>
  </div>;
}
