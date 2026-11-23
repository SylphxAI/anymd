import { createSignal, onMount } from 'solid-js';

export function useBrowserLocales() {
  const [locales, setLocales] = createSignal<readonly string[]>();
  onMount(() => setLocales(navigator.languages.length ? navigator.languages : [navigator.language]));
  return locales;
}
