import { CampaignLink } from '@/lib/attribution-link';
import { ArrowRight, ArrowUpRight, FileText, Globe, Terminal } from 'lucide-solid';
import { useLocation } from '@solidjs/router';
import { For } from 'solid-js';
import { CopyButton } from '@/components/copy';
import { HomeConversionDemo } from '@/components/home-conversion-demo';
import { PageMeta } from '@/components/page-meta';
import { Faq, StructuredData, faqSchema, softwareSchema, freeOffer, proOffer } from '@/components/search-details';
import { useBrowserLocales } from '@/lib/browser-locales';
import { defaultCurrency, formatPrice, proAmount } from '@/lib/catalog';
import { homeCopy } from '@/lib/marketing-copy';
import { localeFromPath, localizedPath } from '@/lib/marketing-locale';

const npmSetup = 'npx -y @sylphx/anymd setup';
const pipInstall = 'pip install anymd';
const uvxRun = 'uvx anymd';

export default function Home() {
  const route = useLocation();
  const locale = () => localeFromPath(route.pathname);
  const t = () => homeCopy[locale()];
  const pricing = (): '/pricing' | '/zh-TW/pricing' => locale() === 'zh-TW' ? '/zh-TW/pricing' : '/pricing';
  const browserLocales = useBrowserLocales();
  const price = () => formatPrice(proAmount(defaultCurrency), defaultCurrency, locale() === 'zh-TW' ? 'zh-TW' : browserLocales());
  return <>
    <PageMeta canonical="/" />
    <StructuredData nodes={[softwareSchema(freeOffer(), proOffer(defaultCurrency, pricing())), faqSchema(localizedPath('/', locale()), t().questions)]} />
    <section class="hero wrap">
      <div class="hero-copy">
        <div class="eyebrow"><span class="status-dot" /> {t().eyebrow}</div>
        <h1>{t().title}<br/><em>{t().accent}</em></h1>
        <p class="hero-summary">{t().summary}</p>
        <div class="hero-actions"><CampaignLink href="/studio" class="button button-primary">{t().tryStudio} <ArrowRight size={16} /></CampaignLink><CampaignLink href={pricing()} class="button">{t().getPro} — {price()}</CampaignLink></div>
        <div class="hero-install" role="group" aria-label={t().install}>
          <div class="install-line">
            <code><span aria-hidden="true">$ </span>{npmSetup}</code>
            <CopyButton text={npmSetup} label={t().copyInstall} copied={t().copied} copyFailed={t().copyFailed} compact />
          </div>
          <div class="install-line">
            <div class="install-command"><code>{pipInstall}</code><CopyButton text={pipInstall} label={t().copyPip} copied={t().copied} copyFailed={t().copyFailed} compact /></div>
            <div class="install-command"><span>{t().or}</span><code>{uvxRun}</code><CopyButton text={uvxRun} label={t().copyUvx} copied={t().copied} copyFailed={t().copyFailed} compact /></div>
            <a class="install-pypi" href="https://pypi.org/project/anymd/">PyPI <ArrowUpRight size={12} aria-hidden="true" /></a>
          </div>
        </div>
      </div>
      <div class="hero-preview">
        <CampaignLink href="/studio" class="preview-link">{t().openStudio} <ArrowRight size={15} /></CampaignLink>
        <HomeConversionDemo locale={locale()} />
      </div>
    </section>
    <section class="format-strip" aria-label={t().formats}><div class="wrap formats">
      <span class="eyebrow">{t().localFiles}</span><span><FileText /> PDF</span><span><FileText /> DOCX</span><span><FileText /> HTML</span><span><FileText /> EPUB</span><span class="format-rule"><Globe /> {t().urls}</span>
    </div></section>
    <section class="wrap home-workflow"><div><span class="eyebrow">{t().workflow}</span><h2>{t().workflowTitle}<br/>{t().workflowAccent}</h2></div><div class="workflow-steps">
      <For each={t().steps}>{([title, detail], index) => <div><span>0{index() + 1}</span><div><h3>{title}</h3><p>{detail}</p></div></div>}</For>
    </div></section>
    <section class="wrap home-bottom"><div class="pro-panel"><div><span class="eyebrow"><Terminal size={16} /> ANYMD PRO</span><h2>{t().proTitle}</h2><p>{t().proDetail}</p></div><CampaignLink href={pricing()} class="button button-primary">{t().seePrice} <ArrowRight size={16} /></CampaignLink></div></section>
    <section class="wrap home-faq" aria-labelledby="faq-title">
      <h2 id="faq-title">{t().faq}</h2>
      <Faq questions={t().questions} />
    </section>
  </>;
}
