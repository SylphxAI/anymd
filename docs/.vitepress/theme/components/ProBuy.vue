<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import {
  buyHref,
  buyMailto,
  buyReady,
  buyUrl,
  clickId,
  savedChoice,
  trackBeginCheckout,
} from '../../pro/tracking';

defineProps<{ label?: string }>();
const href = ref(buyReady ? buyUrl : buyMailto);

// The Google click id rides along as Stripe client_reference_id only when the
// visitor accepted ad measurement. buyHref leaves non-Stripe links (mailto:) alone.
function refresh() {
  if (!buyReady) return;
  href.value = savedChoice() === 'all' ? buyHref(clickId(location.search)) : buyUrl;
}

onMounted(() => {
  refresh();
  window.addEventListener('anymd-pro-consent', refresh);
});
onUnmounted(() => window.removeEventListener('anymd-pro-consent', refresh));
</script>

<template>
  <span class="pro-buy-wrap">
    <a class="pro-buy" :href="href" rel="noopener" @click="buyReady && trackBeginCheckout()">{{ label ?? 'Buy anymd Pro, US$29 once' }}</a>
    <span v-if="!buyReady" class="pro-buy-note">Purchase is by email: we reply with a secure Stripe payment link, and your licence token follows by email, usually within a few hours.</span>
  </span>
</template>
