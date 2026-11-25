import assert from 'node:assert/strict';
import { test } from 'node:test';
import { showsArticleBridge } from './docs-bridge';

test('the bridge appears on indexable docs, comparisons, task guides, and use cases', () => {
  for (const canonical of ['/docs', '/docs/cli', '/vs', '/vs/docling', '/tasks/check-pdf-citation', '/use-cases/pdf-and-office-to-markdown-mcp']) {
    assert.equal(showsArticleBridge({ canonical, noindex: false }), true, canonical);
    assert.equal(showsArticleBridge({ canonical, noindex: true }), false, canonical);
  }
});

test('the bridge excludes legal, security, and unrelated pages even when indexable', () => {
  for (const canonical of ['/legal/terms', '/security/advisories', '/privacy', '/vision', '/tasks', '/studio', '/docs-extra/cli']) {
    assert.equal(showsArticleBridge({ canonical, noindex: false }), false, canonical);
  }
});
