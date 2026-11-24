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
