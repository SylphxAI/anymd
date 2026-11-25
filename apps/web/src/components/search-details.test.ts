import assert from 'node:assert/strict';
import { test } from 'node:test';
// @ts-expect-error The Bun test loader is not part of the web app's Node types.
import { mock } from 'bun:test';

// Bun uses the root JSX default; these tests exercise schema nodes, not JSX.
mock.module('react/jsx-dev-runtime', () => ({ jsxDEV() { throw new Error('Render Solid JSX with Vite'); } }));
const { collectionPageSchema, techArticleSchema } = await import('./search-details');

test('CollectionPage identifies the task index and orders canonically linked guides', () => {
  assert.deepEqual(collectionPageSchema('/tasks', 'Task guides', [
    { href: '/tasks/check-pdf-citation', title: 'Check a PDF citation' },
    { href: '/tasks/build-video-timeline', title: 'Build a video timeline' },
    { href: '/tasks/extract-video-frame', title: 'Extract a video frame' },
  ]), {
    '@type': 'CollectionPage',
    name: 'Task guides',
    url: 'https://anymd.sylphx.com/tasks',
    mainEntity: {
      '@type': 'ItemList',
      itemListElement: [
        { '@type': 'ListItem', position: 1, name: 'Check a PDF citation', url: 'https://anymd.sylphx.com/tasks/check-pdf-citation' },
        { '@type': 'ListItem', position: 2, name: 'Build a video timeline', url: 'https://anymd.sylphx.com/tasks/build-video-timeline' },
        { '@type': 'ListItem', position: 3, name: 'Extract a video frame', url: 'https://anymd.sylphx.com/tasks/extract-video-frame' },
      ],
    },
  });
});

test('TechArticle identifies the canonical page, organization, and software', () => {
  assert.deepEqual(techArticleSchema('/docs/benchmarks', 'Benchmarks', 'Document conversion benchmarks.'), {
    '@type': 'TechArticle',
    headline: 'Benchmarks',
    url: 'https://anymd.sylphx.com/docs/benchmarks',
    mainEntityOfPage: { '@type': 'WebPage', '@id': 'https://anymd.sylphx.com/docs/benchmarks' },
    description: 'Document conversion benchmarks.',
    inLanguage: 'en',
    author: { '@type': 'Organization', name: 'Sylphx Limited', url: 'https://sylphx.com' },
    publisher: { '@type': 'Organization', name: 'Sylphx Limited', url: 'https://sylphx.com' },
    about: { '@type': 'SoftwareApplication', name: 'Anymd', url: 'https://anymd.sylphx.com/' },
  });
});

test('TechArticle omits an absent or blank description', () => {
  assert.equal(Object.hasOwn(techArticleSchema('/docs', 'Documentation'), 'description'), false);
  for (const description of ['', '   ']) {
    assert.equal(Object.hasOwn(techArticleSchema('/docs', 'Documentation', description), 'description'), false);
  }
});
