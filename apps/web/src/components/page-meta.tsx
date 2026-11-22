import { Link, Meta, Title } from '@solidjs/meta';
import { canonicalOrigin } from '@/lib/app-origin';

const defaultTitle = 'Anymd — Documents in. Markdown out.';
const defaultDescription = 'Convert PDFs, Office documents, web pages and audio to clean Markdown. Run Anymd locally as a CLI or connect it to your agent over MCP.';

export function PageMeta(props: { title?: string; canonical?: string; description?: string; robots?: string; referrer?: string }) {
  const title = () => props.title ? props.title + ' · Anymd' : defaultTitle;
  const description = () => props.description ?? defaultDescription;
  return <>
    <Title>{title()}</Title>
    <Meta name="description" content={description()} />
    <Meta name="twitter:card" content="summary_large_image" />
    <Meta property="og:type" content="website" />
    <Meta property="og:title" content={title()} />
    <Meta property="og:description" content={description()} />
    <Meta property="og:image" content={canonicalOrigin + '/og-image.png'} />
    {props.canonical && <><Link rel="canonical" href={canonicalOrigin + props.canonical} /><Meta property="og:url" content={canonicalOrigin + props.canonical} /></>}
    {props.robots && <Meta name="robots" content={props.robots} />}
    {props.referrer && <Meta name="referrer" content={props.referrer} />}
  </>;
}
