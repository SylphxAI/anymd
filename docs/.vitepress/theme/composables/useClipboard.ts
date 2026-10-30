import { onBeforeUnmount, ref } from 'vue';

export function useClipboard() {
  const state = ref<'idle' | 'copying' | 'copied' | 'failed'>('idle');
  let timer: ReturnType<typeof setTimeout> | undefined;
  let attempt = 0;

  function reset() {
    attempt++;
    clearTimeout(timer);
    state.value = 'idle';
  }

  async function copy(text: string) {
    if (state.value === 'copying') return false;
    reset();
    const current = attempt;
    state.value = 'copying';
    try {
      await navigator.clipboard.writeText(text);
      if (current !== attempt) return false;
      state.value = 'copied';
      timer = setTimeout(() => (state.value = 'idle'), 1500);
      return true;
    } catch {
      if (current === attempt) state.value = 'failed';
      return false;
    }
  }

  onBeforeUnmount(reset);
  return { state, copy, reset };
}
