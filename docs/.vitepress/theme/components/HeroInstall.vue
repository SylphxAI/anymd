<script setup lang="ts">
import { ref } from 'vue';

const cmd = 'npx -y @sylphx/anymd setup';
const copied = ref(false);

async function copy() {
  try {
    await navigator.clipboard.writeText(cmd);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch {
    // Clipboard blocked: the command stays selectable.
  }
}
</script>

<template>
  <div class="hero-install">
    <div class="hero-install-cmd">
      <code><span class="hero-install-prompt" aria-hidden="true">$</span> {{ cmd }}</code>
      <button type="button" @click="copy">{{ copied ? 'Copied' : 'Copy' }}</button>
    </div>
    <p class="hero-install-note">
      Adds anymd to Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf and Gemini CLI. Needs Node 18+.
    </p>
    <p class="hero-proof">1,000+ GitHub stars · 10,000+ npm downloads a month · MIT licensed</p>
  </div>
</template>
