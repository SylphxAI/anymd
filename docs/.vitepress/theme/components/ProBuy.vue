<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import {
  buyHref,
  buyUrl,
  persistAttribution,
  savedChoice,
  trackBeginCheckout,
} from '../../pro/tracking';

defineProps<{ label?: string }>();
const href = ref(buyUrl);

// Forward inbound click IDs and UTMs to /pricing; only direct Stripe payment
// links gate client_reference_id on ad-measurement consent.
function refresh() {
  const attr = persistAttribution(location.search);
  if (/^https:\/\/buy\.stripe\.com\//.test(buyUrl)) {
    href.value = savedChoice() === 'all' ? buyHref(attr) : buyUrl;
  } else {
    href.value = buyHref(attr);
  }
}

onMounted(() => {
  refresh();
  window.addEventListener('anymd-pro-consent', refresh);
});
onUnmounted(() => window.removeEventListener('anymd-pro-consent', refresh));
</script>

<template>
  <span class="pro-buy-wrap">
    <a class="pro-buy" :href="href" target="_self" rel="noopener" @click="trackBeginCheckout()">{{ label ?? 'Buy anymd Pro' }}</a>
  </span>
</template>
