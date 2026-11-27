import { For } from 'solid-js';
import { canonicalOrigin } from '@/lib/app-origin';
import { CampaignLink } from '@/lib/attribution-link';
import { type Currency, defaultCurrency, proAmount } from '@/lib/catalog';

type AnswerPart = string | { text: string; href: '/billing' | '/legal/pro-terms' | 'mailto:hi@sylphx.com' };
export type Question = { question: string; answer: string | readonly AnswerPart[] };

function offer(name: string, price: number, path: string, currency: Currency) {
  return { '@type': 'Offer', name, price, priceCurrency: currency, url: canonicalOrigin + path };
}

export const freeOffer = (currency: Currency = defaultCurrency) => offer('Free OSS', 0, '/docs', currency);
export const proOffer = (currency: Currency = defaultCurrency, path = '/pricing') => offer('Anymd Pro', proAmount(currency),
  currency === defaultCurrency ? path : path + '?currency=' + currency, currency);

const organization = { '@type': 'Organization', name: 'Sylphx Limited', url: 'https://sylphx.com' };

export function softwareSchema(...offers: ReturnType<typeof offer>[]) {
  return {
    '@type': 'SoftwareApplication',
    name: 'Anymd',
    alternateName: 'Anymd by Sylphx',
    url: canonicalOrigin + '/',
    applicationCategory: 'DeveloperApplication',
    operatingSystem: 'macOS, Linux, Windows',
    description: 'Convert PDF and Office documents to Markdown locally with the Anymd CLI and MCP server.',
    publisher: organization,
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
      acceptedAnswer: { '@type': 'Answer', text: typeof answer === 'string' ? answer : answer.map(part => typeof part === 'string' ? part : part.text).join('') },
    })),
  };
}

export function StructuredData(props: { nodes: object[] }) {
  const json = () => JSON.stringify({ '@context': 'https://schema.org', '@graph': props.nodes }).replace(/</g, '\\u003c');
  return <script type="application/ld+json" innerHTML={json()} />;
}

export function Faq(props: { questions: readonly Question[] }) {
  return <dl class="faq-list"><For each={props.questions}>{({ question, answer }) => <div>
    <dt>{question}</dt><dd>{typeof answer === 'string' ? answer : <For each={answer}>{part =>
      typeof part === 'string' ? part : part.href === 'mailto:hi@sylphx.com'
        ? <a class="inline-link" href={part.href} rel="external">{part.text}</a>
        : <CampaignLink class="inline-link" href={part.href}>{part.text}</CampaignLink>
    }</For>}</dd>
  </div>}</For></dl>;
}
