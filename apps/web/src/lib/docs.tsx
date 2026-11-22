import { A, createAsync, query } from '@solidjs/router';
import { HttpStatusCode } from '@solidjs/start';
import { For, Show } from 'solid-js';
import { PageMeta } from '@/components/page-meta';

const nav = [['Getting started','getting-started'],['CLI','cli'],['MCP tools','tools'],['Formats','formats'],['Cite-check','cite-check'],['Video timeline','video-timeline'],['Benchmarks','benchmarks'],['Migration','migration']] as const;

export const loadDoc = query(async (slug: string, legacyDocs = false) => {
  'use server';
  const { renderDoc } = await import('./docs-source');
  return renderDoc(slug, legacyDocs);
}, 'documentation');

function NotFound() {
  return <section class="wrap page-shell"><HttpStatusCode code={404} /><PageMeta title="Page not found" robots="noindex" /><h1>Page not found.</h1><p><A href="/docs" class="inline-link">Browse the documentation</A></p></section>;
}

export function Doc(props: { name: string; legacyDocs?: boolean; index?: boolean }) {
  const doc = createAsync(() => loadDoc(props.name, props.legacyDocs), { deferStream: true });
  return <Show when={doc() !== undefined}>
    <Show when={doc()} fallback={<NotFound />}>{article => <section class="wrap page-shell article-layout">
      <PageMeta title={props.index ? 'Documentation' : article().title} canonical={article().canonical} description={props.index ? undefined : article().description} />
      <nav class="docs-nav" aria-label="Documentation"><span class="eyebrow">REFERENCE</span><For each={nav}>{([label, href]) => <A href={href === 'getting-started' ? '/docs' : '/docs/' + href}>{label}</A>}</For><A href="/vs">Comparisons</A></nav>
      {/* Only trusted repository Markdown, rendered with inline HTML disabled. */}
      <article class="prose" innerHTML={article().html} />
    </section>}</Show>
  </Show>;
}
