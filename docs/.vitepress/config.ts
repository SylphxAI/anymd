import { readFileSync } from 'node:fs';
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
              text: 'Run npx -y @sylphx/anymd setup to configure detected MCP clients automatically, or install via cargo install anymd, brew install sylphxai/tap/anymd, Scoop, or Docker.',
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
};

/** A page's visible FAQ as plain text: each bold question line under `## FAQ` and the answer line after it. */
export function visibleFaq(markdown: string): Array<{ question: string; answer: string }> {
  const section = markdown.split('\n## FAQ\n')[1]?.split('\n## ')[0] ?? '';
  const plain = (text: string) => text.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1').replace(/`/g, '');
  const lines = section.split('\n');
  return lines.flatMap((line, i) => {
    const question = line.match(/^\*\*(.+)\*\*$/)?.[1];
    return question ? [{ question, answer: plain(lines[i + 1] ?? '') }] : [];
  });
}

/** A comparison page (`vs/<tool>.md`): the free and Pro offers, and its visible FAQ word for word. */
function comparisonGraph(relativePath: string, pageDesc: string) {
  const markdown = readFileSync(new URL(`../${relativePath}`, import.meta.url), 'utf8');
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
        url: site + relativePath.replace(/\.md$/, ''),
        downloadUrl: 'https://www.npmjs.com/package/@sylphx/anymd',
        softwareVersion: pkg.version,
        description: pageDesc,
        offers: [
          { '@type': 'Offer', name: 'anymd (MIT, local CLI & MCP)', price: '0', priceCurrency: 'USD' },
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
        mainEntity: visibleFaq(markdown).map(({ question, answer }) => ({
          '@type': 'Question',
          name: question,
          acceptedAnswer: { '@type': 'Answer', text: answer },
        })),
      },
    ],
  };
}

function buildJsonLd(relativePath: string, pageDesc: string) {
  const pageGraph = pageGraphs[relativePath];
  if (pageGraph) {
    return pageGraph;
  }
  if (/^vs\/[^/]+\.md$/.test(relativePath)) {
    return comparisonGraph(relativePath, pageDesc);
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
      {
        text: 'Benchmarks',
        activeMatch: '^/(guide/benchmarks|vs/)',
        items: [
          { text: 'AgentDocBench results', link: '/guide/benchmarks' },
          {
            text: 'Compare',
            items: [
              { text: 'anymd vs Docling', link: '/vs/docling' },
              { text: 'anymd vs Marker', link: '/vs/marker' },
              { text: 'anymd vs MarkItDown', link: '/vs/markitdown' },
              { text: 'anymd vs Unstructured', link: '/vs/unstructured' },
            ],
          },
        ],
      },
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
        items: [
          { text: 'anymd vs Docling', link: '/vs/docling' },
          { text: 'anymd vs Marker', link: '/vs/marker' },
          { text: 'anymd vs MarkItDown', link: '/vs/markitdown' },
          { text: 'anymd vs Unstructured', link: '/vs/unstructured' },
        ],
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
