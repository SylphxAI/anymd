import { A, useLocation } from '@solidjs/router';
import type { Capabilities } from '@/lib/auth-capabilities';
import { createEffect, createSignal, on, onCleanup, Show } from 'solid-js';

export type Session = { authConfigured: boolean; user: { name: string | null; email: string | null } | null } & Partial<Capabilities>;

export function useSession(includeProviders = false) {
  const route = useLocation();
  const [state, setState] = createSignal<Session | 'error' | null>(null);
  createEffect(on(() => route.pathname, () => {
    setState(null);
    const controller = new AbortController();
    onCleanup(() => controller.abort());
    fetch('/api/auth/session' + (includeProviders ? '?providers' : ''), { cache: 'no-store', signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error('Session unavailable');
        const session = await response.json();
        if (!controller.signal.aborted) setState(session);
      })
      .catch(() => { if (!controller.signal.aborted) setState('error'); });
  }));
  return state;
}

export function GoogleMark() {
  return <svg aria-hidden="true" width="16" height="16" viewBox="0 0 48 48"><path fill="#4285F4" d="M46 24.5c0-1.6-.1-3.1-.4-4.5H24v8.5h12.4a10.6 10.6 0 0 1-4.6 7v5.8h7.4C43.6 37 46 31.3 46 24.5Z"/><path fill="#34A853" d="M24 47c6.2 0 11.5-2 15.2-5.7l-7.4-5.8c-2.1 1.4-4.7 2.2-7.8 2.2-6 0-11.1-4.1-13-9.6H3.4V34A23 23 0 0 0 24 47Z"/><path fill="#FBBC05" d="M11 28a14 14 0 0 1 0-8v-5.9H3.4a23 23 0 0 0 0 19.9Z"/><path fill="#EA4335" d="M24 10.3c3.4 0 6.4 1.2 8.8 3.5l6.6-6.5A22 22 0 0 0 24 1 23 23 0 0 0 3.4 14l7.6 6c1.9-5.6 7-9.7 13-9.7Z"/></svg>;
}

export function SessionAction(props: { primary?: boolean }) {
  const session = useSession();
  const route = useLocation();
  const signedIn = () => {
    const state = session();
    return state && state !== 'error' && state.user;
  };
  const href = () => '/sign-in?next=' + encodeURIComponent(route.pathname === '/' ? '/dashboard' : route.pathname);
  const buttonClass = () => props.primary ? 'button-primary' : '';
  return <Show when={signedIn()} fallback={<A class={'button sign-in-button ' + buttonClass()} href={href()}>Sign in</A>}>
    <A href="/dashboard" class={'button ' + buttonClass()}>Dashboard</A>
  </Show>;
}

export function SignOut() {
  const [failed, setFailed] = createSignal(false);
  return <><button class="button" type="button" onClick={async () => {
    try {
      const response = await fetch('/api/auth/sign-out', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
      if (!response.ok) throw new Error('Sign-out failed');
      // Reload session consumers even when signing out on the homepage.
      location.replace('/');
    } catch { setFailed(true); }
  }}>Sign out</button>{failed() && <span role="alert">Could not sign out. Please try again.</span>}</>;
}
