import { PageMeta } from '@/components/page-meta';
import { A, useLocation, useSearchParams } from '@solidjs/router';
import { For } from 'solid-js';
import { single } from '@/lib/request-query';
import { Check } from 'lucide-solid';
import { Checkout } from '@/components/commerce';
import { Faq, StructuredData, faqSchema, softwareSchema, freeOffer, proOffer } from '@/components/search-details';
import { useBrowserLocales } from '@/lib/browser-locales';
import { defaultCurrency, formatPrice, isCurrency, proAmount } from '@/lib/catalog';
import { pricingCopy } from '@/lib/marketing-copy';
import { localeFromPath, localizedPath } from '@/lib/marketing-locale';

function PlanFeatures(props: { points: readonly string[] }) {
  return <ul class="plan-features"><For each={props.points}>{(point) => <li class="flex gap-2"><Check aria-hidden="true" size={17} class="mt-1 shrink-0 text-[var(--accent)]" />{point}</li>}</For></ul>;
}

export function Pricing(props: { title?: string }) {
  const route = useLocation();
  const locale = () => localeFromPath(route.pathname);
  const t = () => pricingCopy[locale()];
  const path = () => localizedPath('/pricing', locale());
  const [query] = useSearchParams();
  const sessionId = () => single(query.session_id);
  const currency = () => {
    const selected = single(query.currency);
    return isCurrency(selected) ? selected : defaultCurrency;
  };
  const browserLocales = useBrowserLocales();
  const locales = () => locale() === 'zh-TW' ? ['zh-TW'] : browserLocales();
  const questions = () => [{ question: t().costQuestion, answer: t().costAnswer(formatPrice(proAmount(currency()), currency(), locales())) }, ...t().questions];
  return <section class="wrap page-shell pricing-page"><PageMeta title={props.title ?? t().title} canonical="/pricing" description={t().description} />
    <StructuredData nodes={[softwareSchema(freeOffer(currency()), proOffer(currency(), path())), faqSchema(path(), questions())]} />
    <header class="page-heading"><span class="eyebrow">{t().eyebrow}</span><h1>{t().heading}</h1><p>{t().intro}</p></header>
    {query.checkout === 'cancelled' && <p role="status" class="notice mb-5">{t().cancelled}</p>}
    {sessionId() && <p class="notice mb-5"><A class="inline-link" href={'/recover?session_id=' + encodeURIComponent(sessionId() ?? '')}>{t().recover}</A></p>}
    <div class="columns">
      <article class="panel stack"><span class="eyebrow">{t().freeLabel}</span><h2>{t().free}</h2><div class="price">{formatPrice(0, currency(), locales())}</div><p class="muted">{t().freeDetails}</p><PlanFeatures points={t().freeFeatures} /><A class="button justify-self-start" href="/docs">{t().install}</A></article>
      <article class="panel stack pro-plan">
        <div class="pro-plan-title"><span class="eyebrow">{t().proLabel}</span><h2>{t().pro}</h2></div>
        {locale() === 'zh-TW' && <p class="pro-plan-payment muted">{t().oneTime}</p>}
        <PlanFeatures points={t().proFeatures} />
        <div class="pricing-checkout"><Checkout currency={currency()} locales={locales()} locale={locale()} /></div>
      </article>
    </div>
    <section class="pricing-faq" aria-labelledby="faq-title">
      <h2 id="faq-title">{t().faq}</h2>
      <Faq questions={questions()} />
    </section>
  </section>;
}
