import { PageMeta } from '@/components/page-meta';
import { AccountPanel } from '@/components/commerce';
export function Dashboard(props: { title?: string }) { return <section class="wrap page-shell"><PageMeta title={props.title ?? 'Dashboard'} robots="noindex" /><header class="page-heading"><span class="eyebrow">WORKSPACE / ACCOUNT</span><h1>Your Anymd.</h1><p>Check your plan, manage billing and connect your local tools.</p></header><AccountPanel /></section>; }
