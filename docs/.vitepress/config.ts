import { defineConfig } from 'vitepress';
import pkg from '../../packages/anymd/package.json';
import product from '../../product.json';

/**
 * anymd — documentation site.
 *
 * Local-first by construction: every asset here is served from this package.
 * No external fonts, scripts, trackers, or images.
 */
const site = 'https://sylphxai.github.io/anymd/';

const organizationJsonLd = {
  '@type': 'Organization',
  name: 'Sylphx',
  legalName: 'Sylphx Limited',
  url: 'https://sylphx.com',
  sameAs: ['https://github.com/SylphxAI', 'https://sylphx.com'],
};

const websiteJsonLd = {
  '@type': 'WebSite',
  name: 'anymd',
  url: site,
  description: product.description,
};

const publisherOrg = {
  '@type': 'Organization',
  name: 'Sylphx Limited',
  url: 'https://sylphx.com',
  email: 'hi@sylphx.com',
};

const pageGraphs: Record<string, object> = {
  'use-cases/pdf-and-office-to-markdown-mcp.md': {
    '@context': 'https://schema.org',
    '@graph': [
      organizationJsonLd,
      websiteJsonLd,
      {
        '@type': 'SoftwareApplication',
        name: 'anymd',
        applicationCategory: 'DeveloperApplication',
        operatingSystem: 'macOS, Linux, Windows',
        url: `${site}use-cases/pdf-and-office-to-markdown-mcp`,
        downloadUrl: 'https://www.npmjs.com/package/@sylphx/anymd',
        softwareVersion: pkg.version,
        description:
          'Fast local Rust MCP server and CLI converting PDF, Word, PowerPoint, Excel, EPUB, HTML, OCR images, and audio/video into clean, token-lean Markdown for AI agents.',
        offers: [
          {
            '@type': 'Offer',
            name: 'anymd Core (MIT, local CLI & MCP)',
            price: '0',
            priceCurrency: 'USD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (One-time 100-year offline licence)',
            price: '29',
            priceCurrency: 'USD',
          },
        ],
        publisher: publisherOrg,
      },
      {
        '@type': 'FAQPage',
        mainEntity: [
          {
            '@type': 'Question',
            name: 'What file formats does anymd support?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'anymd converts PDF, Word (.docx), PowerPoint (.pptx), Excel and spreadsheets (.xlsx, .xls, .ods, CSV/TSV), EPUB, HTML and URLs, images with OCR, and audio/video metadata, chapters, subtitles, and transcripts.',
            },
          },
          {
            '@type': 'Question',
            name: 'Does anymd require an API key or upload local files?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'No. Local anymd runs offline on your machine under the MIT licence with no API key or account required. Network calls happen only when you pass an explicit http(s) URL.',
            },
          },
          {
            '@type': 'Question',
            name: 'How does anymd compare on AgentDocBench?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'Across 38 documents in 12 categories on AgentDocBench, anymd scores 96.4 overall (38/38 converted, 92.2 Table cells F1) in 11.5 seconds, compared to Docling at 93.0 in 2,432.4 seconds and MarkItDown at 76.8 in 75.6 seconds.',
            },
          },
          {
            '@type': 'Question',
            name: 'How do I install anymd into Claude Code, Cursor, or Codex?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'Run npx -y @sylphx/anymd setup to configure detected MCP clients automatically, or install via cargo install anymd, brew install sylphxai/tap/anymd, winget, or Docker.',
            },
          },
        ],
      },
    ],
  },
  'use-cases/verifiable-video-and-cite-check-ai-agents.md': {
    '@context': 'https://schema.org',
    '@graph': [
      organizationJsonLd,
      websiteJsonLd,
      {
        '@type': 'SoftwareApplication',
        name: 'anymd Pro',
        applicationCategory: 'DeveloperApplication',
        operatingSystem: 'macOS, Linux, Windows',
        url: `${site}use-cases/verifiable-video-and-cite-check-ai-agents`,
        softwareVersion: pkg.version,
        isAccessibleForFree: false,
        description:
          'Find the page behind a PDF quote and the frame behind a video citation. Works on your machine, with a one-time Pro licence.',
        offers: [
          {
            '@type': 'Offer',
            name: 'anymd Pro (USD, one-time 100-year licence)',
            price: '29',
            priceCurrency: 'USD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (HKD, one-time 100-year licence)',
            price: '229',
            priceCurrency: 'HKD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (TWD, one-time 100-year licence)',
            price: '959',
            priceCurrency: 'TWD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (GBP, one-time 100-year licence)',
            price: '21.99',
            priceCurrency: 'GBP',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (EUR, one-time 100-year licence)',
            price: '26.99',
            priceCurrency: 'EUR',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (JPY, one-time 100-year licence)',
            price: '4639',
            priceCurrency: 'JPY',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (KRW, one-time 100-year licence)',
            price: '38900',
            priceCurrency: 'KRW',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (MYR, one-time 100-year licence)',
            price: '118.99',
            priceCurrency: 'MYR',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (SGD, one-time 100-year licence)',
            price: '37.99',
            priceCurrency: 'SGD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (CAD, one-time 100-year licence)',
            price: '41.99',
            priceCurrency: 'CAD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (AUD, one-time 100-year licence)',
            price: '42.99',
            priceCurrency: 'AUD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (NZD, one-time 100-year licence)',
            price: '52.99',
            priceCurrency: 'NZD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (THB, one-time 100-year licence)',
            price: '989',
            priceCurrency: 'THB',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (PHP, one-time 100-year licence)',
            price: '1859',
            priceCurrency: 'PHP',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (IDR, one-time 100-year licence)',
            price: '529000',
            priceCurrency: 'IDR',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (VND, one-time 100-year licence)',
            price: '749000',
            priceCurrency: 'VND',
          },
        ],
        publisher: publisherOrg,
      },
      {
        '@type': 'FAQPage',
        mainEntity: [
          {
            '@type': 'Question',
            name: 'What does a citation match tell me?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'It tells you that the quoted text was found on the specified page and area. It does not establish that the statement is true. Missing OCR or text positions can leave a citation unconfirmed.',
            },
          },
          {
            '@type': 'Question',
            name: 'Do I need an internet connection after purchase?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'Pro works offline. Activate your licence once on your machine; licence checks stay there.',
            },
          },
          {
            '@type': 'Question',
            name: 'Is there a subscription?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'No. You make one payment for a single-person, 100-year licence with future releases included. The current price, currency and applicable tax are shown before you buy.',
            },
          },
          {
            '@type': 'Question',
            name: 'Can I buy Pro from the terminal?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'On anymd 8.4.0 or later, run anymd pro buy. It opens checkout in your browser and saves the issued licence on your machine. You can also buy on the web and activate the licence yourself.',
            },
          },
        ],
      },
    ],
  },
  'vs/markitdown.md': {
    '@context': 'https://schema.org',
    '@graph': [
      organizationJsonLd,
      websiteJsonLd,
      {
        '@type': 'SoftwareApplication',
        name: 'anymd',
        applicationCategory: 'DeveloperApplication',
        operatingSystem: 'macOS, Linux, Windows',
        url: `${site}vs/markitdown`,
        downloadUrl: 'https://www.npmjs.com/package/@sylphx/anymd',
        softwareVersion: pkg.version,
        description:
          'A native Rust CLI and MCP server that turns documents into Markdown for AI agents. On AgentDocBench it scores 96.4 to MarkItDown 76.8 on the same 38 documents.',
        offers: [
          {
            '@type': 'Offer',
            name: 'anymd (MIT, local CLI & MCP)',
            price: '0',
            priceCurrency: 'USD',
          },
          {
            '@type': 'Offer',
            name: 'anymd Pro (One-time 100-year offline licence)',
            price: '29',
            priceCurrency: 'USD',
          },
        ],
        publisher: publisherOrg,
      },
      {
        '@type': 'FAQPage',
        mainEntity: [
          {
            '@type': 'Question',
            name: 'Is anymd a drop-in replacement for MarkItDown?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: "On the command line, mostly: anymd report.pdf -o report.md works like markitdown report.pdf -o report.md, and npx -y @sylphx/anymd setup adds anymd to your MCP clients. anymd's Python package is not on PyPI yet, so code that imports markitdown cannot switch today.",
            },
          },
          {
            '@type': 'Question',
            name: 'Is anymd faster than MarkItDown?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'On AgentDocBench, anymd converted all 38 documents in 11.5 seconds and MarkItDown in 75.6 seconds, each timed as a fresh process per file with start-up included.',
            },
          },
          {
            '@type': 'Question',
            name: 'Does anymd need Python?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'No. anymd is a native Rust binary. The npx install needs Node 18 or newer; Homebrew, winget and Docker install the same binary without Node.',
            },
          },
          {
            '@type': 'Question',
            name: 'Can MarkItDown read scanned PDFs?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'Not with its default options: on AgentDocBench its CLI returned no text for three image-only PDFs. It offers an OCR plugin that calls an LLM, and Azure Document Intelligence. anymd reads scans locally with tesseract, or with a local document model after anymd setup ocr, and scores 77.9 on the scanned category.',
            },
          },
          {
            '@type': 'Question',
            name: 'Is the benchmark biased toward anymd?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: "It lives in the anymd repository, so you can check it: the scoring does not know which tool produced an output, the corpus and ground truth are public, six documents come from MarkItDown's own test files, and the categories where anymd loses are published. Anyone can rerun it or add a tool.",
            },
          },
          {
            '@type': 'Question',
            name: 'Is anymd free?',
            acceptedAnswer: {
              '@type': 'Answer',
              text: 'Yes. anymd is MIT licensed, including the CLI, the MCP server, OCR and transcripts. anymd Pro is an optional one-time purchase, US$29 or the local price in 16 currencies, that adds PDF cite-check and video timelines and frames.',
            },
          },
        ],
      },
    ],
  },
};

function buildJsonLd(relativePath: string, pageDesc: string) {
  const pageGraph = pageGraphs[relativePath];
  if (pageGraph) {
    return pageGraph;
  }
  const isProPage = relativePath === 'pro.md' || relativePath === 'pro/thanks.md';
  if (isProPage) {
    return {
      '@context': 'https://schema.org',
      '@graph': [
        organizationJsonLd,
        websiteJsonLd,
        {
          '@type': 'SoftwareApplication',
          name: 'anymd Pro',
          applicationCategory: 'DeveloperApplication',
          operatingSystem: 'macOS, Linux, Windows',
          url: `${site}pricing`,
          isAccessibleForFree: false,
          description: pageDesc,
        },
      ],
    };
  }
  return {
    '@context': 'https://schema.org',
    '@graph': [
      organizationJsonLd,
      websiteJsonLd,
      {
        '@type': 'SoftwareApplication',
        name: 'anymd',
        applicationCategory: 'DeveloperApplication',
        operatingSystem: 'macOS, Linux, Windows',
        url: site,
        downloadUrl: 'https://www.npmjs.com/package/@sylphx/anymd',
        description: product.description,
        offers: {
          '@type': 'Offer',
          price: '0',
          priceCurrency: 'USD',
        },
      },
    ],
  };
}

export default defineConfig({
  base: '/anymd/',
  cleanUrls: true,
  // Keel emits /pricing and /recover after VitePress has checked the docs.
  ignoreDeadLinks: [/^\/recover$/, /^\/pricing(?:$|[?#])/],
  title: 'anymd',
  description: product.description,

  // anymd's identity is ink + citrus; dark is the designed default and the
  // toggle still lets readers choose light.
  appearance: 'dark',
  lastUpdated: true,
  sitemap: {
    hostname: site,
    // /pro/thanks is post-purchase; /pro canonicalizes to /pricing (emitted by Keel).
    transformItems: (items) => {
      const filtered = items.filter(
        (item) => !item.url.includes('pro/thanks') && !/(?:^|\/)pro(?:\/|$)/.test(item.url)
      );
      if (!filtered.some((item) => item.url === 'pricing' || item.url === 'pricing.html')) {
        filtered.push({ url: 'pricing' });
      }
      return filtered;
    },
  },

  // Maintainer docs that are not part of the site.
  srcExclude: ['PUBLISH.md', 'vision.md', 'security/**', 'storefront/**'],
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
    ['meta', { property: 'og:image:alt', content: 'anymd: any file to clean Markdown for AI agents' }],
    ['meta', { property: 'og:image:width', content: '1280' }],
    ['meta', { property: 'og:image:height', content: '640' }],
    [
      'meta',
      {
        name: 'keywords',
        content: [product.name, ...product.keywords].join(', '),
      },
    ],
    ['meta', { name: 'author', content: 'Sylphx' }],
    ['meta', { name: 'robots', content: 'index, follow' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/anymd/favicon.svg' }],
    ['link', { rel: 'icon', href: '/anymd/favicon.ico', sizes: '16x16 32x32 48x48' }],
  ],

  // Each page names its own URL, so search engines index every page, not just the home page.
  transformPageData(pageData) {
    const rawPageUrl =
      site + pageData.relativePath.replace(/(^|\/)index\.md$/, '$1').replace(/\.md$/, '');
    const pageUrl = pageData.relativePath === 'pro.md' ? `${site}pricing` : rawPageUrl;
    const pageTitle =
      pageData.frontmatter.title ?? (pageData.title || `${product.name} — ${product.tagline}`);
    const pageDesc = pageData.frontmatter.description ?? product.description;
    const isHome = pageData.relativePath === 'index.md';
    const hasBrandSuffix = / \| anymd(?: Pro)?$/.test(pageTitle);
    const ogTitle = isHome || hasBrandSuffix ? pageTitle : `${pageTitle} | anymd`;
    const image = pageData.frontmatter.image ?? `${site}og-image.png`;
    pageData.frontmatter.head ??= [];
    pageData.frontmatter.head.push(
      ['link', { rel: 'canonical', href: pageUrl }],
      ['meta', { property: 'og:url', content: pageUrl }],
      ['meta', { property: 'og:title', content: ogTitle }],
      ['meta', { name: 'twitter:title', content: ogTitle }],
      ['meta', { property: 'og:image', content: image }],
      ['meta', { name: 'twitter:image', content: image }],
      ['meta', { property: 'og:description', content: pageDesc }],
      ['script', { type: 'application/ld+json' }, JSON.stringify(buildJsonLd(pageData.relativePath, pageDesc))]
    );
  },

  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'anymd',

    nav: [
      { text: 'Guide', link: '/guide/getting-started', activeMatch: '^/guide/(?!benchmarks)' },
      {
        text: 'Use Cases',
        link: '/use-cases/pdf-and-office-to-markdown-mcp',
        activeMatch: '^/use-cases/',
      },
      { text: 'Benchmarks', link: '/guide/benchmarks' },
      { text: 'Playground', link: '/playground' },
      { text: 'Pro', link: '/pricing' },
      { text: 'GitHub', link: 'https://github.com/SylphxAI/anymd' },
      { text: 'npm', link: 'https://www.npmjs.com/package/@sylphx/anymd' },
    ],

    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'Getting started', link: '/guide/getting-started' },
          { text: 'Local and hosted', link: '/guide/hosted' },
          { text: 'MCP tools', link: '/guide/tools' },
          { text: 'Cite-check', link: '/guide/cite-check' },
          { text: 'CLI', link: '/guide/cli' },
          { text: 'Python and loaders', link: '/guide/python' },
          { text: 'Video timelines', link: '/guide/video-timeline' },
          { text: 'Formats', link: '/guide/formats' },
          { text: 'Benchmarks', link: '/guide/benchmarks' },
          { text: 'Migration', link: '/guide/migration' },
          { text: 'Security', link: '/guide/security' },
        ],
      },
      {
        text: 'Tasks',
        items: [
          { text: 'Extract a video frame', link: '/tasks/extract-video-frame' },
          { text: 'Build a video timeline', link: '/tasks/build-video-timeline' },
          { text: 'Check a PDF citation', link: '/tasks/check-pdf-citation' },
        ],
      },
      {
        text: 'Use Cases',
        items: [
          {
            text: 'PDF & Office to Markdown MCP',
            link: '/use-cases/pdf-and-office-to-markdown-mcp',
          },
          {
            text: 'Cite-check & Video Evidence',
            link: '/use-cases/verifiable-video-and-cite-check-ai-agents',
          },
        ],
      },
      {
        text: 'Compare',
        items: [{ text: 'anymd vs MarkItDown', link: '/vs/markitdown' }],
      },
      {
        text: 'Try it',
        items: [{ text: 'Playground', link: '/playground' }],
      },
      {
        text: 'Pro',
        items: [{ text: 'anymd Pro', link: '/pricing' }],
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

    outline: { level: [2, 3] },

    search: {
      provider: 'local',
      options: {
        detailedView: true,
      },
    },
  },
});
