import { Link, Meta, Title } from '@solidjs/meta';
import { useLocation } from '@solidjs/router';
import { For } from 'solid-js';
import { canonicalOrigin } from '@/lib/app-origin';
import { localeFromPath, pageLinks } from '@/lib/marketing-locale';

const defaultTitle = 'Anymd by Sylphx — local PDF & Office to Markdown CLI and MCP';
const defaultDescription = 'Anymd by Sylphx converts PDF, Office documents, web pages and audio to clean Markdown on your machine. Free, MIT-licensed CLI and MCP server for AI agents.';

export function PageMeta(props: { title?: string; canonical?: string; description?: string; robots?: string; referrer?: string }) {
  const route = useLocation();
  const zh = () => localeFromPath(route.pathname) === 'zh-TW';
  const links = () => props.canonical ? pageLinks(props.canonical, localeFromPath(route.pathname)) : undefined;
  const title = () => props.title ? props.title + ' · Anymd' : zh() ? 'Anymd by Sylphx — 在本機把 PDF、Office 轉成 Markdown 的 CLI 與 MCP' : defaultTitle;
  const description = () => props.description ?? (zh()
    ? 'Anymd 由 Sylphx 開發，在你的電腦上把 PDF、Office 文件、網頁和音訊轉成易讀的 Markdown。免費、MIT 授權的 CLI 與 MCP 伺服器，可整合到你的 AI 工具。'
    : defaultDescription);
  return <>
    <Title>{title()}</Title>
    <Meta name="description" content={description()} />
    <Meta name="twitter:card" content="summary_large_image" />
    <Meta property="og:type" content="website" />
    <Meta property="og:title" content={title()} />
    <Meta property="og:description" content={description()} />
    <Meta property="og:image" content={canonicalOrigin + '/og-image.png'} />
    {links() && <><Link rel="canonical" href={links()!.canonical} /><Meta property="og:url" content={links()!.canonical} /></>}
    {!props.robots?.includes('noindex') && <For each={links()?.alternates}>{({ lang, href }) => <Link rel="alternate" hreflang={lang} href={href} />}</For>}
    {props.robots && <Meta name="robots" content={props.robots} />}
    {props.referrer && <Meta name="referrer" content={props.referrer} />}
  </>;
}
