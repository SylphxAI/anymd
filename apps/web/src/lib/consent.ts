export const consentKey = 'anymd.cookie-settings.v1';
export type Choice = 'reject' | 'analytics' | 'all';
export type Region = 'opt-in' | 'opt-out';
export type Permission = 'granted' | 'denied';

const optIn = new Set('AT BE BG HR CY CZ DK EE FI FR DE GR HU IS IE IT LV LI LT LU MT NL NO PL PT RO SK SI ES SE GB CH AX GF GP MQ MF RE YT'.split(' '));
// Only assigned ISO country codes count as known. Cloudflare's XX/T1, missing
// responses and unrecognized codes are opt-in, not the rest-of-world default.
const countries = new Set(('AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ ' +
  'CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR ' +
  'GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP ' +
  'KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ ' +
  'NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ ' +
  'TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW').split(' '));

export function parseChoice(value: unknown): Choice | null {
  return value === 'reject' || value === 'analytics' || value === 'all' ? value : null;
}

export function traceRegion(text: string, hostname: string): Region {
  const lines = text.split(/\r?\n/);
  const locations = lines.filter(line => line.startsWith('loc='));
  const hosts = lines.filter(line => line.startsWith('h='));
  if (hosts.length !== 1 || hosts[0] !== 'h=' + hostname || locations.length !== 1) return 'opt-in';
  const country = locations[0].slice(4);
  return countries.has(country) && !optIn.has(country) ? 'opt-out' : 'opt-in';
}

export function consentFor(region: Region, choice: Choice | null, gpc: boolean) {
  const analytics: Permission = !gpc && (choice === 'analytics' || choice === 'all' || (choice === null && region === 'opt-out')) ? 'granted' : 'denied';
  const ads: Permission = analytics === 'granted' && (choice === 'all' || choice === null) ? 'granted' : 'denied';
  return { analytics_storage: analytics, ad_storage: ads, ad_user_data: ads, ad_personalization: 'denied' as const };
}
