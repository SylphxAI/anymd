import assert from 'node:assert/strict';
import { test } from 'node:test';
import { resolveEffectiveConsent } from '@sylphx/sdk/signal';
import { behavioralParameters, type BehavioralEvent, gaDisable, isPricingPage, measurementId, pageLocation, trackBehavioral } from './analytics';

const origin = 'https://anymd.sylphx.com';

function withBrowser(run: (commands: unknown[][], google: {[gaDisable]: boolean}) => void) {
  const commands: unknown[][] = [];
  const google = {[gaDisable]:false, gtag: (...args: unknown[]) => commands.push(args)};
  Object.assign(globalThis, {location:{origin}, window:google});
  try { run(commands, google); }
  finally {
    Reflect.deleteProperty(globalThis, 'location');
    Reflect.deleteProperty(globalThis, 'window');
  }
}

test('analytics uses only known sections and canonical localized page locations', () => withBrowser(() => {
  for (const [input, path, pricing] of [
    ['/zh-TW', '/zh-TW', false],
    ['/zh-Hant/', '/zh-TW', false],
    ['/zh-TW/pricing', '/zh-TW/pricing', true],
    ['/zh-TW/studio?file=private#private', '/zh-TW/studio', false],
    ['/zh-Hant/studio?file=private', '/zh-TW/studio', false],
    ['/zh-Hant/pricing/?session_id=private#proof', '/zh-TW/pricing', true],
    ['/zh-TW/secret-token', '/other', false],
    ['/arbitrary/path', '/other', false],
    ['/pricing?proof=private', '/pricing', true],
    ['/pro', '/pro', true],
    ['/pro/thanks?session_id=private', '/pro', false],
    ['/cli-auth/private?challenge=private#proof', '/cli-auth', false],
  ] as const) {
    assert.equal(pageLocation(input), origin + path, input);
    assert.equal(isPricingPage(input), pricing, input);
  }
}));

test('behavioral transport drops all non-schema fields and non-allowlisted formats', () => withBrowser(commands => {
  const payload = {
    file_format:'private.pdf', source_kind:'local_file' as const, unit_count:2, token_count:370, duration_ms:49,
    filename:'private.pdf', markdown:'private document contents', email:'private@example.invalid',
  };
  trackBehavioral(resolveEffectiveConsent({storedChoice:'analytics_only'}), '/studio?query=private#proof', 'document_converted', payload);
  const context = {page_location:origin + '/studio', page_referrer:'', page_title:'Anymd'};
  assert.deepEqual(commands, [
    ['set', context],
    ['event', 'document_converted', {
      file_format:'other', source_kind:'local_file', unit_count:2, token_count:370, duration_ms:49, ...context, send_to:measurementId,
    }],
  ]);
}));

test('behavioral transport emits nothing before readiness, when denied, under GPC, or when stopped', () => withBrowser((commands, google) => {
  const allowed = resolveEffectiveConsent({storedChoice:'granted'});
  const denied = resolveEffectiveConsent({storedChoice:'denied'});
  const gpc = resolveEffectiveConsent({storedChoice:'analytics_only'}, new Headers({'Sec-GPC':'1'}));
  for (const consent of [undefined, denied, gpc]) trackBehavioral(consent, '/studio', 'quote_searched', {matched:false});
  google[gaDisable] = true;
  trackBehavioral(allowed, '/studio', 'quote_searched', {matched:false});
  assert.deepEqual(commands, []);
}));

test('comparison copy and Pro CTA events accept only known labels and never send clipboard text', () => withBrowser(commands => {
  const consent = resolveEffectiveConsent({storedChoice:'analytics_only'});
  const context = {page_location:origin + '/vs', page_referrer:'', page_title:'Anymd'};
  const copied = {surface:'markdownify_mcp', command_kind:'mcp_config', text:'private clipboard input'} as const;
  const pro = {entry_surface:'seo_vs-markdownify-mcp', gclid:'private click id'} as const;
  const path = '/vs/markdownify-mcp?gclid=private#proof';
  trackBehavioral(consent, path, 'output_copied', copied);
  trackBehavioral(consent, path, 'pro_cta_clicked', pro);
  trackBehavioral(consent, '/vs/docling', 'pro_cta_clicked', {entry_surface:'seo_vs-docling'});
  const bridge = {entry_surface:'article_bridge', utm_source:'private source'} as const;
  trackBehavioral(consent, path, 'pro_cta_clicked', bridge);
  // Untyped callers cannot smuggle arbitrary labels through the runtime projection.
  trackBehavioral(consent, path, 'output_copied', {...copied, command_kind:'private-input' as 'mcp_config'});
  trackBehavioral(consent, path, 'pro_cta_clicked', {entry_surface:'private-input' as 'seo_vs-markdownify-mcp'});
  assert.deepEqual(commands, [
    ['set', context], ['event', 'output_copied', {...context, send_to:measurementId, surface:'markdownify_mcp', command_kind:'mcp_config'}],
    ['set', context], ['event', 'pro_cta_clicked', {...context, send_to:measurementId, entry_surface:'seo_vs-markdownify-mcp'}],
    ['set', context], ['event', 'pro_cta_clicked', {...context, send_to:measurementId, entry_surface:'seo_vs-docling'}],
    ['set', context], ['event', 'pro_cta_clicked', {...context, send_to:measurementId, entry_surface:'article_bridge'}],
  ]);
}));

test('Pro CTA projection accepts organic labels and rejects non-CTA or arbitrary values', () => {
  for (const entry_surface of ['seo_vs-hub', 'seo_verifiable-video-and-cite-check-ai-agents', 'task_check_pdf_citation'] as const) {
    assert.deepEqual(behavioralParameters('pro_cta_clicked', {entry_surface}), {entry_surface});
  }
  for (const entry_surface of ['pricing', 'pro', 'cli_pdf', 'npm_citra', 'seo_vs-private-input', 'task_private_input', 'private-input']) {
    assert.equal(behavioralParameters(...(['pro_cta_clicked', {entry_surface}] as BehavioralEvent)), undefined);
  }
});
