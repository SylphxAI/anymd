import { MetaProvider } from '@solidjs/meta';
import { Router, useLocation } from '@solidjs/router';
import { FileRoutes } from '@solidjs/start/router';
import { Suspense, type ParentProps } from 'solid-js';
import { Footer, Header } from '@/components/chrome';
import { CookieConsent } from '@/components/cookie-consent';
import { localeFromPath } from '@/lib/marketing-locale';
import './globals.css';

function Shell(props: ParentProps) {
  const route = useLocation();
  const locale = () => localeFromPath(route.pathname);
  return <MetaProvider><CookieConsent>
    <div class="marketing-shell" lang={locale()}>
      <a class="skip-link" href="#main">{locale() === 'zh-TW' ? '跳到主要內容' : 'Skip to content'}</a>
      <Header />
      <main id="main"><Suspense>{props.children}</Suspense></main>
      <Footer />
    </div>
  </CookieConsent></MetaProvider>;
}

export default function App() {
  return <Router explicitLinks root={Shell}><FileRoutes /></Router>;
}
