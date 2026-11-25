import assert from 'node:assert/strict';
import { test } from 'node:test';
import { campaignDocHtml } from './docs-campaign';

const base = '/pricing?entry_surface=task_check_pdf_citation&amp;currency=EUR#plans';
const content = '<h1>Guide</h1><a href="' + base + '" title="A &gt; B" data-doc-href="' + base + '" class="button">Plans</a>';
const other = '<code>&lt;a href="/studio"&gt;Studio&lt;/a&gt;</code><a href="/examples/a.pdf">PDF</a><a href="https://example.invalid/x">External</a>';

test('decorates only marked repository links and serializes query separators once', () => {
  const decorated = '/pricing?entry_surface=task_check_pdf_citation&amp;currency=EUR&amp;gclid=ONE&amp;utm_campaign=R%26D#plans';
  assert.equal(campaignDocHtml(content + other, '?gclid=ONE&utm_campaign=R%26D&email=private'),
    content.replace(base, decorated) + other);
  assert.equal(campaignDocHtml(content + other, ''), content + other);
});
