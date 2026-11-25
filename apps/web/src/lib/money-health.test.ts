import assert from 'node:assert/strict';
import { test, type TestContext } from 'node:test';
// @ts-expect-error The Bun test loader is not part of the web app's Node types.
import { mock } from 'bun:test';

// Use the same generated-module fixture as purchase-conversion.test.ts.
const productionMoney = {
  origin: 'https://production.example.invalid', parent: 'orgs/org_prod/projects/prj_prod/envs/env_prod', pk: 'unused',
};
mock.module('./money-production.generated', () => ({ productionMoney }));

const liveKey = ['sylphx', 'sk', 'live', 'fixture'].join('_');
const testKey = ['sylphx', 'sk', 'test', 'fixture'].join('_');
const testParent = 'orgs/org_test/projects/prj_test/envs/env_test';
const testOrigin = 'https://money.example.invalid';
const settings = {
  ANYMD_MONEY_MODE: 'live', SYLPHX_MONEY_API_KEY: liveKey,
  ANYMD_MONEY_TEST_API_KEY: testKey, ANYMD_MONEY_TEST_PARENT: testParent, ANYMD_MONEY_TEST_URL: testOrigin,
};

async function setup(t: TestContext, upstream: (request: Request) => Response | Promise<Response>) {
  const { GET } = await import('../routes/api/health');
  const original = { ...process.env };
  const originalFetch = globalThis.fetch;
  const originalNow = performance.now;
  const calls: Request[] = [];
  let now = 0;
  globalThis.fetch = (async (url, init) => {
    const request = new Request(url, init);
    calls.push(request);
    return upstream(request);
  }) as typeof fetch;
  performance.now = () => now;
  t.after(() => {
    for (const key of Object.keys(settings)) {
      if (original[key] === undefined) delete process.env[key];
      else process.env[key] = original[key];
    }
    globalThis.fetch = originalFetch;
    performance.now = originalNow;
  });
  // Clear the process cache through its unconfigured path before each test.
  delete process.env.SYLPHX_MONEY_API_KEY;
  process.env.ANYMD_MONEY_MODE = 'live';
  assert.equal((await (await GET()).json()).money, 'unconfigured');
  Object.assign(process.env, settings);
  return { calls, advance: (ms: number) => { now += ms; }, async expectHealth(money: 'ok' | 'error' | 'unconfigured') {
    const response = await GET();
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('cache-control'), 'private, no-store');
    assert.deepEqual([...response.headers.keys()], ['cache-control', 'content-type']);
    assert.deepEqual(await response.json(), { status: 'ok', service: 'anymd-web', money });
  } };
}

test('health does not probe with absent or wrong-mode credentials or invalid preview config', async t => {
  const { calls, expectHealth } = await setup(t, () => Response.json({ env: productionMoney.parent }));
  delete process.env.SYLPHX_MONEY_API_KEY;
  await expectHealth('unconfigured'); // Live never falls back to the valid preview binding.
  process.env.SYLPHX_MONEY_API_KEY = testKey;
  await expectHealth('unconfigured');
  delete process.env.ANYMD_MONEY_MODE;
  process.env.SYLPHX_MONEY_API_KEY = liveKey;
  process.env.ANYMD_MONEY_TEST_API_KEY = liveKey;
  await expectHealth('unconfigured');
  process.env.ANYMD_MONEY_TEST_API_KEY = testKey;
  process.env.ANYMD_MONEY_TEST_PARENT = 'orgs/org_test';
  await expectHealth('unconfigured');
  process.env.ANYMD_MONEY_TEST_PARENT = testParent;
  for (const url of ['invalid URL', 'http://money.example.invalid', 'file:///local']) {
    process.env.ANYMD_MONEY_TEST_URL = url;
    await expectHealth('unconfigured');
  }
  assert.equal(calls.length, 0);
});

test('health checks the live Money identity using a read-only bearer request', async t => {
  const { calls, expectHealth } = await setup(t, () => Response.json({ env: '/' + productionMoney.parent + '//', key: liveKey }));
  process.env.SYLPHX_MONEY_API_KEY = ' ' + liveKey + ' ';
  await expectHealth('ok');
  assert.equal(calls.length, 1);
  const [request] = calls;
  assert.equal(request.url, productionMoney.origin + '/v1/whoami');
  assert.equal(request.method, 'GET');
  assert.equal(request.body, null);
  assert.deepEqual([...request.headers], [['accept', 'application/json'], ['authorization', 'Bearer ' + liveKey]]);
  assert.equal(request.cache, 'no-store');
  assert.equal(request.redirect, 'error');
});

test('preview probes its test key and switches targets when changed, or defaults to the Money origin', async t => {
  const { calls, expectHealth } = await setup(t, () => Response.json({ env: testParent }));
  process.env.ANYMD_MONEY_MODE = 'preview';
  process.env.ANYMD_MONEY_TEST_URL = testOrigin + '/unused';
  await expectHealth('ok');
  process.env.ANYMD_MONEY_TEST_URL = 'http://127.0.0.1:3031';
  await expectHealth('ok');
  delete process.env.ANYMD_MONEY_TEST_URL;
  await expectHealth('ok');
  process.env.ANYMD_MONEY_TEST_PARENT = 'orgs/org_test/projects/prj_test/envs/env_other';
  await expectHealth('error');
  assert.deepEqual(calls.map(request => request.url), [testOrigin, 'http://127.0.0.1:3031', productionMoney.origin, productionMoney.origin].map(origin => origin + '/v1/whoami'));
  assert.ok(calls.every(request => request.headers.get('authorization') === 'Bearer ' + testKey));
});

test('concurrent refreshes coalesce and the success TTL starts on completion', async t => {
  const pending = Promise.withResolvers<Response>();
  const { calls, advance, expectHealth } = await setup(t, () => pending.promise);
  const concurrent = Array.from({ length: 8 }, () => expectHealth('ok'));
  assert.equal(calls.length, 1);
  advance(1_000);
  pending.resolve(Response.json({ env: productionMoney.parent }));
  await Promise.all(concurrent);
  advance(59_999);
  await expectHealth('ok');
  assert.equal(calls.length, 1);
  advance(1);
  // A new response is required at the exact expiry, not another body read.
  await expectHealth('error');
  assert.equal(calls.length, 2);
});

test('a key change cannot coalesce with or reuse the old key’s result', async t => {
  const oldProbe = Promise.withResolvers<Response>();
  const { calls, expectHealth } = await setup(t, req => req.headers.get('authorization') === 'Bearer ' + liveKey
    ? oldProbe.promise : Response.json({ env: productionMoney.parent }));
  const oldRequest = expectHealth('error');
  process.env.SYLPHX_MONEY_API_KEY = liveKey + 'rotated';
  await expectHealth('ok');
  oldProbe.resolve(new Response(null, { status: 401 }));
  await oldRequest;
  await expectHealth('ok');
  assert.equal(calls.length, 2);
  delete process.env.SYLPHX_MONEY_API_KEY;
  await expectHealth('unconfigured');
  process.env.SYLPHX_MONEY_API_KEY = liveKey + 'rotated';
  await expectHealth('ok');
  assert.equal(calls.length, 3);
});

test('error is cached for 15 seconds before recovery', async t => {
  let available = false;
  const { calls, advance, expectHealth } = await setup(t, () => Response.json({ env: productionMoney.parent, private: liveKey }, { status: available ? 200 : 500 }));
  await expectHealth('error');
  available = true;
  advance(14_999);
  await expectHealth('error');
  assert.equal(calls.length, 1);
  advance(1);
  await expectHealth('ok');
  assert.equal(calls.length, 2);
});

test('refusal, malformed JSON, and invalid binding never leak upstream details', async t => {
  const invalid = [
    () => Response.json({ env: productionMoney.parent, secret: liveKey }, { status: 401 }),
    () => Response.json({ env: productionMoney.parent }, { status: 500 }),
    () => new Response('{' + liveKey),
    () => new Response(null, { status: 204 }),
    () => Response.json(null),
    () => Response.json({}),
    () => Response.json({ env: 42 }),
    () => Response.json({ env: '' }),
    () => Response.json({ env: '//' }),
    () => Response.json({ env: ' /' + productionMoney.parent }),
    () => Response.json({ env: testParent }),
    () => { throw new Error(liveKey + productionMoney.parent); },
  ];
  let caseIndex = 0;
  const { calls, advance, expectHealth } = await setup(t, () => invalid[caseIndex++]());
  for (const _ of invalid) {
    await expectHealth('error');
    advance(15_000);
  }
  assert.equal(calls.length, invalid.length);
});

for (const phase of ['headers', 'body']) {
  test('the same 2 second deadline covers stalled ' + phase, async t => {
    const timeout = AbortSignal.timeout;
    const deadline = new AbortController();
    const durations: number[] = [];
    AbortSignal.timeout = ms => { durations.push(ms); return deadline.signal; };
    t.after(() => { AbortSignal.timeout = timeout; });
    const started = Promise.withResolvers<void>();
    const { calls, expectHealth } = await setup(t, request => {
      if (phase === 'headers') return new Promise((_, reject) => {
        request.signal.addEventListener('abort', () => reject(request.signal.reason), { once: true });
        started.resolve();
      });
      return new Response(new ReadableStream({ pull(controller) {
        request.signal.addEventListener('abort', () => controller.error(request.signal.reason), { once: true });
        started.resolve();
      } }));
    });
    const result = expectHealth('error');
    await started.promise;
    deadline.abort(new DOMException('fixture deadline', 'TimeoutError'));
    await result;
    assert.deepEqual(durations, [2_000]);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].signal.aborted, true);
  });
}
