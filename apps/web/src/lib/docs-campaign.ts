import { carryCampaign } from './campaign';

const entities = {'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;'} as const;
const characters = {'&amp;':'&', '&lt;':'<', '&gt;':'>', '&quot;':'"'} as const;

export function campaignDocHtml(html: string, search: string) {
  if (!search) return html;
  // Only match links marked by the trusted Markdown renderer. Code and text
  // containing markup are escaped, and raw Markdown HTML is disabled.
  return html.replace(/<a href="[^"]*"([^>]*? data-doc-href="([^"]*)"[^>]*?)>/g, (_link, attrs: string, encoded: string) => {
    const href = encoded.replace(/&(?:amp|lt|gt|quot);/g, entity => characters[entity as keyof typeof characters]);
    const decorated = carryCampaign(href, search);
    return '<a href="' + decorated.replace(/[&<>"]/g, char => entities[char as keyof typeof entities]) + '"' + attrs + '>';
  });
}
