<script setup lang="ts">
import { nextTick, ref } from 'vue';
import { useClipboard } from '../composables/useClipboard';

const cmd = 'npx -y @sylphx/anymd setup';
const { state, copy: write } = useClipboard();
const manual = ref<HTMLTextAreaElement | null>(null);

async function copy() {
  await write(cmd);
  if (state.value === 'failed') {
    await nextTick();
    manual.value?.focus();
    manual.value?.select();
  }
}
</script>

<template>
  <div class="hero-install">
    <div class="hero-install-cmd">
      <code><span class="hero-install-prompt" aria-hidden="true">$</span> {{ cmd }}</code>
      <button type="button" :disabled="state === 'copying'" @click="copy">{{ state === 'copied' ? 'Copied' : state === 'copying' ? 'Copying…' : state === 'failed' ? 'Retry copy' : 'Copy' }}</button>
    </div>
    <p class="hero-copy-status" role="status">{{ state === 'copied' ? 'Install command copied.' : state === 'failed' ? 'Copy failed. Copy the selected command manually, or retry.' : '' }}</p>
    <textarea v-if="state === 'failed'" ref="manual" class="hero-copy-manual" readonly :value="cmd" aria-label="Install command to copy manually" rows="2" @focus="manual?.select()" />
    <p class="hero-install-note">
      Adds anymd to Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf and Gemini CLI. Needs Node 18+.
    </p>
    <p class="hero-proof">1,000+ GitHub stars · 10,000+ npm downloads a month · MIT licensed</p>
  </div>
</template>

<style scoped>
.hero-copy-status:empty { display: none; }
.hero-copy-status { margin: 10px 0; font-size: 14px; }
.hero-copy-manual { display: block; width: 100%; padding: 10px; border: 1px solid var(--vp-c-brand-1); border-radius: 6px; background: var(--vp-c-bg-soft); font: 13px var(--vp-font-family-mono); resize: vertical; }
</style>
