import { PageMeta } from '@/components/page-meta';
import { useSearchParams } from '@solidjs/router';
import { single } from '@/lib/request-query';
import { Check } from 'lucide-solid';
import { A } from '@solidjs/router';
import { Checkout } from '@/components/commerce';
import { Faq, StructuredData, faqSchema, softwareSchema, freeOffer, proOffer, type Question } from '@/components/search-details';
import { usdPriceLabel } from '@/lib/catalog';

const freeFeatures = [
  'PDF, Office, EPUB, web and media conversion',
  'Command-line tools and MCP server',
  'Page boundaries and structured Markdown',
];
const proFeatures = [
  'Everything in Free OSS',
  'PDF cite-check with exact page & bounding-box proof',
  'Hashed video-frame timeline extraction & citations',
  'Works offline after activation on each machine',
];

const questions = [
  { question: 'How much does Anymd Pro cost?', answer: `Anymd Pro costs ${usdPriceLabel} USD once, including tax. There is no subscription. You can select another supported currency before checkout.` },
  { question: 'How do I activate Pro?', answer: 'Sign in and pay securely, then run anymd login. Use the same account in your browser to activate the CLI and MCP server on that machine.' },
  { question: 'What can I do for free?', answer: 'Convert supported documents in Studio or use the open-source CLI and MCP server for local conversion. No account or API key is needed.' },
  { question: 'What does Pro add to the CLI and MCP server?', answer: 'Pro can verify PDF quotes against the exact page and location, and find and cite exact video frames. These tools run locally.' },
] satisfies Question[];

function PlanFeatures({ points }: { points: string[] }) {
  return <ul class="plan-features">{points.map((point) => <li class="flex gap-2"><Check aria-hidden="true" size={17} class="mt-1 shrink-0 text-[var(--accent)]" />{point}</li>)}</ul>;
}

export function Pricing(props: { title?: string }) {
  const [query] = useSearchParams();
  const sessionId = () => single(query.session_id);
  return <section class="wrap page-shell pricing-page"><PageMeta title={props.title ?? 'Pricing'} canonical="/pricing" />
    <StructuredData nodes={[softwareSchema(freeOffer, proOffer), faqSchema('/pricing', questions)]} />
    <header class="page-heading"><span class="eyebrow">PLANS / 01</span><h1>Open source first.</h1><p>Convert documents for free. Add Pro when you need to show exactly where a quote or video frame came from.</p></header>
    {query.checkout === 'cancelled' && <p role="status" class="notice mb-5">Checkout was closed. No changes were made to your plan.</p>}
    {sessionId() && <p class="notice mb-5"><A class="inline-link" href={'/recover?session_id=' + encodeURIComponent(sessionId() ?? '')}>Link a legacy purchase</A></p>}
    <div class="columns">
      <article class="panel stack"><span class="eyebrow">CORE TOOLKIT</span><h2>Free OSS</h2><div class="price">Free</div><p class="muted">MIT licensed. Run locally, with no account or API key.</p><PlanFeatures points={freeFeatures} /><A class="button justify-self-start" href="/docs">Install MCP / CLI</A></article>
      <article class="panel stack"><span class="eyebrow">SOURCE VERIFICATION</span><h2>Anymd Pro</h2><PlanFeatures points={proFeatures} /><Checkout /></article>
    </div>
    <p class="mt-6 muted">Bought Pro before account checkout? <A href="/recover" class="inline-link">Recover your purchase</A>.</p>
    <section class="pricing-faq" aria-labelledby="faq-title">
      <h2 id="faq-title">Frequently asked questions</h2>
      <Faq questions={questions} />
    </section>
  </section>;
}
