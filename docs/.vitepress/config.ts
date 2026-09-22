import { defineConfig } from 'vitepress';

export default defineConfig({
  base: '/citra/',
  cleanUrls: true,
  title: 'Citra',
  description:
    'Give your AI agent eyes for PDFs. Structured text, tables, OCR, visual evidence, and page-level citations — local-first native engine for Claude, Cursor, VS Code, and any MCP client.',

  appearance: 'dark',
  lastUpdated: true,

  vite: {
    build: {
      target: 'esnext',
    },
  },

  head: [
    ['meta', { property: 'og:type', content: 'website' }],
    [
      'meta',
      { property: 'og:title', content: 'Citra — Give your AI agent eyes for PDFs' },
    ],
    [
      'meta',
      {
        property: 'og:description',
        content:
          'The most-starred PDF MCP server. One read_pdf call returns markdown, tables, trust signals, and source evidence with page numbers and bounding boxes. Local-first and benchmark-gated.',
      },
    ],
    ['meta', { property: 'og:url', content: 'https://sylphxai.github.io/citra/' }],
    ['meta', { property: 'og:site_name', content: 'Citra' }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:title', content: 'Citra' }],
    [
      'meta',
      {
        name: 'twitter:description',
        content:
          'Stop PDF hallucinations. Turn PDFs into Agent Document Twins with tables, trust reports, and citeable source evidence.',
      },
    ],
    ['meta', { name: 'twitter:site', content: '@sylphxai' }],
    ['meta', { property: 'og:image', content: 'https://sylphxai.github.io/citra/og-image.png' }],
    ['meta', { name: 'twitter:image', content: 'https://sylphxai.github.io/citra/og-image.png' }],
    [
      'meta',
      {
        name: 'keywords',
        content:
          'mcp, pdf, reader, ai, claude, model context protocol, rust, rag, citations, pdf inspection, pdf intelligence, agent document twin, visual evidence, ocr provenance, trust report, accessibility report, layout analysis, reading order',
      },
    ],
    ['meta', { name: 'author', content: 'Sylphx' }],
    ['meta', { name: 'robots', content: 'index, follow' }],
    ['link', { rel: 'canonical', href: 'https://sylphxai.github.io/citra/' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/logo.svg' }],
  ],

  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'Citra',

    nav: [
      { text: 'Home', link: '/' },
      { text: 'Guide', link: '/guide/' },
      { text: 'API', link: '/api/' },
      { text: 'Benchmark', link: '/benchmark' },
      { text: 'Articles', link: '/articles/stop-pdf-hallucinations' },
      { text: 'Design', link: '/design/' },
      { text: 'Performance', link: '/performance/' },
    ],

    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'Introduction', link: '/guide/' },
          { text: 'Installation', link: '/guide/installation' },
          { text: 'Getting Started', link: '/guide/getting-started' },
          { text: 'Product Proof', link: '/guide/product-proof' },
        ],
      },
      {
        text: 'Reference',
        items: [
          { text: 'API Reference', link: '/api/' },
          { text: 'Benchmark Proof', link: '/benchmark' },
          { text: 'Design Philosophy', link: '/design/' },
          { text: 'Performance', link: '/performance/' },
          { text: 'Comparison', link: '/comparison/' },
          { text: 'Security', link: '/security/maintainer-process' },
        ],
      },
      {
        text: 'Articles',
        items: [
          { text: 'Stop PDF Hallucinations', link: '/articles/stop-pdf-hallucinations' },
          { text: 'Evidence-First PDF Reading', link: '/articles/evidence-first' },
        ],
      },
      {
        text: 'Architecture',
        items: [
          {
            text: 'V3 Smart Tool Surface',
            link: '/specs/2026-06-22-v3-smart-tool-surface',
          },
          {
            text: '2027 SOTA Boundary',
            link: '/adr/0001-2027-sota-document-intelligence-boundary',
          },
          {
            text: 'Operating Model',
            link: '/specs/2026-06-16-2027-sota-document-intelligence-operating-model',
          },
        ],
      },
      {
        text: 'Updates',
        items: [
          {
            text: 'V3 PDF Intelligence',
            link: '/weekly/2026-06-22-v3-pdf-intelligence',
          },
        ],
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/SylphxAI/citra' },
      { icon: 'npm', link: 'https://www.npmjs.com/package/@sylphx/citra' },
    ],

    editLink: {
      pattern: 'https://github.com/SylphxAI/citra/edit/main/docs/:path',
      text: 'Edit this page on GitHub',
    },

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright 2024-2026 SylphxAI',
    },

    search: {
      provider: 'local',
    },
  },
});
