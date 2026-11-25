import { CampaignLink } from '@/lib/attribution-link';
import type { ConversionSource } from '@/lib/analytics';
import { translatedPaths, type Locale } from '@/lib/marketing-locale';
import { studioCopy } from '@/lib/studio-copy';
import { findQuoteMatches, normalizeQuote, type QuoteMatch, type SourceUnit } from '@/lib/studio-quote';
import { ArrowRight, Check } from 'lucide-solid';
import { createSignal, For, Show } from 'solid-js';
import { useCookieConsent } from '../cookie-consent';
import { citeHeadingId, citeProofId } from './cite-preview';

// Verbatim phrases from the bundled samples' Rust/WASM source units.
const sampleQuotes = {
  pdf: ['Quarterly Field Report', 'emits one clean paragraph', 'never leave this page'],
  html: ['Field Notes: Water Survey', 'sampled the lower river', 'the lowest turbidity'],
};
const surface = { entry_surface: 'studio_quote_match' } as const;

function revealProof() {
  const proof = document.getElementById(citeProofId);
  if (proof instanceof HTMLDetailsElement) proof.open = true;
  document.getElementById(citeHeadingId)?.focus({ preventScroll: true });
}

export default function QuoteSearch(props: {
  locale: Locale;
  output: { format: string; sourceKind: ConversionSource; source_units: SourceUnit[] };
}) {
  const t = () => studioCopy[props.locale];
  const consent = useCookieConsent();
  const [quote, setQuote] = createSignal('');
  const [match, setMatch] = createSignal<QuoteMatch[] | null>(null);
  const suggestions = () => {
    const { format, sourceKind } = props.output;
    if (sourceKind === 'sample' && (format === 'pdf' || format === 'html')) return sampleQuotes[format];
  };

  function search(text: string) {
    const needle = normalizeQuote(text);
    if (needle.length < 3) return;
    const matches = findQuoteMatches(props.output.source_units, needle);
    setMatch(matches);
    consent.trackBehavioral('quote_searched', { matched: matches.length > 0 });
  }

  return <form class="studio-quote stack" onSubmit={event => { event.preventDefault(); search(quote()); }}>
    <div><h2>{t().quoteTitle}</h2><p class="muted">{t().quoteHelp}</p></div>
    <label class="field-label">{t().quoteLabel}
      <input class="field" required minLength={3} maxLength={500} placeholder={t().quotePlaceholder}
        value={quote()} onInput={event => { setQuote(event.currentTarget.value); setMatch(null); }} />
    </label>
    <Show when={suggestions()}>{phrases => <div class="studio-quote-samples" role="group" aria-label={t().quoteSamples}>
      <p class="muted">{t().quoteSamples}</p>
      <For each={phrases()}>{phrase => <button type="button" class="button" lang="en"
        onClick={() => { setQuote(phrase); search(phrase); }}>{phrase}</button>}</For>
    </div>}</Show>
    <button class="button justify-self-start" type="submit">{t().find} <ArrowRight size={15} /></button>
    <Show when={match()}>{matches => <div class="studio-quote-results">
      <p class="notice" role="status">{matches().length ? <><Check class="inline" size={16} aria-hidden="true" /> {t().found(matches().map(item => item.label))}</> : t().noMatch}</p>
      <Show when={matches().length}>
        <ul class="studio-excerpts">
          <For each={matches()}>{item => <li>
            <span class="studio-excerpt-label">{t().sourceLabel(item.label)}</span>
            <p>{item.before}<mark>{item.matched}</mark>{item.after}</p>
          </li>}</For>
        </ul>
        <Show when={props.output.format === 'pdf'}>
          <aside class="studio-quote-proof">
            <p>{t().quoteProofStart}<code>cite_check</code>{t().quoteProofEnd}</p>
            <div class="studio-quote-proof-links">
              <a class="inline-link" href={'#' + citeHeadingId} onClick={revealProof}>{t().quoteSeeProof}</a>
              <CampaignLink class="inline-link" href={(props.locale === 'zh-TW' ? translatedPaths['/pricing'] : '/pricing') + '?entry_surface=' + surface.entry_surface}
                onClick={() => consent.trackBehavioral('pro_cta_clicked', surface)}>{t().quoteSeePro}</CampaignLink>
            </div>
          </aside>
        </Show>
      </Show>
    </div>}</Show>
  </form>;
}
