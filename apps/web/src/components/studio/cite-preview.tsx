import { CampaignLink } from '@/lib/attribution-link';
import { defaultCurrency, formatPrice, proAmount } from '@/lib/catalog';
import { translatedPaths, type Locale } from '@/lib/marketing-locale';
import { studioCopy } from '@/lib/studio-copy';
import { useLocation } from '@solidjs/router';
import { ArrowRight, ScanLine, ShieldCheck } from 'lucide-solid';
import { onMount } from 'solid-js';
import { useCookieConsent } from '../cookie-consent';
import { PdfQuoteCrop } from './pdf-quote-crop';
import proof from './sample-cite-proof.json';

export const citeHeadingId = 'studio-cite-heading';
export const citeProofId = 'studio-cite-proof';

export default function CitePreview(props: { locale: Locale }) {
  const consent = useCookieConsent();
  const location = useLocation();
  let details!: HTMLDetailsElement;
  let heading!: HTMLHeadingElement;
  onMount(() => {
    if (location.hash !== '#' + citeHeadingId) return;
    details.open = true;
    // Keep the fragment in view as the conversion adds output above the proof.
    heading.focus({ preventScroll: true });
    heading.scrollIntoView();
  });
  const t = () => studioCopy[props.locale].proof;
  const price = () => formatPrice(proAmount(defaultCurrency), defaultCurrency, props.locale);
  const surface = {entry_surface:'studio_cite_preview'} as const;
  return (
    <section class="cite-preview" aria-labelledby={citeHeadingId}>
      <div class="cite-preview-intro">
        <div class="stack">
          <span class="eyebrow"><ScanLine size={16} aria-hidden="true" /> {t().eyebrow}</span>
          <h2 ref={heading} id={citeHeadingId} tabIndex={-1}>{t().heading}</h2>
        </div>
        <CampaignLink class="button button-primary" href={(props.locale === 'zh-TW' ? translatedPaths['/pricing'] : '/pricing') + '?entry_surface=studio_cite_preview'} onClick={() => consent.trackBehavioral('pro_cta_clicked', surface)}>{t().upgrade(price())} <ArrowRight size={16} aria-hidden="true" /></CampaignLink>
      </div>
      <p class="muted">{t().helpStart}<code>cite_check</code>{t().helpEnd}</p>
      <details id={citeProofId} class="cite-preview-details" ref={details}>
        <summary onClick={event => {
          // Native toggle events also fire when we open the proof for a deep link.
          if (event.isTrusted && !details.open) consent.trackBehavioral('paywall_view', surface);
        }}>{t().see}</summary>
        <div class="cite-preview-evidence">
          <figure class="cite-preview-figure">
            <figcaption><span>sample.pdf · {t().page(proof.page)}</span><span>{t().crop}</span></figcaption>
            <PdfQuoteCrop lazy label={t().image(proof.page, proof.quote)} />
          </figure>
          <div class="cite-preview-match">
            <span class="cite-preview-badge"><ShieldCheck size={16} aria-hidden="true" /> {t().verified}</span>
            <blockquote>“{proof.quote}”</blockquote>
            <p class="muted">{t().recordedStart}<code>cite_check</code>{t().recordedEnd}</p>
          </div>
          <details class="cite-preview-json">
            <summary>{t().details}</summary>
            <pre aria-label={t().json}>{JSON.stringify(proof, null, 2)}</pre>
          </details>
        </div>
      </details>
      <p class="cite-preview-footnote muted">{t().footnote}</p>
    </section>
  );
}
