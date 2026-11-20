import type { Metadata } from 'next';
import DocumentStudio from '@/components/studio/document-studio';

export const metadata: Metadata = {title:'Document Studio', description:'Convert a PDF or document to Markdown in your browser. Files stay on your device.', alternates:{canonical:'/studio'}};
export default function Studio() {
  return <section className="wrap page-shell"><header className="page-heading"><span className="eyebrow">WORKBENCH / LOCAL</span><h1>Document Studio.</h1><p>Convert documents to Markdown, find a quote in extracted text, or mark a video timestamp. Files stay on this device.</p></header><DocumentStudio /></section>;
}
