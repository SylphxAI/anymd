import { h, onMounted, onUnmounted } from 'vue';
import { installFencedCodeCopy } from './fenced-code-copy';
import './fenced-code-copy.css';
import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import {
  appendAttributionToUrl,
  isPricingHref,
  mergedAttribution,
  persistAttribution,
} from '../pro/tracking';
import HeroInstall from './components/HeroInstall.vue';
import Playground from './components/Playground.vue';
import ProBuy from './components/ProBuy.vue';
import ProTracking from './components/ProTracking.vue';
import SiteFooter from './components/SiteFooter.vue';
import './custom.css';

function decoratePricingLinks(): void {
  const attr = persistAttribution(window.location.search);
  if (Object.keys(attr).length === 0) return;
  for (const anchor of document.querySelectorAll<HTMLAnchorElement>('a[href]')) {
    const raw = anchor.getAttribute('href');
    if (raw && isPricingHref(raw)) {
      anchor.setAttribute('href', appendAttributionToUrl(raw, attr));
    }
  }
}

function installAttributionForwarding(): () => void {
  decoratePricingLinks();
  const onClick = (event: MouseEvent) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const anchor = target.closest('a[href]');
    if (!(anchor instanceof HTMLAnchorElement)) return;
    const raw = anchor.getAttribute('href');
    if (!raw || !isPricingHref(raw)) return;
    const attr = mergedAttribution(window.location.search);
    if (Object.keys(attr).length > 0) {
      anchor.setAttribute('href', appendAttributionToUrl(raw, attr));
    }
  };
  const onPopState = () => decoratePricingLinks();
  document.addEventListener('click', onClick, true);
  window.addEventListener('popstate', onPopState);
  return () => {
    document.removeEventListener('click', onClick, true);
    window.removeEventListener('popstate', onPopState);
  };
}

/**
 * anymd theme — inherits VitePress and layers the citrus design system in
 * custom.css. Deliberately dependency-free and offline: the docs site loads no
 * external fonts, scripts, or images, matching the product's local-first rule.
 */
const theme: Theme = {
  extends: DefaultTheme,
  setup() {
    let consentObserver: ResizeObserver | undefined;
    let disposeCopy: (() => void) | undefined;
    let disposeAttribution: (() => void) | undefined;
    onMounted(() => {
      const wrapper = document.querySelector('.pro-consent-wrap');
      if (wrapper) {
        const measure = () => {
          document.documentElement.style.setProperty(
            '--vp-layout-top-height',
            String(wrapper.getBoundingClientRect().height) + 'px'
          );
        };
        consentObserver = new ResizeObserver(measure);
        consentObserver.observe(wrapper);
        measure();
      }
      disposeCopy = installFencedCodeCopy();
      disposeAttribution = installAttributionForwarding();
    });
    onUnmounted(() => {
      consentObserver?.disconnect();
      document.documentElement.style.removeProperty('--vp-layout-top-height');
      disposeCopy?.();
      disposeAttribution?.();
    });
  },
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      'home-hero-actions-after': () => h(HeroInstall),
      'layout-top': () => h(ProTracking),
      'layout-bottom': () => h(SiteFooter),
    }),
  enhanceApp({ app }) {
    app.component('Playground', Playground);
    app.component('ProBuy', ProBuy);
    app.component('ProTracking', ProTracking);
  },
};

export default theme;
