import { createMemo, For, Show } from 'solid-js';
import { Dynamic } from 'solid-js/web';
import type { Locale } from '@/lib/marketing-locale';
import { studioCopy } from '@/lib/studio-copy';
import { type Block, parseInline, parseMarkdown } from '@/lib/studio-markdown';

export type MarkdownView = 'markdown' | 'preview';

export function MarkdownViewToggle(props: { locale: Locale; view: MarkdownView; onViewChange: (view: MarkdownView) => void }) {
  const copy = () => studioCopy[props.locale];
  return <div class="studio-view" role="group" aria-label={copy().view}>
    <button type="button" aria-pressed={props.view === 'markdown'} onClick={() => props.onViewChange('markdown')}>Markdown</button>
    <button type="button" aria-pressed={props.view === 'preview'} onClick={() => props.onViewChange('preview')}>{copy().preview}</button>
  </div>;
}

function InlineText(props: { text: string }) {
  const spans = createMemo(() => parseInline(props.text));
  return <For each={spans()}>{span => {
    switch (span.kind) {
      case 'code': return <code>{span.text}</code>;
      case 'strong': return <strong>{span.text}</strong>;
      case 'text': return span.text;
    }
  }}</For>;
}

function PreviewBlock(props: { block: Block; locale: Locale }) {
  const block = props.block;
  const copy = () => studioCopy[props.locale];
  switch (block.kind) {
    case 'heading': return <Dynamic component={(['h1', 'h2', 'h3', 'h4'] as const)[block.level - 1]}><InlineText text={block.text} /></Dynamic>;
    case 'paragraph': return <p><InlineText text={block.text} /></p>;
    case 'quote': return <blockquote><InlineText text={block.text} /></blockquote>;
    case 'marker': return <p class="studio-marker">{copy().sourceLabel(block.label)}</p>;
    case 'code': return <pre tabIndex={0}><code>{block.text}</code></pre>;
    case 'list': return <Dynamic component={block.start === null ? 'ul' : 'ol'} start={block.start ?? undefined}>
      <For each={block.items}>{item => <li><InlineText text={item} /></li>}</For>
    </Dynamic>;
    case 'table': return <div class="studio-table" role="region" aria-label={copy().previewTable} tabIndex={0}>
      <table>
        <thead><tr><For each={block.header}>{(cell, col) =>
          <th scope="col" class={block.align[col()]}><InlineText text={cell} /></th>
        }</For></tr></thead>
        <tbody><For each={block.rows}>{row => <tr><For each={row}>{(cell, col) =>
          <td class={block.align[col()]}><InlineText text={cell} /></td>
        }</For></tr>}</For></tbody>
      </table>
    </div>;
  }
}

export default function MarkdownPreview(props: { markdown: string; locale: Locale }) {
  const document = createMemo(() => parseMarkdown(props.markdown));
  const copy = () => studioCopy[props.locale];
  return <div class="studio-preview" role="region" aria-label={copy().previewOutput} tabIndex={0}>
    <Show when={document().metadata.length}>
      <dl class="studio-metadata">
        <For each={document().metadata}>{field => <div><dt>{copy().metadataLabel(field.key)}</dt><dd>{field.value}</dd></div>}</For>
      </dl>
    </Show>
    <div class="studio-preview-body">
      <For each={document().blocks}>{block => <PreviewBlock block={block} locale={props.locale} />}</For>
    </div>
  </div>;
}
