import { Check, Copy } from 'lucide-solid';
import { createSignal, onCleanup } from 'solid-js';

export function CopyButton(props: { text: string; label?: string }) {
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
    setResult(message);
    timer = setTimeout(() => setResult(undefined), 1800);
  }}>{result() === 'Copied' ? <Check size={15} /> : <Copy size={15} />}<span aria-live="polite">{result() ?? props.label ?? 'Copy'}</span></button>;
}
