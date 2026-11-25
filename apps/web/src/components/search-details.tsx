import { For } from 'solid-js';
import { canonicalOrigin } from '@/lib/app-origin';
import { type Currency, defaultCurrency, proAmount } from '@/lib/catalog';

export type Question = { question: string; answer: string };

function offer(name: string, price: number, path: string, currency: Currency) {
  return { '@type': 'Offer', name, price, priceCurrency: currency, url: canonicalOrigin + path };
}

export const freeOffer = (currency: Currency = defaultCurrency) => offer('Free OSS', 0, '/docs', currency);
export const proOffer = (currency: Currency = defaultCurrency, path = '/pricing') => offer('Anymd Pro', proAmount(currency),
  currency === defaultCurrency ? path : path + '?currency=' + currency, currency);

export function softwareSchema(...offers: ReturnType<typeof offer>[]) {
  return {
    '@type': 'SoftwareApplication',
    name: 'Anymd',
    url: canonicalOrigin + '/',
    applicationCategory: 'DeveloperApplication',
    operatingSystem: 'macOS, Linux, Windows',
    description: 'Convert documents to Markdown locally with the Anymd CLI and MCP server.',
    offers,
  };
}

export function collectionPageSchema(path: string, name: string, entries: readonly { href: string; title: string }[]) {
  return {
    '@type': 'CollectionPage',
    name,
    url: canonicalOrigin + path,
    mainEntity: {
      '@type': 'ItemList',
      itemListElement: entries.map(({ href, title }, index) => ({
        '@type': 'ListItem',
        position: index + 1,
        name: title,
        url: canonicalOrigin + href,
      })),
    },
  };
}

export function techArticleSchema(path: string, headline: string, description?: string) {
  const url = canonicalOrigin + path;
  const organization = { '@type': 'Organization', name: 'Sylphx Limited', url: 'https://sylphx.com' };
  return {
    '@type': 'TechArticle',
    headline,
    url,
    mainEntityOfPage: { '@type': 'WebPage', '@id': url },
    ...(description?.trim() ? { description } : {}),
    inLanguage: 'en',
    author: organization,
    publisher: organization,
    about: { '@type': 'SoftwareApplication', name: 'Anymd', url: canonicalOrigin + '/' },
  };
}

export function studioSchema(path: string, locale: string, title: string, description: string) {
  return {
    '@type': 'WebApplication',
    name: 'Anymd ' + title,
    url: canonicalOrigin + path,
    inLanguage: locale,
    applicationCategory: 'UtilitiesApplication',
    browserRequirements: 'Requires a browser with WebAssembly support.',
    description,
    offers: offer('Free Studio', 0, path, defaultCurrency),
  };
}

export function faqSchema(path: string, questions: readonly Question[]) {
  return {
    '@type': 'FAQPage',
    '@id': canonicalOrigin + path + '#faq-title',
    mainEntity: questions.map(({ question, answer }) => ({
      '@type': 'Question',
      name: question,
      acceptedAnswer: { '@type': 'Answer', text: answer },
    })),
  };
}

export function StructuredData(props: { nodes: object[] }) {
  const json = () => JSON.stringify({ '@context': 'https://schema.org', '@graph': props.nodes }).replace(/</g, '\\u003c');
  return <script type="application/ld+json" innerHTML={json()} />;
}

export function Faq(props: { questions: readonly Question[] }) {
  return <dl class="faq-list"><For each={props.questions}>{({ question, answer }) => <div>
    <dt>{question}</dt><dd>{answer}</dd>
  </div>}</For></dl>;
}
