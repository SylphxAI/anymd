import { Claim, LegacyRecovery } from '@/components/commerce';

export const metadata = { title: 'Link a previous purchase', alternates: { canonical: '/recover' }, robots: {index:false} };
export default async function Recover({ searchParams }: { searchParams: Promise<{ session_id?: string }> }) {
  const { session_id } = await searchParams;
  return <section className="wrap page-shell"><header className="page-heading"><span className="eyebrow">ACCOUNT / RECOVERY</span><h1>Bring your Pro purchase.</h1><p>Link an earlier purchase to the Anymd account you use now.</p></header><LegacyRecovery /><Claim checkoutSessionId={typeof session_id === 'string' ? session_id : undefined} /></section>;
}
