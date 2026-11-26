import { A, createAsync, useLocation, useSearchParams } from '@solidjs/router';
import { Show } from 'solid-js';
import { PageMeta } from '@/components/page-meta';
import { SignInForm } from '@/components/sign-in';
import { signInOptions } from '@/lib/auth-pages';
import { localeFromPath, signInReturnPath } from '@/lib/marketing-locale';
import { single } from '@/lib/request-query';

export const route = { preload: () => signInOptions() };
export default function SignIn() {
  const location = useLocation();
  const locale = () => localeFromPath(location.pathname);
  const zh = () => locale() === 'zh-TW';
  const [search] = useSearchParams();
  const options = createAsync(() => signInOptions(), { deferStream: true });
  const privacy = () => <A class="inline-link" href="/legal/privacy">{zh() ? '隱私權聲明（英文）' : 'privacy notice'}</A>;
  const terms = () => <A class="inline-link" href="/legal/terms">{zh() ? '網站使用條款（英文）' : 'site terms'}</A>;
  return <section class="wrap page-shell auth-shell">
    <PageMeta title={zh() ? '登入' : 'Sign in'} robots="noindex" />
    <header class="page-heading"><span class="eyebrow">{zh() ? '帳號 / 登入' : 'ACCOUNT / SIGN IN'}</span><h1>{zh() ? '登入 Anymd' : 'Sign in to Anymd'}</h1><p>{zh()
      ? '登入後可管理 Pro 授權、連結本機工具。轉檔不需要帳號。'
      : 'Manage Pro and connect your local tools. No account is needed to convert files.'}</p></header>
    <Show when={options()} keyed>{auth => <SignInForm initialAuth={auth} locale={locale()} next={signInReturnPath(location.pathname, location.search)} error={single(search.error)} />}</Show>
    <p class="muted mt-4 text-sm">{zh() ? <>請參閱{privacy()}與{terms()}。</> : <>See our {privacy()} and {terms()}.</>}</p>
  </section>;
}
