import assert from 'node:assert/strict';
import { test } from 'node:test';
import { articleBridgeSurface, inlineProCtaSurface, showsArticleBridge } from './docs-bridge';

test('the bridge appears on indexable docs, comparisons, task guides, and use cases', () => {
  for (const canonical of ['/docs', '/docs/cli', '/vs', '/vs/docling', '/tasks/check-pdf-citation', '/use-cases/pdf-and-office-to-markdown-mcp']) {
    assert.equal(showsArticleBridge({ canonical, noindex: false }), true, canonical);
    assert.equal(showsArticleBridge({ canonical, noindex: true }), false, canonical);
  }
});

test('the bridge attributes canonical organic pages and falls back on generic or unknown docs', () => {
  for (const [canonical, surface] of [
    ['/vs', 'seo_vs-hub'], ['/vs/docling', 'seo_vs-docling'], ['/vs/unstructured', 'seo_vs-unstructured'],
    ['/use-cases/pdf-and-office-to-markdown-mcp', 'seo_pdf-and-office-to-markdown-mcp'],
    ['/tasks/check-pdf-citation', 'task_check_pdf_citation'], ['/tasks/extract-video-frame', 'task_extract_video_frame'],
    ...['/docs', '/docs/cli', '/vs/private-input', '/tasks/private-input', '/use-cases/private-input', '/vs/docling/more', '/vs/docling?proof=private'].map(path => [path, 'article_bridge']),
  ]) assert.equal(articleBridgeSurface(canonical), surface, canonical);
});

test('only a local pricing link with one allowlisted Pro CTA label supplies inline attribution', () => {
  assert.equal(inlineProCtaSurface('/pricing?entry_surface=seo_vs-docling#plans'), 'seo_vs-docling');
  assert.equal(inlineProCtaSurface('/pricing?currency=HKD&entry_surface=task_check_pdf_citation'), 'task_check_pdf_citation');
  for (const href of [null, '/pricing', '/pricing?entry_surface=cli_pdf', '/pricing?entry_surface=private-input',
    '/pricing?entry_surface=seo_vs-docling&entry_surface=seo_vs-docling', '/pricing?foo=bar#entry_surface=seo_vs-docling',
    '/studio?entry_surface=seo_vs-docling', 'https://other.invalid/pricing?entry_surface=seo_vs-docling']) {
    assert.equal(inlineProCtaSurface(href), undefined, String(href));
  }
});

test('the bridge excludes legal, security, and unrelated pages even when indexable', () => {
  for (const canonical of ['/legal/terms', '/security/advisories', '/privacy', '/vision', '/tasks', '/studio', '/docs-extra/cli']) {
    assert.equal(showsArticleBridge({ canonical, noindex: false }), false, canonical);
  }
});
