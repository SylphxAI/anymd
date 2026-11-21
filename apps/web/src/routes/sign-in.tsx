import { A, createAsync, useSearchParams } from '@solidjs/router';
import { Show } from 'solid-js';
import { PageMeta } from '@/components/page-meta';
import { SignInForm } from '@/components/sign-in';
import { signInOptions } from '@/lib/auth-pages';
import { safeNext } from '@/lib/safe-next';
import { single } from '@/lib/request-query';

export const route = { preload: () => signInOptions() };
export default function SignIn() {
  const [search] = useSearchParams();
  const options = createAsync(() => signInOptions(), { deferStream: true });
  return <section class="wrap page-shell auth-shell">
    <PageMeta title="Sign in" robots="noindex" />
    <header class="page-heading"><span class="eyebrow">ACCOUNT / SIGN IN</span><h1>Sign in to Anymd.</h1><p>Manage Pro and connect your local tools. No account is needed to convert files.</p></header>
    <Show when={options()} keyed>{auth => <SignInForm initialAuth={auth} next={safeNext(search.next)} error={single(search.error)} />}</Show>
    <p class="muted mt-4 text-sm">See our <A class="inline-link" href="/legal/privacy">privacy notice</A> and <A class="inline-link" href="/legal/terms">site terms</A>.</p>
  </section>;
}
