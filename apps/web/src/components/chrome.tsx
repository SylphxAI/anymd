import { ArrowUpRight, Braces, Menu } from 'lucide-solid';
import { GithubMark } from './github-mark';
import { A } from '@solidjs/router';
import { For } from 'solid-js';
import { useCookieConsent } from './cookie-consent';
import { SessionAction } from './session';

const links = [['Studio', '/studio'], ['Docs', '/docs'], ['Comparisons', '/vs'], ['Pricing', '/pricing']] as const;

export function Header() {
  return (
    <header class="site-header">
      <div class="wrap header-row">
        <A href="/" class="brand" aria-label="Anymd home"><span><Braces size={20} strokeWidth={2.4} /></span>anymd<span class="brand-dot">.</span></A>
        <nav aria-label="Main" class="main-nav"><For each={links}>{([text, href]) => <A href={href}>{text}</A>}</For></nav>
        <div class="header-actions">
          <a href="https://github.com/SylphxAI/anymd" class="github-header" aria-label="Anymd GitHub repository"><GithubMark size={18} /></a>
          <SessionAction />
          <details class="mobile-menu"><summary aria-label="Open navigation"><Menu size={22} /></summary><nav aria-label="Mobile"><For each={links}>{([text, href]) => <A href={href} onClick={(event) => {
            if (!event.defaultPrevented && event.button === 0 && !event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey) {
              event.currentTarget.closest('details')?.removeAttribute('open');
            }
          }}>{text}</A>}</For></nav></details>
        </div>
      </div>
    </header>
  );
}

export function Footer() {
  const cookies = useCookieConsent();
  return (
    <footer class="site-footer"><div class="wrap footer-row">
      <div><A class="brand" href="/"><span><Braces size={18} /></span>anymd<span class="brand-dot">.</span></A><p>Documents in. Markdown out.</p></div>
      <div class="footer-links"><A href="/docs">Docs</A><A href="/vs">Comparisons</A><A href="/privacy">Privacy</A><button class="footer-cookie" type="button" onClick={event => cookies.open(event.currentTarget)}>Cookie settings</button><A href="/legal/pro-terms">Pro terms</A><a href="https://github.com/SylphxAI/anymd">GitHub <ArrowUpRight size={13} /></a><a href="https://pypi.org/project/anymd/">PyPI <ArrowUpRight size={13} /></a></div>
    </div></footer>
  );
}
