import type { Metadata } from 'next';
import { headers } from 'next/headers';
import Link from 'next/link';
import { SignInForm } from '@/components/sign-in';
import { authConfig, clientCapabilities } from '@/lib/auth';
import { noCapabilities } from '@/lib/auth-capabilities';
import { safeNext } from '@/lib/http';

export const dynamic = 'force-dynamic';
export const metadata: Metadata = {title:'Sign in', robots:{index:false}};
export default async function SignIn({searchParams}: {searchParams:Promise<{next?:string; error?:string}>}) {
  const config = authConfig();
  const query = await searchParams;
  const capabilities = config
    ? await clientCapabilities(new Request('http://localhost/sign-in', { headers: await headers() }), config).catch(() => noCapabilities)
    : noCapabilities;
  const initialAuth = { authConfigured: Boolean(config), ...capabilities };
  return <section className="wrap page-shell auth-shell"><header className="page-heading"><span className="eyebrow">ACCOUNT / SIGN IN</span><h1>Sign in to Anymd.</h1><p>Manage Pro and connect your local tools. No account is needed to convert files.</p></header><SignInForm initialAuth={initialAuth} next={safeNext(query.next)} error={typeof query.error === 'string' ? query.error : undefined} /><p className="muted mt-4 text-sm">See our <Link className="inline-link" href="/legal/privacy">privacy notice</Link> and <Link className="inline-link" href="/legal/terms">site terms</Link>.</p></section>;
}
