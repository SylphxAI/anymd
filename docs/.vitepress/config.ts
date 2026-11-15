import { defineConfig } from 'vitepress';
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

function buildJsonLd(relativePath: string, pageDesc: string) {
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
    const ogTitle = isHome ? pageTitle : `${pageTitle} | anymd`;
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
