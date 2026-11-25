import { createAsync, query, useLocation } from '@solidjs/router';
import { HttpStatusCode } from '@solidjs/start';
import { createEffect, createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch, untrack } from 'solid-js';
import { useCookieConsent } from '@/components/cookie-consent';
import { PageMeta } from '@/components/page-meta';
import { CampaignLink } from './attribution-link';
import { carryCampaign } from './campaign';
import { campaignDocHtml } from './docs-campaign';
import type { renderDoc } from './docs-source';

const nav = [['Getting started','getting-started'],['CLI','cli'],['Python API','python'],['MCP tools','tools'],['Formats','formats'],['Cite-check','cite-check'],['Video timeline','video-timeline'],['Benchmarks','benchmarks'],['Migration','migration']] as const;

type Document = NonNullable<ReturnType<typeof renderDoc>>;

export const loadDoc = query(async (slug: string, legacyDocs = false): Promise<Document | null | 'unavailable'> => {
  if (import.meta.env.SSR) {
    const { renderDoc } = await import('./docs-source');
    return renderDoc(slug, legacyDocs);
  }
  try {
    const params = new URLSearchParams({ slug });
    if (legacyDocs) params.set('legacy', '1');
    const response = await fetch('/api/docs?' + params);
    if (response.status === 404) return null;
    if (response.ok) {
      const article: Document | null = await response.json();
      if (typeof article?.html === 'string' && typeof article.title === 'string' && typeof article.canonical === 'string') return article;
    }
  } catch {
    // A failed fetch must not crash the page or masquerade as a missing guide.
  }
  return 'unavailable';
}, 'documentation');

function NotFound() {
  return <section class="wrap page-shell"><HttpStatusCode code={404} /><PageMeta title="Page not found" robots="noindex" /><h1>Page not found.</h1><p><CampaignLink href="/docs" class="inline-link">Browse the documentation</CampaignLink></p></section>;
}

function DocSearch(props: { entries: { title: string; description?: string; path: string }[] }) {
  const [filter, setFilter] = createSignal('');
  const terms = () => filter().toLowerCase().trim().split(/\s+/).filter(Boolean);
  const matches = () => props.entries.filter(entry => {
    const text = (entry.title + ' ' + (entry.description ?? '') + ' ' + entry.path).toLowerCase();
    return terms().every(term => text.includes(term));
  });
  return <section class="docs-search" aria-label="Filter documentation">
    <label class="field-label">Search documentation<input class="field" type="search" placeholder="Python, OCR, MCP…" value={filter()} onInput={event => setFilter(event.currentTarget.value)} /></label>
    <Show when={terms().length}>
      <p class="muted" role="status">{matches().length ? matches().length + (matches().length === 1 ? ' guide found' : ' guides found') : 'No guides found. Try a different term.'}</p>
      <ul><For each={matches()}>{(entry) => <li><CampaignLink href={entry.path} class="inline-link" rel="external">{entry.title}</CampaignLink>{entry.description && <p class="muted">{entry.description}</p>}</li>}</For></ul>
    </Show>
  </section>;
}

export function Doc(props: { name: string; legacyDocs?: boolean; index?: boolean }) {
  const consent = useCookieConsent();
  const route = useLocation();
  const [interactive, setInteractive] = createSignal(false);
  const [feedback, setFeedback] = createSignal('');
  let timeout: ReturnType<typeof setTimeout> | undefined;
  let previous: HTMLButtonElement | undefined;
  let attempt = 0;
  onMount(() => setInteractive(true));
  const reset = () => {
    clearTimeout(timeout);
    if (previous) previous.textContent = 'Copy code';
    setFeedback('');
  };
  onCleanup(() => { attempt++; clearTimeout(timeout); });
  async function copyCode(event: MouseEvent) {
    if (!(event.target instanceof HTMLButtonElement) || !event.target.classList.contains('doc-copy')) return;
    const button = event.target;
    const code = button.parentElement?.querySelector('pre > code')?.textContent;
    if (code == null) return;
    const run = ++attempt;
    reset();
    previous = button;
    let message: string;
    try { await navigator.clipboard.writeText(code); message = 'Copied'; }
    catch { message = 'Select and copy'; }
    if (run !== attempt) return;
    if (message === 'Copied') consent.trackBehavioral('output_copied', {surface:'docs_code'});
    button.textContent = message;
    setFeedback(message);
    timeout = setTimeout(reset, 1800);
  }
  const campaignSearch = createMemo(() => consent.carryCampaign() ? route.search : '');
  const doc = createAsync(() => loadDoc(props.name, props.legacyDocs), { deferStream: true });
  const article = createMemo(() => {
    const result = doc();
    return result && result !== 'unavailable' ? result : undefined;
  });
  // Put real hrefs in SSR HTML. Track document changes here, not consent changes.
  const html = createMemo(() => campaignDocHtml(article()?.html ?? '', untrack(campaignSearch)));
  let body: HTMLElement | undefined;
  createEffect(() => {
    const search = campaignSearch();
    void html();
    // Update only hrefs after hydration so focus, selections and copy feedback survive.
    for (const link of body?.querySelectorAll<HTMLAnchorElement>('a[data-doc-href]') ?? []) {
      const base = link.dataset.docHref!;
      const href = carryCampaign(base, search);
      if (link.getAttribute('href') !== href) link.setAttribute('href', href);
    }
  });
  return <Switch>
    <Match when={doc() === null}><NotFound /></Match>
    <Match when={doc() === 'unavailable'}>
      <section class="wrap page-shell"><PageMeta title="Documentation unavailable" robots="noindex" /><h1>Documentation unavailable</h1><p>Reload the page to try again.</p></section>
    </Match>
    <Match when={article()}>{value => <section class="wrap page-shell article-layout">
      <PageMeta title={props.index ? 'Documentation' : value().title} canonical={value().canonical} description={value().description} />
      <nav class="docs-nav" aria-label="Documentation"><span class="eyebrow">REFERENCE</span><For each={nav}>{([label, href]) => <CampaignLink href={href === 'getting-started' ? '/docs' : '/docs/' + href}>{label}</CampaignLink>}</For><CampaignLink href="/tasks">Task guides</CampaignLink><CampaignLink href="/vs">Comparisons</CampaignLink></nav>
      {/* Only trusted repository Markdown, rendered with inline HTML disabled. */}
      <div class="min-w-0"><Show when={props.index && interactive() && value().searchIndex}>{entries => <DocSearch entries={entries()} />}</Show><article ref={body} class="prose" data-interactive={interactive() ? 'true' : undefined} onClick={copyCode} innerHTML={html()} /><span class="sr-only" role="status">{feedback()}</span></div>
    </section>}</Match>
  </Switch>;
}
