import type { EntryConfig } from '@sylphx/auth/client';
import { createSignInController, signInMessages } from '@sylphx/auth-solid';
import { ArrowRight, Fingerprint } from 'lucide-solid';
import { createEffect, createSignal, onMount, Show } from 'solid-js';
import { authRoute } from '@/lib/auth-route';
import type { Locale } from '@/lib/marketing-locale';
import { completeCode, maskEmail, normalizeCode } from '@/lib/sign-in-flow';
import { GithubMark } from './github-mark';
import { GoogleMark } from './session';

const tw = signInMessages['zh-Hant-TW'];

export function SignInForm(props: {next:string; locale:Locale; error?:string; initialAuth:{authConfigured:boolean; entry:EntryConfig}}) {
  const copy = (en: string, zh: string) => props.locale === 'zh-TW' ? zh : en;
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
  const notice = () => {
    const code = auth.errorCode();
    if (code === 'invalid_code') return copy('That code couldn’t be verified. Check it or request a new one.', '無法驗證這組驗證碼。請確認是否輸入正確，或重新取得驗證碼。');
    if (code) return copy(auth.error(), tw.errors[code]);
    return props.error === 'auth_unconfigured' ? copy('Sign-in hasn’t been enabled in this environment.', '此環境尚未啟用登入。') : '';
  };
  onMount(() => {
    setEmbedded(window.top !== window.self || /Instagram|FBAN|FBAV|MicroMessenger|Line\//i.test(navigator.userAgent));
    setReady(true);
  });
  createEffect(() => { if (auth.step() === 'otp') queueMicrotask(() => codeField?.focus()); });

  return <div class="panel stack auth-panel">
    <Show when={auth.step() === 'choose'}>
      <Show when={google()}><a class="button button-primary" href={authRoute + '/google' + query()} rel="external"><GoogleMark /> {copy('Continue with Google', tw.continueProvider('Google'))}</a></Show>
      <Show when={github()}><a class="button" href={authRoute + '/github' + query()} rel="external"><GithubMark size={17} /> {copy('Continue with GitHub', tw.continueProvider('GitHub'))}</a></Show>
      <Show when={auth.config()?.otp || auth.config()?.passkey} fallback={<p class="notice" role="status">{!props.initialAuth.authConfigured
        ? copy('Sign-in isn’t connected in this environment. You can still convert files in Studio and use the docs.', '此環境尚未啟用登入。你仍可使用轉檔工作室，也可以查看文件。')
        : google() || github() ? copy('Email sign-in is currently unavailable.', '目前無法使用電子郵件登入。')
        : copy('No sign-in methods are available. Try again in a supported browser.', '目前沒有可用的登入方式。請改用支援的瀏覽器再試一次。')}</p>}>
        <Show when={auth.config()?.otp || auth.canPasskey()} fallback={<p class="notice">{copy('Passkeys need a compatible browser and a secure connection.', '使用通行金鑰需要支援的瀏覽器與安全連線。')}</p>}>
          <Show when={google() || github()}><div class="auth-rule"><span>{copy('or use email', tw.orEmail)}</span></div></Show>
          <form class="stack" method="post" onSubmit={(ev) => {
            ev.preventDefault();
            auth.setEmail(emailField!.value.trim());
            void (auth.config()?.otp ? auth.continueWithEmail('otp') : auth.passkey());
          }}>
            <label class="field-label">{copy('Email address', tw.email)}<input class="field" ref={emailField} name="email" type="email" required autocomplete={auth.config()?.passkey ? 'username webauthn' : 'username'} inputmode="email" placeholder="you@example.com" maxLength={254} value={auth.email()} onInput={ev => auth.setEmail(ev.currentTarget.value)} /></label>
            <button class="button button-primary" type="submit" disabled={!ready() || !!auth.busy()}>{copy('Continue', '繼續')} <ArrowRight size={16} /></button>
          </form>
          <Show when={auth.config()?.passkey && auth.canPasskey()}><button type="button" class="button" disabled={!!auth.busy()} onClick={() => void auth.passkey()}><Fingerprint size={16} /> {copy('Use a passkey', tw.usePasskey)}</button></Show>
        </Show>
      </Show>
    </Show>
    <Show when={auth.step() === 'otp'}>
      <h2 class="font-semibold">{copy('Verify it’s you', tw.verifyIdentity)}</h2>
      <p class="muted" role="status">{copy('Sent to ' + maskEmail(auth.email()), '驗證碼已寄至 ' + maskEmail(auth.email()))}</p>
      <form class="stack" method="post" onSubmit={ev => { ev.preventDefault(); void auth.verifyCode(); }}>
        <label class="field-label">{copy('6-digit code', '6 位數驗證碼')}<input class="field" ref={codeField} name="code" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" required readOnly={!!auth.busy()} value={auth.code()} onInput={ev => {
          const value = normalizeCode(ev.currentTarget.value);
          ev.currentTarget.value = value;
          auth.setCode(value);
          if (completeCode.test(value)) void auth.verifyCode(value);
        }} /></label>
        <button class="button button-primary" type="submit" disabled={!!auth.busy() || !completeCode.test(auth.code())}>{copy('Verify code', '驗證')}</button>
      </form>
      <button type="button" class="inline-link justify-self-start" disabled={!!auth.busy() || auth.wait() > 0} onClick={() => void auth.sendCode()}>{auth.wait() ? copy('Resend (' + auth.wait() + 's)', tw.resendIn(auth.wait() + ' 秒')) : copy('Resend', tw.resendCode)}</button>
      <button type="button" class="inline-link justify-self-start" disabled={!!auth.busy()} onClick={() => { auth.go('choose'); queueMicrotask(() => emailField?.focus()); }}>{copy('Change email', tw.changeEmail)}</button>
    </Show>
    <Show when={auth.step() === 'mfa'}>
      <h2 class="font-semibold">Verify it’s you</h2>
      <form class="stack" method="post" onSubmit={ev => { ev.preventDefault(); void auth.submitMfa(String(new FormData(ev.currentTarget).get('factor') ?? '')); }}>
        <label class="field-label">{auth.recovery() ? copy('Recovery code', tw.recoveryCode) : copy('Authenticator code', tw.authenticatorCode)}<input class="field" name="factor" autocomplete="one-time-code" required /></label>
        <button class="button button-primary" type="submit" disabled={!!auth.busy()}>Verify code</button>
      </form>
      <button type="button" class="inline-link justify-self-start" onClick={() => auth.setRecovery(value => !value)}>{auth.recovery() ? copy('Use an authenticator', tw.useAuthenticator) : copy('Use a recovery code', tw.useRecovery)}</button>
    </Show>
    <Show when={notice()}><p class="notice" role="alert">{notice()}</p></Show>
  </div>;
}
