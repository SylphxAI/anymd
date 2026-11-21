'use client';

import { usePathname, useRouter } from 'next/navigation';
import Link from 'next/link';
import { useEffect, useState } from 'react';

export type Session = { authConfigured: boolean; providers?: string[]; user: { name: string | null; email: string | null } | null };

export function useSession(includeProviders = false) {
  const pathname = usePathname();
  const [state, setState] = useState<Session | 'error' | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    fetch('/api/auth/session' + (includeProviders ? '?providers' : ''), { cache: 'no-store', signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error('Session unavailable');
        setState(await response.json());
      })
      .catch(() => { if (!controller.signal.aborted) setState('error'); });
    return () => controller.abort();
  }, [pathname, includeProviders]);
  return state;
}

export function GoogleMark() {
  return <svg aria-hidden="true" width="16" height="16" viewBox="0 0 48 48"><path fill="#4285F4" d="M46 24.5c0-1.6-.1-3.1-.4-4.5H24v8.5h12.4a10.6 10.6 0 0 1-4.6 7v5.8h7.4C43.6 37 46 31.3 46 24.5Z"/><path fill="#34A853" d="M24 47c6.2 0 11.5-2 15.2-5.7l-7.4-5.8c-2.1 1.4-4.7 2.2-7.8 2.2-6 0-11.1-4.1-13-9.6H3.4V34A23 23 0 0 0 24 47Z"/><path fill="#FBBC05" d="M11 28a14 14 0 0 1 0-8v-5.9H3.4a23 23 0 0 0 0 19.9Z"/><path fill="#EA4335" d="M24 10.3c3.4 0 6.4 1.2 8.8 3.5l6.6-6.5A22 22 0 0 0 24 1 23 23 0 0 0 3.4 14l7.6 6c1.9-5.6 7-9.7 13-9.7Z"/></svg>;
}

export function SessionAction({ primary = false }: { primary?: boolean }) {
  const state = useSession();
  const path = usePathname();
  if (state && state !== 'error' && state.user) return <Link href="/dashboard" className={'button ' + (primary ? 'button-primary' : '')}>Dashboard</Link>;
  const href = '/sign-in?next=' + encodeURIComponent(path === '/' ? '/dashboard' : path);
  return <Link className={'button sign-in-button ' + (primary ? 'button-primary' : '')} href={href}>Sign in</Link>;
}

export function SignOut() {
  const router = useRouter();
  const [failed, setFailed] = useState(false);
  return <><button className="button" type="button" onClick={async () => {
    try {
      const response = await fetch('/api/auth/sign-out', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
      if (!response.ok) throw new Error('Sign-out failed');
      router.replace('/');
      router.refresh();
    } catch { setFailed(true); }
  }}>Sign out</button>{failed && <span role="alert">Could not sign out. Please try again.</span>}</>;
}
