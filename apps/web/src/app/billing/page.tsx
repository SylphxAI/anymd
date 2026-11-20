import { AccountPanel } from '@/components/commerce';
export const metadata = { title:'Billing', robots: {index:false} };
export default function Billing() { return <section className="wrap page-shell"><header className="page-heading"><span className="eyebrow">WORKSPACE / BILLING</span><h1>Billing and invoices.</h1><p>Check your Pro access and find your invoices in Stripe.</p></header><AccountPanel billing /></section>; }
