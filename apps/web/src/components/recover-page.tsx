import { PageMeta } from '@/components/page-meta';
import { useSearchParams } from '@solidjs/router';
import { single } from '@/lib/request-query';
import { Claim, LegacyRecovery } from '@/components/commerce';

export function Recover(props: { title?: string }) {
  const [query] = useSearchParams();
  return <section class="wrap page-shell"><PageMeta title={props.title ?? 'Link a previous purchase'} canonical="/recover" robots="noindex" /><header class="page-heading"><span class="eyebrow">ACCOUNT / RECOVERY</span><h1>Bring your Pro purchase.</h1><p>Link an earlier purchase to the Anymd account you use now.</p></header><LegacyRecovery /><Claim checkoutSessionId={single(query.session_id)} /></section>;
}
