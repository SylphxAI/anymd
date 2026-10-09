<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import {
  buyHref,
  buyUrl,
  clickId,
  savedChoice,
  trackBeginCheckout,
} from '../../pro/tracking';

defineProps<{ label?: string }>();
const href = ref(buyUrl);

// The Google click id rides along as Stripe client_reference_id only when the
// visitor accepted ad measurement. buyHref leaves non-Stripe checkout links alone.
function refresh() {
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
    <a class="pro-buy" :href="href" rel="noopener" @click="trackBeginCheckout()">{{ label ?? 'Buy anymd Pro, US$29 once' }}</a>
  </span>
</template>
