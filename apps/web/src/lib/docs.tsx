import { A, createAsync, query, useLocation } from '@solidjs/router';
import { HttpStatusCode } from '@solidjs/start';
import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { useCookieConsent } from '@/components/cookie-consent';
import { PageMeta } from '@/components/page-meta';
import { carryCampaign } from './campaign';

const nav = [['Getting started','getting-started'],['CLI','cli'],['Python API','python'],['MCP tools','tools'],['Formats','formats'],['Cite-check','cite-check'],['Video timeline','video-timeline'],['Benchmarks','benchmarks'],['Migration','migration']] as const;

export const loadDoc = query(async (slug: string, legacyDocs = false, campaignSearch = '') => {
  'use server';
  const { renderDoc } = await import('./docs-source');
  return renderDoc(slug, legacyDocs, campaignSearch);
}, 'documentation');

function NotFound() {
  return <section class="wrap page-shell"><HttpStatusCode code={404} /><PageMeta title="Page not found" robots="noindex" /><h1>Page not found.</h1><p><A href="/docs" class="inline-link">Browse the documentation</A></p></section>;
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
      <ul><For each={matches()}>{(entry) => <li><A href={entry.path} class="inline-link" rel="external">{entry.title}</A>{entry.description && <p class="muted">{entry.description}</p>}</li>}</For></ul>
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
  // Like CampaignLink, put allowed values in the actual SSR href. Filter
  // before passing the search into the query key, not just when making links.
  const campaignSearch = () => consent.carryCampaign() ? carryCampaign('/', route.search).slice(1) : '';
  const doc = createAsync(() => loadDoc(props.name, props.legacyDocs, campaignSearch()), { deferStream: true });
  return <Show when={doc() !== undefined}>
    <Show when={doc()} fallback={<NotFound />}>{article => <section class="wrap page-shell article-layout">
      <PageMeta title={props.index ? 'Documentation' : article().title} canonical={article().canonical} description={article().description} />
      <nav class="docs-nav" aria-label="Documentation"><span class="eyebrow">REFERENCE</span><For each={nav}>{([label, href]) => <A href={href === 'getting-started' ? '/docs' : '/docs/' + href}>{label}</A>}</For><A href="/tasks">Task guides</A><A href="/vs">Comparisons</A></nav>
      {/* Only trusted repository Markdown, rendered with inline HTML disabled. */}
      <div class="min-w-0"><Show when={props.index && interactive() && article().searchIndex}>{entries => <DocSearch entries={entries()} />}</Show><article class="prose" data-interactive={interactive() ? 'true' : undefined} onClick={copyCode} innerHTML={article().html} /><span class="sr-only" role="status">{feedback()}</span></div>
    </section>}</Show>
  </Show>;
}
