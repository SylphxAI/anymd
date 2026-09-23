import { defineConfig } from 'vitepress';

/**
 * Citra — documentation site.
 *
 * Local-first by construction: every asset here is served from this package.
 * No external fonts, scripts, trackers, or images.
 */
export default defineConfig({
  base: '/citra/',
  cleanUrls: true,
  title: 'Citra',
  description:
    'Give your AI agent eyes for PDFs — with proof. Local-first PDF evidence for agents: structured text, tables, OCR, visual crops, and page-level citations your agent can defend.',

  // Citra's identity is ink + citrus; dark is the designed default and the
  // toggle still lets readers choose light.
  appearance: 'dark',
  lastUpdated: true,

  srcExclude: ['**/adr/**', '**/specs/**'],
  vite: {
    build: {
      target: 'esnext',
    },
  },

  head: [
    ['meta', { name: 'theme-color', content: '#c3f53c' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:title', content: 'Citra — PDF evidence for agents, with proof' }],
    [
      'meta',
      {
        property: 'og:description',
        content:
          'One read_pdf call returns structured text, tables, OCR, and page-level citations your agent can defend — local-first, native, and fail-closed.',
      },
    ],
    ['meta', { property: 'og:url', content: 'https://sylphxai.github.io/citra/' }],
    ['meta', { property: 'og:site_name', content: 'Citra' }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:title', content: 'Citra — PDF evidence for agents, with proof' }],
    [
      'meta',
      {
        name: 'twitter:description',
        content:
          'Stop PDF hallucinations. Turn PDFs into an structured document result: tables with geometry, OCR with provenance, and citations agents can show a human.',
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
          'mcp, pdf, reader, ai agent, claude, cursor, model context protocol, rust, rag, citations, pdf inspection, pdf intelligence, agent document twin, visual evidence, ocr provenance, trust report, accessibility report, layout analysis',
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
      { text: 'Guide', link: '/guide/' },
      { text: 'Reference', link: '/api/' },
      { text: 'Proof', link: '/guide/product-proof' },
      { text: 'Performance', link: '/performance/' },
    ],

    sidebar: [
      {
        text: 'Get started',
        items: [
          { text: 'Introduction', link: '/guide/' },
          { text: 'Installation', link: '/guide/installation' },
          { text: 'Quickstart', link: '/guide/getting-started' },
        ],
      },
      {
        text: 'What you get',
        collapsed: true,
        items: [
          { text: 'Vision', link: '/vision' },
          { text: 'Capabilities', link: '/capabilities' },
          { text: 'Tool surface — four tools', link: '/TOOL_SURFACE' },
          { text: 'The evidence contract', link: '/EVIDENCE_CONTRACT' },
          { text: 'Local-first frontier', link: '/LOCAL_FIRST_FRONTIER' },
          { text: 'Product proof', link: '/guide/product-proof' },
        ],
      },
      {
        text: 'Reference',
        collapsed: true,
        items: [
          { text: 'API reference', link: '/api/' },
          { text: 'Comparison', link: '/comparison/' },
          { text: 'Design philosophy', link: '/design/' },
        ],
      },
      {
        text: 'Proof & performance',
        collapsed: true,
        items: [
          { text: 'Performance', link: '/performance/' },
          { text: 'Why Rust', link: '/performance/why-rust' },
          { text: 'Benchmark proof', link: '/benchmark' },
        ],
      },
      {
        text: 'Security & operations',
        collapsed: true,
        items: [
          { text: 'Security reporting', link: '/security/maintainer-process' },
          { text: 'Remote URL policy', link: '/security/remote-url-policy' },
          { text: 'Advisory record', link: '/security/advisory-backlog' },
        ],
      },
      {
        text: 'Articles',
        collapsed: true,
        items: [
          { text: 'Stop PDF hallucinations', link: '/articles/stop-pdf-hallucinations' },
          { text: 'Evidence-first PDF reading', link: '/articles/evidence-first' },
        ],
      },
      {
        text: 'Design & roadmap',
        collapsed: true,
        items: [
          { text: 'Design philosophy', link: '/design/' },
          { text: 'SOTA family roadmap', link: '/roadmap/sota-family-roadmap' },
          { text: 'V3 PDF intelligence', link: '/weekly/2026-06-22-v3-pdf-intelligence' },
        ],
      },
      {
        text: 'Release history',
        collapsed: true,
        items: [{ text: 'Migration notes', link: '/migration' }],
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
      message: 'MIT licensed · local-first by design · no external calls from these docs',
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
