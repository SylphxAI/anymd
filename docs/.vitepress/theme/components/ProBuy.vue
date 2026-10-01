<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { buyHref, buyUrl, clickId, savedChoice, trackBeginCheckout } from '../../pro/tracking';

defineProps<{ label?: string }>();
const href = ref(buyUrl);

// The Google click id rides along as Stripe client_reference_id only when the
// visitor accepted ad measurement.
onMounted(() => {
  if (savedChoice() === 'all') href.value = buyHref(clickId(location.search));
  window.addEventListener('anymd-pro-consent', () => {
    href.value = savedChoice() === 'all' ? buyHref(clickId(location.search)) : buyUrl;
  });
});
</script>

<template>
  <a class="pro-buy" :href="href" rel="noopener" @click="trackBeginCheckout">{{ label ?? 'Buy anymd Pro, US$29 once' }}</a>
</template>
