import { Doc, canonicalDocPath, documents, getDoc } from '@/lib/docs';

type Props = { params: Promise<{ slug: string[] }> };
export const dynamicParams = false;
export function generateStaticParams() { return [...documents, 'vs'].map((slug) => ({ slug: slug.split('/') })); }
export async function generateMetadata({ params }: Props) {
  const slug = (await params).slug.join('/');
  const doc = getDoc(slug);
  return doc ? { title:doc.title, description:doc.description, alternates:{ canonical:canonicalDocPath(doc.slug) } } : {};
}
export default async function Article({ params }: Props) { return <Doc name={(await params).slug.join('/')} />; }
