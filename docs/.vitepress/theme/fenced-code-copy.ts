// Temporary adapter for VitePress 1.6.4. Remove when its copy handler awaits
// Clipboard API rejection and checks execCommand's result:
// https://github.com/vuejs/vitepress/pull/5556
const FEEDBACK_MS = 2000;

async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // A denied API write may still allow a user-initiated legacy copy.
  }
  const field = document.createElement('textarea');
  const focused = document.activeElement;
  const selection = window.getSelection();
  const ranges = Array.from({ length: selection?.rangeCount ?? 0 }, (_, i) =>
    selection!.getRangeAt(i).cloneRange(),
  );
  field.value = text;
  field.readOnly = true;
  field.style.cssText =
    'position:fixed;inset-inline-start:-9999px;font-size:12pt;contain:strict';
  try {
    document.body.appendChild(field);
    field.select();
    field.setSelectionRange(0, text.length);
    return document.execCommand('copy') === true;
  } catch {
    return false;
  } finally {
    field.remove();
    if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
    selection?.removeAllRanges();
    for (const range of ranges) selection?.addRange(range);
  }
}

/** Capture before VitePress's bubbling listener can report an unchecked copy. */
export function installFencedCodeCopy(): () => void {
  const timers = new Map<HTMLButtonElement, ReturnType<typeof setTimeout>>();
  const pending = new Set<HTMLButtonElement>();
  const statuses = new Map<HTMLButtonElement, HTMLElement>();
  let disposed = false;
  const clear = (button: HTMLButtonElement) => {
    clearTimeout(timers.get(button));
    timers.delete(button);
    button.classList.remove('copied');
    statuses.get(button)?.remove();
    statuses.delete(button);
  };
  const onClick = (event: MouseEvent) => {
    const button =
      event.target instanceof Element
        ? event.target.closest<HTMLButtonElement>(
            'div[class*="language-"] > button.copy',
          )
        : null;
    const block = button?.parentElement;
    const code = block?.querySelector<HTMLElement>('pre > code');
    if (!button || !block || !code) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (pending.has(button)) return;
    clear(button);
    const clone = code.cloneNode(true) as HTMLElement;
    clone
      .querySelectorAll('.vp-copy-ignore, .diff.remove')
      .forEach((node) => node.remove());
    clone.innerHTML = clone.innerHTML.replace(/\n+/g, '\n');
    let text = clone.textContent ?? '';
    const language =
      /language-(\w+)/.exec(block.className)?.[1]?.toLowerCase() ?? '';
    if (['shellscript', 'shell', 'bash', 'sh', 'zsh'].includes(language)) {
      text = text.replace(/^ *(\$|>) /gm, '').trim();
    }
    pending.add(button);
    button.setAttribute('aria-disabled', 'true');
    button.setAttribute('aria-busy', 'true');
    // Insert the live region before changing its text so readers announce it.
    const status = document.createElement('span');
    status.className = 'fenced-copy-status';
    status.setAttribute('role', 'status');
    status.setAttribute('aria-live', 'polite');
    block.appendChild(status);
    statuses.set(button, status);
    void copyText(text)
      .then((ok) => {
        if (disposed || !button.isConnected) return;
        if (ok) {
          button.classList.add('copied');
          status.textContent = 'Copied';
          status.classList.add('fenced-copy-status-success');
        } else {
          window.getSelection()?.selectAllChildren(code);
          status.textContent = window.matchMedia('(pointer: coarse)').matches
            ? 'Copy blocked: press and hold the highlighted text to copy.'
            : 'Copy blocked. Press ⌘C / Ctrl+C.';
        }
        timers.set(
          button,
          setTimeout(() => clear(button), FEEDBACK_MS),
        );
      })
      .finally(() => {
        pending.delete(button);
        button.removeAttribute('aria-disabled');
        button.removeAttribute('aria-busy');
      });
  };
  window.addEventListener('click', onClick, true);
  return () => {
    disposed = true;
    window.removeEventListener('click', onClick, true);
    for (const button of statuses.keys()) clear(button);
    for (const button of pending) {
      button.removeAttribute('aria-disabled');
      button.removeAttribute('aria-busy');
    }
  };
}
