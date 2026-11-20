import 'server-only';
import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { notFound } from 'next/navigation';
import Link from 'next/link';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

const root = resolve(process.cwd(), '../../docs');
const folders = ['guide', 'legal', 'security', 'tasks', 'use-cases', 'vs'];
export const documents = [...folders.flatMap((dir) => readdirSync(resolve(root, dir))
  .filter((name) => name.endsWith('.md')).map((name) => dir + '/' + name.slice(0,-3))), 'vision'];

const nav = [['Getting started','getting-started'],['CLI','cli'],['MCP tools','tools'],['Formats','formats'],['Cite-check','cite-check'],['Video timeline','video-timeline'],['Benchmarks','benchmarks'],['Migration','migration']];
const aliases: Record<string, string> = { vs: 'vs/index' };

export function getDoc(input: string) {
  const slug = aliases[input] ?? input;
  if (!documents.includes(slug)) return null;
  const raw = readFileSync(resolve(root, slug + '.md'), 'utf8');
  const metadata = raw.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/)?.[1];
  const markdown = raw.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, '');
  const title = markdown.match(/^# (.+)$/m)?.[1] ?? slug;
  const description = metadata?.match(/^description: *(.*)$/m)?.[1]?.replace(/^['"]|['"]$/g, '');
  return { slug, title, description, markdown };
}

function localUrl(href: string, slug: string) {
  href = href.replace(/^https?:\/\/sylphxai\.github\.io\/anymd(?=\/|$)/, '').replace(/^\/anymd(?=\/|$)/, '');
  if (!href) return '/';
  if (href.startsWith('#') || /^[a-z]+:/i.test(href) || href.startsWith('//')) return href;
  const url = new URL(href, 'https://anymd.sylphx.com/' + slug);
  return url.pathname.replace(/\/(?:index\.md|index\.html)$/, '/').replace(/\.(?:md|html)$/, '') + url.search + url.hash;
}

function headingText(children: React.ReactNode): string {
  if (typeof children === 'string') return children;
  if (Array.isArray(children)) return children.map(headingText).join('');
  if (children && typeof children === 'object' && 'props' in children) {
    const props = children.props as { children?: React.ReactNode };
    return headingText(props.children);
  }
  return '';
}

export function Doc({ name }: { name: string }) {
  const doc = getDoc(name);
  if (!doc) notFound();
  const headings = new Map<string, number>();
  function heading(children: React.ReactNode) {
    const base = headingText(children).toLowerCase().replace(/[^\p{L}\p{N}\s-]/gu,'').trim().replace(/\s+/g,'-');
    const count = (headings.get(base) ?? 0) + 1;
    headings.set(base, count);
    return count > 1 ? base + '-' + count : base;
  }
  return <section className="wrap page-shell article-layout"><nav className="docs-nav" aria-label="Documentation"><span className="eyebrow">REFERENCE</span>{nav.map(([label, href]) => <Link key={href} href={href === 'getting-started' ? '/docs' : '/docs/' + href}>{label}</Link>)}<Link href="/vs">Comparisons</Link></nav>
    <article className="prose"><Markdown remarkPlugins={[remarkGfm]} components={{
      a: ({ href, children }) => <a href={href ? localUrl(href, doc.slug) : undefined}>{children}</a>,
      h2: ({ children }) => <h2 id={heading(children)}>{children}</h2>,
      h3: ({ children }) => <h3 id={heading(children)}>{children}</h3>,
    }}>{doc.markdown}</Markdown></article>
  </section>;
}
