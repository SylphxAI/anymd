import { MetaProvider } from '@solidjs/meta';
import { Router } from '@solidjs/router';
import { FileRoutes } from '@solidjs/start/router';
import { Suspense } from 'solid-js';
import { Footer, Header } from '@/components/chrome';
import { CookieConsent } from '@/components/cookie-consent';
import './globals.css';

export default function App() {
  return <Router explicitLinks root={props => <MetaProvider><CookieConsent>
    <a class="skip-link" href="#main">Skip to content</a>
    <Header />
    <main id="main"><Suspense>{props.children}</Suspense></main>
    <Footer />
  </CookieConsent></MetaProvider>}><FileRoutes /></Router>;
}
