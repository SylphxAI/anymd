import { A, useLocation } from '@solidjs/router';
import { splitProps, type ComponentProps } from 'solid-js';
import { isBackendHref } from './backend-href';
import { canonicalOrigin } from './app-origin';
import { carryCampaign } from './campaign';
import { useCookieConsent } from '@/components/cookie-consent';

// Render the real URL (including during SSR) so reloads and open-in-new-tab
// work without a click handler, storage or a change to the nested Auth return.
export function CampaignLink(props: ComponentProps<typeof A>) {
  const [local, rest] = splitProps(props, ['href', 'rel']);
  const route = useLocation();
  const consent = useCookieConsent();
  return <A {...rest} rel={isBackendHref(local.href, canonicalOrigin) ? 'external' : local.rel} href={consent.carryCampaign() ? carryCampaign(local.href, route.search) : local.href} />;
}
