import { defineConfig } from 'vitepress';
import product from '../../product.json';

/**
 * anymd — documentation site.
 *
 * Local-first by construction: every asset here is served from this package.
 * No external fonts, scripts, trackers, or images.
 */
const site = 'https://sylphxai.github.io/anymd/';

export default defineConfig({
  base: '/anymd/',
  cleanUrls: true,
  title: 'anymd',
  description: product.description,

  // anymd's identity is ink + citrus; dark is the designed default and the
  // toggle still lets readers choose light.
  appearance: 'dark',
  lastUpdated: true,
  sitemap: { hostname: site },

  // Internal records kept for repository scripts and history; not part of the site.
  srcExclude: [
    '**/adr/**',
    '**/specs/**',
    '**/api/**',
    '**/operations/**',
    '**/reference/**',
    '**/security/**',
    '**/performance/**',
    'PUBLISH.md',
  ],
  vite: {
    build: {
      target: 'esnext',
    },
  },

  head: [
    ['meta', { name: 'theme-color', content: '#c3f53c' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'anymd' }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:site', content: '@sylphxai' }],
    ['meta', { property: 'og:image', content: 'https://sylphxai.github.io/anymd/og-image.png' }],
    ['meta', { name: 'twitter:image', content: 'https://sylphxai.github.io/anymd/og-image.png' }],
    [
      'meta',
      {
        name: 'keywords',
        content: [product.name, ...product.keywords].join(', '),
      },
    ],
    ['meta', { name: 'author', content: 'Sylphx' }],
    ['meta', { name: 'robots', content: 'index, follow' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/anymd/logo.svg' }],
  ],

  // Each page names its own URL, so search engines index every page, not just the home page.
  transformPageData(pageData) {
    const pageUrl =
      site + pageData.relativePath.replace(/(^|\/)index\.md$/, '$1').replace(/\.md$/, '');
    const pageTitle =
      pageData.frontmatter.title ?? (pageData.title || `${product.name} — ${product.tagline}`);
    const pageDesc = pageData.frontmatter.description ?? product.description;
    pageData.frontmatter.head ??= [];
    pageData.frontmatter.head.push(
      ['link', { rel: 'canonical', href: pageUrl }],
      ['meta', { property: 'og:url', content: pageUrl }],
      ['meta', { property: 'og:title', content: pageTitle }],
      ['meta', { property: 'og:description', content: pageDesc }],
    );
  },

  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'anymd',

    nav: [
      { text: 'Guide', link: '/guide/getting-started', activeMatch: '^/guide/(?!benchmarks)' },
      { text: 'Benchmarks', link: '/guide/benchmarks' },
      { text: 'Playground', link: '/playground' },
      { text: 'GitHub', link: 'https://github.com/SylphxAI/anymd' },
      { text: 'npm', link: 'https://www.npmjs.com/package/@sylphx/anymd' },
    ],

    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'Getting started', link: '/guide/getting-started' },
          { text: 'MCP tools', link: '/guide/tools' },
          { text: 'CLI', link: '/guide/cli' },
          { text: 'Formats', link: '/guide/formats' },
          { text: 'Benchmarks', link: '/guide/benchmarks' },
          { text: 'Migration', link: '/guide/migration' },
          { text: 'Security', link: '/guide/security' },
        ],
      },
      {
        text: 'Try it',
        items: [{ text: 'Playground', link: '/playground' }],
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/SylphxAI/anymd' },
      { icon: 'npm', link: 'https://www.npmjs.com/package/@sylphx/anymd' },
    ],

    editLink: {
      pattern: 'https://github.com/SylphxAI/anymd/edit/main/docs/:path',
      text: 'Edit this page on GitHub',
    },

    footer: {
      message: 'MIT licensed · local, no API key',
      copyright: 'Copyright 2024–2026 Sylphx',
    },

    outline: { level: [2, 3] },

    search: {
      provider: 'local',
      options: {
        detailedView: true,
      },
    },
  },
});
