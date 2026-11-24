import { CampaignLink } from '@/lib/attribution-link';
import { ArrowUpRight, Braces, Globe, Menu } from 'lucide-solid';
import { GithubMark } from './github-mark';
import { useLocation } from '@solidjs/router';
import { For } from 'solid-js';
import { useCookieConsent } from './cookie-consent';
import { SessionAction } from './session';
import { isCurrency } from '@/lib/catalog';
import { languageSwitchTarget, localeFromPath } from '@/lib/marketing-locale';

const zhNav: Record<string, string> = { Studio: '轉檔工作室（英文）', Docs: '文件', Comparisons: '工具比較', Pricing: '定價' };

const links = [['Studio', '/studio'], ['Docs', '/docs'], ['Comparisons', '/vs'], ['Pricing', '/pricing']] as const;

export function Header() {
  const route = useLocation();
  const locale = () => localeFromPath(route.pathname);
  const zh = () => locale() === 'zh-TW';
  const other = () => zh() ? 'en' : 'zh-TW';
  const switchCurrency = () => {
    const currencies = new URLSearchParams(route.search).getAll('currency');
    return currencies.length === 1 && isCurrency(currencies[0]) ? 'currency=' + currencies[0] : '';
  };
  const pricing = () => languageSwitchTarget(route.pathname, 'en') === '/pricing';
  const navHref = (href: (typeof links)[number][1]) => zh() && href === '/pricing' ? '/zh-TW/pricing' : href;
  return (
    <header class="site-header">
      <div class="wrap header-row">
        <CampaignLink href={zh() ? '/zh-TW' : '/'} class="brand" aria-label={zh() ? 'Anymd 首頁' : 'Anymd home'}><span><Braces size={20} strokeWidth={2.4} /></span>anymd<span class="brand-dot">.</span></CampaignLink>
        <nav aria-label={zh() ? '主要導覽' : 'Main'} class="main-nav"><For each={links}>{([text, href]) => <CampaignLink href={navHref(href)}>{zh() ? zhNav[text] : text}</CampaignLink>}</For></nav>
        <div class="header-actions">
          <a href="https://github.com/SylphxAI/anymd" class="github-header" aria-label={zh() ? 'Anymd 的 GitHub 儲存庫' : 'Anymd GitHub repository'}><GithubMark size={18} /></a>
          <CampaignLink class="language-switch" href={pricing()
            ? (switchCurrency() ? (zh() ? '/pricing?' + switchCurrency() : '/zh-TW/pricing?' + switchCurrency()) : (zh() ? '/pricing' : '/zh-TW/pricing'))
            : (zh() ? '/' : '/zh-TW')} hreflang={other()} lang={other()}><Globe size={15} aria-hidden="true" />{zh() ? 'English' : '繁體中文'}</CampaignLink>
          <SessionAction />
          <details class="mobile-menu"><summary aria-label={zh() ? '開啟導覽選單' : 'Open navigation'}><Menu size={22} /></summary><nav aria-label={zh() ? '行動版導覽' : 'Mobile'}><For each={links}>{([text, href]) => <CampaignLink href={navHref(href)} onClick={(event) => {
            if (!event.defaultPrevented && event.button === 0 && !event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey) {
              event.currentTarget.closest('details')?.removeAttribute('open');
            }
          }}>{zh() ? zhNav[text] : text}</CampaignLink>}</For></nav></details>
        </div>
      </div>
    </header>
  );
}

export function Footer() {
  const cookies = useCookieConsent();
  const route = useLocation();
  const locale = () => localeFromPath(route.pathname);
  const zh = () => locale() === 'zh-TW';
  return (
    <footer class="site-footer"><div class="wrap footer-row">
      <div><CampaignLink class="brand" href={zh() ? '/zh-TW' : '/'}><span><Braces size={18} /></span>anymd<span class="brand-dot">.</span></CampaignLink><p>{zh() ? '讀取文件，輸出 Markdown' : 'Documents in. Markdown out.'}</p></div>
      <div class="footer-links"><CampaignLink href="/docs">{zh() ? '文件' : 'Docs'}</CampaignLink><CampaignLink href="/vs">{zh() ? '工具比較' : 'Comparisons'}</CampaignLink><CampaignLink href="/privacy">{zh() ? '隱私權' : 'Privacy'}</CampaignLink><button class="footer-cookie" type="button" onClick={event => cookies.open(event.currentTarget)}>{zh() ? 'Cookie 設定' : 'Cookie settings'}</button><CampaignLink href="/legal/pro-terms">{zh() ? 'Pro 條款' : 'Pro terms'}</CampaignLink><a href="https://github.com/SylphxAI/anymd">GitHub <ArrowUpRight size={13} /></a><a href="https://pypi.org/project/anymd/">PyPI <ArrowUpRight size={13} /></a></div>
    </div></footer>
  );
}
