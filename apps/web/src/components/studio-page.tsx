import { PageMeta } from '@/components/page-meta';
import DocumentStudio from '@/components/studio/document-studio';

export function Studio(props: { title?: string }) {
  return <section class="wrap page-shell studio-page"><PageMeta title={props.title ?? 'Document Studio'} canonical="/studio" description="Convert a PDF or document to Markdown in your browser. Files stay on your device." /><header class="page-heading"><span class="eyebrow">WORKBENCH / LOCAL</span><h1>Document Studio.</h1><p>Convert documents to Markdown, find a quote in extracted text, or mark a video timestamp. Files stay on this device.</p></header><DocumentStudio /></section>;
}
