<script setup lang="ts">
import { withBase } from 'vitepress';
import { useSidebar } from 'vitepress/theme';

const { hasSidebar } = useSidebar();
</script>

<template>
  <footer class="site-footer" :class="{ 'has-sidebar': hasSidebar }">
    <div class="site-footer-inner">
      <nav class="site-footer-links" aria-label="Legal and contact">
        <a :href="withBase('/pro')">anymd Pro</a>
        <a :href="withBase('/legal/terms')">Terms</a>
        <a :href="withBase('/legal/pro-terms')">anymd Pro terms</a>
        <a :href="withBase('/legal/privacy')">Privacy</a>
        <a href="mailto:hi@sylphx.com">hi@sylphx.com</a>
        <a href="https://github.com/SylphxAI/anymd/security/policy">Security</a>
        <a href="https://sylphx.com">Sylphx</a>
      </nav>
      <p>anymd is free and open source under the MIT licence. Runs on your machine, no API key.</p>
      <p>
        © 2024–2026 Sylphx Limited. Registered in England and Wales, company number 16438428.
        Registered office: 128 City Road, London EC1V 2NX. Phone:
        <a href="tel:+443333357935">+44 333 335 7935</a>.
      </p>
    </div>
  </footer>
</template>
