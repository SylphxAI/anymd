import { defineConfig } from 'vitepress';

/**
 * anymd — documentation site.
 *
 * Local-first by construction: every asset here is served from this package.
 * No external fonts, scripts, trackers, or images.
 */
export default defineConfig({
  base: '/anymd/',
  cleanUrls: true,
  title: 'anymd',
  description:
    'Any file → clean Markdown for AI agents. PDF, Office, EPUB, HTML, images, audio/video. Fast Rust MCP server + CLI. Local, no API key.',

  // anymd's identity is ink + citrus; dark is the designed default and the
  // toggle still lets readers choose light.
  appearance: 'dark',
  lastUpdated: true,

  // Internal records kept for repository scripts and history; not part of the site.
  srcExclude: [
    '**/adr/**',
    '**/specs/**',
    '**/api/**',
    '**/operations/**',
    '**/reference/**',
    '**/security/**',
    '**/performance/**',
    'BRAND_PUBLISH.md',
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
    ['meta', { property: 'og:title', content: 'anymd — any file → clean Markdown for AI agents' }],
    [
      'meta',
      {
        property: 'og:description',
        content:
          'Any file → clean Markdown for AI agents. PDF, Office, EPUB, HTML, images, audio/video. Fast Rust MCP server + CLI. Local, no API key.',
      },
    ],
    ['meta', { property: 'og:url', content: 'https://sylphxai.github.io/anymd/' }],
    ['meta', { property: 'og:site_name', content: 'anymd' }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:title', content: 'anymd — any file → clean Markdown for AI agents' }],
    [
      'meta',
      {
        name: 'twitter:description',
        content:
          'Any file → clean Markdown for AI agents. PDF, Office, EPUB, HTML, images, audio/video. Fast Rust MCP server + CLI. Local, no API key.',
      },
    ],
    ['meta', { name: 'twitter:site', content: '@sylphxai' }],
    ['meta', { property: 'og:image', content: 'https://sylphxai.github.io/anymd/og-image.png' }],
    ['meta', { name: 'twitter:image', content: 'https://sylphxai.github.io/anymd/og-image.png' }],
    [
      'meta',
      {
        name: 'keywords',
        content:
          'anymd, markdown, mcp, model context protocol, pdf to markdown, docx to markdown, pptx, xlsx, epub, html to markdown, ocr, rag, ai agents, llm, claude, cursor, codex, rust, cli, markitdown alternative',
      },
    ],
    ['meta', { name: 'author', content: 'Sylphx' }],
    ['meta', { name: 'robots', content: 'index, follow' }],
    ['link', { rel: 'canonical', href: 'https://sylphxai.github.io/anymd/' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/anymd/logo.svg' }],
  ],

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
