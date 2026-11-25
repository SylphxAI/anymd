import { useLocation } from '@solidjs/router';
import { localeFromPath, localizedPath } from '@/lib/marketing-locale';
import { studioCopy } from '@/lib/studio-copy';
import { PageMeta } from '@/components/page-meta';
import { StructuredData, studioSchema } from '@/components/search-details';
import DocumentStudio from '@/components/studio/document-studio';

export function Studio(props: { title?: string }) {
  const route = useLocation();
  const locale = () => localeFromPath(route.pathname);
  const t = () => studioCopy[locale()];
  return <section class="wrap page-shell studio-page">
    <PageMeta title={props.title ?? t().title} canonical="/studio" description={t().description} />
    <StructuredData nodes={[studioSchema(localizedPath('/studio', locale()), locale(), t().title, t().description)]} />
    <header class="page-heading"><span class="eyebrow">{t().eyebrow}</span><h1>{t().heading}</h1><p>{t().intro}</p></header>
    <DocumentStudio locale={locale()} />
  </section>;
}
