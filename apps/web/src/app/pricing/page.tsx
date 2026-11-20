import { Check } from 'lucide-react';
import Link from 'next/link';
import { Checkout } from '@/components/commerce';

export const metadata = { title: 'Pricing', alternates: { canonical: '/pricing' } };
type Props = { searchParams: Promise<{ checkout?: string; session_id?: string }> };

const freeFeatures = [
  'PDF, Office, EPUB, web and media conversion',
  'Command-line tools and MCP server',
  'Page boundaries and structured Markdown',
];
const proFeatures = [
  'Everything in Free OSS',
  'PDF cite-check with exact page & bounding-box proof',
  'Hashed video-frame timeline extraction & citations',
  'Local CLI & MCP sync via anymd login across your machines · 100-year licence',
];

function PlanFeatures({ points }: { points: string[] }) {
  return <ul className="stack my-3">{points.map((point) => <li key={point} className="flex gap-2"><Check aria-hidden="true" size={17} className="mt-1 shrink-0 text-[var(--accent)]" />{point}</li>)}</ul>;
}

export default async function Pricing({searchParams}: Props) {
  const query = await searchParams;
  return <section className="wrap page-shell">
    <header className="page-heading"><span className="eyebrow">PLANS / 01</span><h1>Open source first.</h1><p>Convert documents for free. Add Pro when you need to show exactly where a quote or video frame came from.</p></header>
    {query.checkout === 'cancelled' && <p role="status" className="notice mb-5">Checkout was closed. No changes were made to your plan.</p>}
    {typeof query.session_id === 'string' && <p className="notice mb-5"><Link className="inline-link" href={'/recover?session_id=' + encodeURIComponent(query.session_id)}>Check or link your purchase</Link></p>}
    <div className="columns">
      <article className="panel stack"><span className="eyebrow">CORE TOOLKIT</span><h2>Free OSS</h2><div className="price">Free</div><p className="muted">MIT licensed. Run locally, with no account or API key.</p><PlanFeatures points={freeFeatures} /><Link className="button justify-self-start" href="/docs">Install MCP / CLI</Link></article>
      <article className="panel stack"><span className="eyebrow">SOURCE VERIFICATION</span><h2>Anymd Pro</h2><PlanFeatures points={proFeatures} /><Checkout /></article>
    </div>
    <p className="mt-6 muted">Already purchased? <Link href="/recover" className="inline-link">Link a previous purchase</Link>.</p>
  </section>;
}
