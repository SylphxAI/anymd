import MarkdownIt from 'markdown-it';
import { canonicalOrigin } from './app-origin';

const source = import.meta.glob('../../../../docs/{guide,legal,security,tasks,use-cases,vs}/*.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const prefix = '../../../../docs/';
const docs = new Map(Object.entries(source).map(([name, raw]) => [name.slice(prefix.length, -3), raw]));
export const documents = [...docs.keys(), 'vision'];
const vision = import.meta.glob('../../../../docs/vision.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
docs.set('vision', vision[prefix + 'vision.md']);

export function canonicalDocPath(slug: string): string {
  if (slug === 'guide/getting-started') return '/docs';
  if (slug === 'vs/index') return '/vs';
  if (slug === 'legal/privacy') return '/privacy';
  return slug.startsWith('guide/') ? '/docs/' + slug.slice('guide/'.length) : '/' + slug;
}

function localUrl(href: string, slug: string) {
  href = href.replace(/^https?:\/\/sylphxai\.github\.io\/anymd(?=\/|$)/, '').replace(/^\/anymd(?=\/|$)/, '');
  if (!href) return '/';
  if (href.startsWith('#') || /^[a-z]+:/i.test(href) || href.startsWith('//')) return href;
  const url = new URL(href, canonicalOrigin + '/' + slug);
  return url.pathname.replace(/\/(?:index\.md|index\.html)$/, '/').replace(/\.(?:md|html)$/, '') + url.search + url.hash;
}

type RenderContext = { slug: string; headings: Map<string, number> };

const md = new MarkdownIt({ html: false, linkify: true });
md.renderer.rules.link_open = (tokens, index, options, env, self) => {
  const { slug } = env as RenderContext;
  const link = tokens[index];
  const href = link.attrGet('href');
  if (href) link.attrSet('href', localUrl(String(href), slug));
  return self.renderToken(tokens, index, options);
};
md.renderer.rules.heading_open = (tokens, index, options, env, self) => {
  const { headings } = env as RenderContext;
  const heading = tokens[index];
  if (heading.tag === 'h2' || heading.tag === 'h3') {
    const text = tokens[index + 1]?.children?.map(token => ['code_inline', 'text'].includes(token.type) ? token.content : '').join('') ?? '';
    const base = text.toLowerCase().replace(/[^\p{L}\p{N}\s-]/gu, '').trim().replace(/\s+/g, '-');
    const count = (headings.get(base) ?? 0) + 1;
    headings.set(base, count);
    heading.attrSet('id', count === 1 ? base : base + '-' + count);
  }
  return self.renderToken(tokens, index, options);
};

export function renderDoc(input: string, legacyDocs: boolean) {
  const slug = !legacyDocs && input === 'vs' ? 'vs/index'
    : legacyDocs && !input.includes('/') && !docs.has(input) ? 'guide/' + input : input;
  const raw = docs.get(slug);
  if (!raw) return null;
  const frontmatter = /^---\r?\n([\s\S]*?)\r?\n---\r?\n/;
  const metadata = raw.match(frontmatter)?.[1];
  const markdown = raw.replace(frontmatter, '');
  const title = markdown.match(/^# (.+)$/m)?.[1] ?? slug;
  const description = metadata?.match(/^description: *(.*)$/m)?.[1]?.replace(/^['"]|['"]$/g, '');
  return { title, description, canonical: canonicalDocPath(slug), html: md.render(markdown, { slug, headings: new Map<string, number>() }) };
}
