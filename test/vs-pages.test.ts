import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import config from '../docs/.vitepress/config';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const site = 'https://sylphxai.github.io/anymd/';

type Graph = { '@graph': Array<Record<string, unknown>> };
type Faq = { name: string; acceptedAnswer: { text: string } };

type HeadTag = [string, Record<string, string>, string?];

/** The head tags the site config adds to a page. */
function head(relativePath: string): HeadTag[] {
  const pageData = { relativePath, frontmatter: {} as Record<string, unknown>, title: '' };
  // biome-ignore lint/suspicious/noExplicitAny: VitePress page data is wider than this test needs.
  config.transformPageData?.(pageData as any, {} as any);
  return pageData.frontmatter.head as HeadTag[];
}

function jsonLd(relativePath: string): Graph {
  const script = head(relativePath).find(
    ([tag, attrs]) => tag === 'script' && attrs.type === 'application/ld+json'
  );
  return JSON.parse(script?.[2] ?? '{}') as Graph;
}

/** The visible FAQ as plain text: each bold question followed by its answer line. */
function visibleFaq(markdown: string): Array<{ question: string; answer: string }> {
  const section = markdown.split('\n## FAQ\n')[1]?.split('\n## ')[0] ?? '';
  const plain = (s: string) => s.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1').replace(/`/g, '');
  const out: Array<{ question: string; answer: string }> = [];
  const lines = section.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const q = lines[i]?.match(/^\*\*(.+)\*\*$/);
    if (q?.[1]) out.push({ question: q[1], answer: plain(lines[i + 1] ?? '') });
  }
  return out;
}

describe('anymd vs MarkItDown page', () => {
  const page = read('../docs/vs/markitdown.md');

  test('is linked from the sidebar, the benchmarks page and llms.txt', () => {
    const groups = (config.themeConfig?.sidebar ?? []) as Array<{
      items: Array<{ link: string }>;
    }>;
    expect(groups.flatMap((g) => g.items.map((i) => i.link))).toContain('/vs/markitdown');
    expect(read('../docs/guide/benchmarks.md')).toContain('(/vs/markitdown)');
    expect(read('../docs/public/llms.txt')).toContain(`(${site}vs/markitdown)`);
  });

  test('carries generated benchmark figures, not hand-edited ones', () => {
    expect(page).toContain('<!-- versus:start -->');
    expect(page).toContain('<!-- versus:end -->');
    expect(page).toMatch(/^description: ".{120,160}"$/m);
  });

  test('names its own canonical URL', () => {
    expect(head('vs/markitdown.md')).toContainEqual([
      'link',
      { rel: 'canonical', href: `${site}vs/markitdown` },
    ]);
  });

  test('FAQPage JSON-LD repeats the visible FAQ word for word', () => {
    const faq = jsonLd('vs/markitdown.md')['@graph'].find((n) => n['@type'] === 'FAQPage');
    const structured = ((faq?.mainEntity ?? []) as Faq[]).map((q) => ({
      question: q.name,
      answer: q.acceptedAnswer.text,
    }));
    expect(structured.length).toBeGreaterThan(0);
    expect(structured).toEqual(visibleFaq(page));
  });

  test('public copy names no internal systems', () => {
    for (const copy of [page, read('../docs/public/llms.txt')]) {
      expect(copy).not.toMatch(/Keel|Spiron|AnyAgents|Sylphx Money/);
    }
  });
});
