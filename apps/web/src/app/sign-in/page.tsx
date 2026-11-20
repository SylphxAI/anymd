import type { Metadata } from 'next';
import Link from 'next/link';
import { SignInForm } from '@/components/sign-in';
import { safeNext } from '@/lib/http';

export const metadata: Metadata = {title:'Sign in', robots:{index:false}};
export default async function SignIn({searchParams}: {searchParams:Promise<{next?:string; error?:string}>}) {
  const query = await searchParams;
  return <section className="wrap page-shell auth-shell"><header className="page-heading"><span className="eyebrow">ACCOUNT / SIGN IN</span><h1>Sign in to Anymd.</h1><p>Manage Pro and connect your local tools. No account is needed to convert files.</p></header><SignInForm next={safeNext(query.next)} error={typeof query.error === 'string' ? query.error : undefined} /><p className="muted mt-4 text-sm">See our <Link className="inline-link" href="/legal/privacy">privacy notice</Link> and <Link className="inline-link" href="/legal/terms">site terms</Link>.</p></section>;
}
