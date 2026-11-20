'use client';

import { Check, Copy } from 'lucide-react';
import { useState } from 'react';

export function CopyButton({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [result, setResult] = useState(label);
  return <button type="button" className="button copy-button" onClick={async () => {
    try { await navigator.clipboard.writeText(text); setResult('Copied'); }
    catch { setResult('Select and copy the text'); }
    window.setTimeout(() => setResult(label), 1800);
  }}>{result === 'Copied' ? <Check size={15} /> : <Copy size={15} />}<span aria-live="polite">{result}</span></button>;
}
