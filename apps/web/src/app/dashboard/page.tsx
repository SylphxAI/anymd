import { AccountPanel } from '@/components/commerce';
export const metadata = { title:'Dashboard', robots: {index:false} };
export default function Dashboard() { return <section className="wrap page-shell"><header className="page-heading"><span className="eyebrow">WORKSPACE / ACCOUNT</span><h1>Your Anymd.</h1><p>Check your plan, manage billing and connect your local tools.</p></header><AccountPanel /></section>; }
