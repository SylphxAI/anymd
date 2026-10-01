import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import { h } from 'vue';
import HeroInstall from './components/HeroInstall.vue';
import Playground from './components/Playground.vue';
import ProBuy from './components/ProBuy.vue';
import ProTracking from './components/ProTracking.vue';
import SiteFooter from './components/SiteFooter.vue';
import './custom.css';

/**
 * anymd theme — inherits VitePress and layers the citrus design system in
 * custom.css. Deliberately dependency-free and offline: the docs site loads no
 * external fonts, scripts, or images, matching the product's local-first rule.
 */
const theme: Theme = {
  extends: DefaultTheme,
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      'home-hero-actions-after': () => h(HeroInstall),
      'layout-bottom': () => h(SiteFooter),
    }),
  enhanceApp({ app }) {
    app.component('Playground', Playground);
    app.component('ProBuy', ProBuy);
    app.component('ProTracking', ProTracking);
  },
};

export default theme;
