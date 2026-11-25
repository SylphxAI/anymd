import { Check, Copy } from 'lucide-solid';
import { createSignal, onCleanup } from 'solid-js';
import type { CopyDetails } from '@/lib/analytics';
import { useCookieConsent } from './cookie-consent';

export function CopyButton(props: { text: string; label?: string; compact?: boolean; copied?: string; copyFailed?: string; analytics?: CopyDetails }) {
  const consent = useCookieConsent();
  const [result, setResult] = createSignal<string>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let attempt = 0;
  onCleanup(() => { attempt++; clearTimeout(timer); });
  return <button type="button" class="button copy-button" onClick={async () => {
    const run = ++attempt;
    clearTimeout(timer);
    let message: string;
    try { await navigator.clipboard.writeText(props.text); message = 'Copied'; }
    catch { message = 'Select and copy the text'; }
    if (run !== attempt) return;
    if (message === 'Copied' && props.analytics) consent.trackBehavioral('output_copied', props.analytics);
    setResult(message);
    timer = setTimeout(() => setResult(undefined), 1800);
  }}>{result() === 'Copied' ? <Check size={15} aria-hidden="true" /> : <Copy size={15} aria-hidden="true" />}<span aria-live="polite" classList={{ 'sr-only': props.compact && (!result() || result() === 'Copied') }}>{result() === 'Copied' ? props.copied ?? result() : result() ? props.copyFailed ?? result() : props.label ?? 'Copy'}</span></button>;
}
