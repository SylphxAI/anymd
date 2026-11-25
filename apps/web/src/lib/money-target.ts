import { productionMoney } from './money-production.generated';

export function testMoneyParent() {
  const parent = process.env.ANYMD_MONEY_TEST_PARENT?.trim();
  if (parent && /^orgs\/org_[a-z0-9]+\/projects\/prj_[a-z0-9]+\/envs\/env_[a-z0-9]+$/.test(parent)) return parent;
}

export function testMoneyOrigin() {
  const endpoint = new URL(process.env.ANYMD_MONEY_TEST_URL || productionMoney.origin);
  if (endpoint.protocol === 'https:' ||
      endpoint.protocol === 'http:' && ['localhost', '127.0.0.1'].includes(endpoint.hostname)) return endpoint.origin;
}
