<script setup lang="ts">
import { useRoute, withBase } from 'vitepress';
import { onMounted, ref, watch } from 'vue';
import {
  type Choice,
  type TrackingPage,
  hasAnalyticsConsent,
  loadTags,
  purgeAnalyticsCookies,
  savedChoice,
  trackPageView,
  trackPurchase,
  trackingEnabled,
  updateConsent,
} from '../../pro/tracking';

const props = defineProps<{ page?: TrackingPage }>();
const route = useRoute();
const enabled = trackingEnabled();
const open = ref(false);
const current = ref<Choice | null>(null);
const options: { choice: Choice; label: string }[] = [
  { choice: 'all', label: 'Accept all' },
  { choice: 'analytics', label: 'Analytics only' },
  { choice: 'none', label: 'Reject' },
];

function resolvedPage(): TrackingPage {
  if (props.page) return props.page;
  const path = route.path;
  if (path.includes('/pro/thanks')) return 'thanks';
  if (path === '/pro' || path.startsWith('/pro.')) return 'pro';
  return 'docs';
}

function choose(choice: Choice) {
  current.value = choice;
  const page = resolvedPage();
  updateConsent(choice, page);
  if (page === 'thanks' && hasAnalyticsConsent(choice)) {
    trackPurchase(location.search);
  }
  window.dispatchEvent(new Event('anymd-pro-consent'));
  open.value = false;
}

onMounted(() => {
  if (!enabled) {
    purgeAnalyticsCookies();
    return;
  }
  current.value = savedChoice();
  open.value = !current.value;
  const page = resolvedPage();
  if (loadTags(page) && page === 'thanks') {
    trackPurchase(location.search);
  }
});

watch(
  () => route.path,
  (next, prev) => {
    if (!enabled || next === prev) return;
    const page = resolvedPage();
    if (!loadTags(page)) return;
    trackPageView();
    if (page === 'thanks') trackPurchase(location.search);
  }
);
</script>

<template>
  <div v-if="enabled" id="cookie-settings-footer" class="pro-consent-wrap">
    <p class="pro-cookie-link">
      <a href="#cookie-settings" @click.prevent="open = true">Cookie settings and Your Privacy Choices</a>
    </p>
    <div v-if="open" id="cookie-settings" class="pro-consent" role="region" aria-label="Cookie consent">
      <span>
        This site uses Google Analytics measurement when enabled to see how people find us and which
        features help most. Depending on your choice, Google may set cookies and process identifiers
        such as your IP address. We never send your name or email. Sylphx Limited is the controller.
        You can change this at any time from Cookie settings. <a :href="withBase('/legal/privacy')">Privacy</a>
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
