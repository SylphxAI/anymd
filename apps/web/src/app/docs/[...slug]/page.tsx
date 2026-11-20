import { Doc, canonicalDocPath, documents, getDoc } from '@/lib/docs';

type Props = { params: Promise<{ slug: string[] }> };
export const dynamicParams = false;
export function generateStaticParams() {
  return documents.map((slug) => ({ slug: slug.replace(/^guide\//, '').split('/') })).concat(
    documents.filter((slug) => slug.startsWith('guide/')).map((slug) => ({ slug:slug.split('/') }))
  );
}
const key = (slug: string[]) => slug.length === 1 && !documents.includes(slug.join('/')) ? 'guide/' + slug[0] : slug.join('/');
export async function generateMetadata({ params }: Props) {
  const { slug } = await params;
  const doc = getDoc(key(slug));
  return doc ? { title: doc.title, description: doc.description, alternates: { canonical: canonicalDocPath(doc.slug) } } : {};
}
export default async function DocsPage({ params }: Props) { return <Doc name={key((await params).slug)} />; }
