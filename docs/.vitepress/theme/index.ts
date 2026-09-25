import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import './custom.css';

/**
 * anymd theme — inherits VitePress and layers the citrus design system in
 * custom.css. Deliberately dependency-free and offline: the docs site loads no
 * external fonts, scripts, or images, matching the product's local-first rule.
 */
const theme: Theme = {
  extends: DefaultTheme,
};

export default theme;
