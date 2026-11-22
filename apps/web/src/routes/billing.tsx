import { PageMeta } from '@/components/page-meta';
import { AccountPanel } from '@/components/commerce';
export default function Billing() { return <section class="wrap page-shell"><PageMeta title="Billing" robots="noindex" /><header class="page-heading"><span class="eyebrow">WORKSPACE / BILLING</span><h1>Billing and invoices.</h1><p>Check your Pro access and find your invoices in Stripe.</p></header><AccountPanel billing /></section>; }
