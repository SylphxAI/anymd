<script setup lang="ts">
import { onMounted, ref } from 'vue';
import {
  type Choice,
  loadTags,
  savedChoice,
  trackPurchase,
  trackingEnabled,
  updateConsent,
} from '../../pro/tracking';

const props = defineProps<{ page: 'pro' | 'thanks' }>();
const enabled = trackingEnabled();
const open = ref(false);
const current = ref<Choice | null>(null);
const options: { choice: Choice; label: string }[] = [
  { choice: 'all', label: 'Accept all' },
  { choice: 'analytics', label: 'Analytics only' },
  { choice: 'none', label: 'Reject' },
];

function choose(choice: Choice) {
  current.value = choice;
  updateConsent(choice);
  window.dispatchEvent(new Event('anymd-pro-consent'));
  open.value = false;
}

onMounted(() => {
  if (!loadTags(props.page)) return;
  current.value = savedChoice();
  open.value = !current.value;
  if (props.page === 'thanks') trackPurchase(location.search);
});
</script>

<template>
  <div v-if="enabled" class="pro-consent-wrap">
    <p class="pro-cookie-link"><a href="#cookie-settings" @click.prevent="open = true">Cookie settings</a></p>
    <div v-if="open" class="pro-consent" role="region" aria-label="Cookie consent">
      <span>
        This page uses Google Analytics and Google Ads measurement to see how people find us and which
        ads work. Depending on your choice, Google may set cookies and process identifiers such as your
        IP address and the ad click id. We never send your name or email. You can change this at any time
        from Cookie settings.
      </span>
      <span class="pro-consent-actions">
        <button
          v-for="o in options"
          :key="o.choice"
          type="button"
          :aria-pressed="current === o.choice ? 'true' : 'false'"
          @click="choose(o.choice)"
        >{{ o.label }}</button>
      </span>
    </div>
  </div>
</template>
