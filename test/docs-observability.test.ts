import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { injectObservability } from '../docs/.vitepress/observability';

const source = readFileSync(new URL('../docs/public/obs.js', import.meta.url), 'utf8');
const receiver =
  'https://api.sylphx.com/v1/orgs/org_fs01afbdccgn8xdk1ghj5y47w0vtph/projects/prj_fs013ejakyn22t4bsvrvyj0f2x1pgq/envs/env_fs01p2f389mwmm2xsm1nmvcst69vgd/error_groups:capture';
const fixtureKey = ['sylphx', 'pk', 'live', `z0z0${'0'.repeat(16)}`].join('_');
const release = 'a'.repeat(40);
const env = {
  ANYMD_OBS_CAPTURE_URL: receiver,
  ANYMD_OBS_PUBLISHABLE_KEY: fixtureKey,
  GITHUB_SHA: release,
};

class FixtureEventTarget {
  private listeners = new Map<string, ((event: object) => void)[]>();

  addEventListener(name: string, listener: (event: object) => void) {
    const listeners = this.listeners.get(name) ?? [];
    listeners.push(listener);
    this.listeners.set(name, listeners);
  }

  emit(name: string, props = {}) {
    // Bun's Event.isTrusted cannot be replaced. Model event input without changing platform events.
    const event = { type: name, ...props };
    for (const listener of this.listeners.get(name) ?? []) listener.call(this, event);
  }
}

function browser(path = '/anymd/playground', captureUrl = receiver, pending = false) {
  const posts: { url: string; body: string; options: RequestInit }[] = [];
  const window = new FixtureEventTarget();
  const document = new FixtureEventTarget();
  const location = new URL(path, 'https://sylphxai.github.io');
  const now = { value: 0 };
  const timers = new Map<number, () => void>();
  let nextTimer = 0;
  class Element {
    constructor(
      public tagName: string,
      private empty = false
    ) {}
    isConnected = true;
    closest() {
      return this;
    }
    matches(selector: string) {
      return selector.includes('src=')
        ? this.empty
        : selector.startsWith('button') && this.tagName === 'BUTTON';
    }
    querySelectorAll() {
      return [];
    }
    getAttribute() {
      return 'private-name@example.test';
    }
    hasAttribute() {
      return false;
    }
  }
  class Observer {
    static supportedEntryTypes = ['resource'];
    observe() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  }
  class XMLHttpRequest {
    send() {}
  }
  const fetch = async (url: string, options?: RequestInit) => {
    if (url === receiver && options?.method === 'POST') {
      posts.push({ url, body: String(options.body), options });
      if (pending) return new Promise<void>(() => {});
      throw new Error('fixture delivery failure');
    }
  };
  const body = new Element('BODY');
  Object.assign(window, { fetch });
  Object.assign(document, {
    body,
    currentScript: { dataset: { captureUrl, publishableKey: fixtureKey, release } },
  });
  runInNewContext(source, {
    document,
    window,
    location,
    Element,
    XMLHttpRequest,
    MutationObserver: Observer,
    PerformanceObserver: Observer,
    AbortController,
    TextEncoder,
    performance: { now: () => now.value },
    setTimeout: (callback: () => void) => {
      timers.set(++nextTimer, callback);
      return nextTimer;
    },
    clearTimeout: (id: number) => timers.delete(id),
  });
  return {
    posts,
    location,
    now,
    click: (target: Element, isTrusted = true) =>
      document.emit('click', { target, button: 0, isTrusted }),
    fire: (name: string, props = {}) => window.emit(name, props),
    flushTimers: () => {
      for (const run of timers.values()) run();
    },
    request: () => (window as FixtureEventTarget & { fetch: typeof fetch }).fetch('/private-path'),
    ready: () => document.emit('DOMContentLoaded'),
    body,
    Element,
  };
}

describe('docs diagnostics', () => {
  const html = `<!DOCTYPE html>
<html lang="en-US">
  <head>
    <meta charset="utf-8">
    <link rel="stylesheet" href="/anymd/assets/style.css">
    <script type="module" src="/anymd/assets/app.js"></script>
  </head>
  <body><img src="missing.png"></body>
</html>`;

  test('registers capture listeners before the rendered VitePress assets', () => {
    const script = `<script src="/anymd/obs.js" data-capture-url="${receiver}" data-publishable-key="${fixtureKey}" data-release="${release}"></script>`;
    for (const page of ['index.md', 'guide/getting-started.md', 'playground.md']) {
      expect(injectObservability(html, page, env)).toBe(
        html.replace('<meta charset="utf-8">', `<meta charset="utf-8">\n    ${script}`)
      );
    }
  });

  test('leaves excluded pages and absent or invalid bindings untouched', () => {
    for (const page of [
      '404.md',
      'pro.md',
      'pro/thanks.md',
      'pricing.md',
      'recover.md',
      'storefront/prebuilt/pricing.html',
    ]) {
      expect(injectObservability(html, page, env)).toBe(html);
    }
    for (const invalid of [
      {},
      { ...env, ANYMD_OBS_CAPTURE_URL: `${receiver}x` },
      { ...env, ANYMD_OBS_PUBLISHABLE_KEY: 'fixture' },
      { ...env, GITHUB_SHA: 'aabb' },
    ]) {
      expect(injectObservability(html, 'index.md', invalid)).toBe(html);
    }
  });

  test('captures early assets and empty src, with only fixed payload fields', async () => {
    const b = browser('/anymd/playground?private-token=not-for-reports');
    b.fire('error', { target: new b.Element('AUDIO'), message: 'private-name@example.test' });
    b.body.querySelectorAll = () => [new b.Element('IMG', true)];
    b.ready();
    b.fire('error', { target: new b.Element('IMG') });
    await Promise.resolve();
    expect(b.posts.map(({ body }) => JSON.parse(body).error_event.fingerprint)).toEqual([
      'anymd-docs:BrokenAsset:playground:audio',
      'anymd-docs:BrokenAsset:playground:img',
    ]);
    for (const { body, options } of b.posts) {
      expect(body).not.toContain('private');
      expect(body).not.toContain(fixtureKey);
      expect(options.credentials).toBe('omit');
      expect(options.referrerPolicy).toBe('no-referrer');
      expect(new Headers(options.headers).get('authorization')).toBe(
        ['Bearer', fixtureKey].join(' ')
      );
    }
  });

  test('caps attempts and in-memory breadcrumbs independently of deduplication', async () => {
    const b = browser('/anymd/');
    const a = new b.Element('A');
    for (let i = 0; i < 18; i++) b.click(a);
    expect(b.posts).toHaveLength(0);
    for (const tag of ['IMG', 'AUDIO', 'VIDEO', 'SOURCE', 'SCRIPT', 'LINK']) {
      b.fire('error', { target: new b.Element(tag) });
    }
    await Promise.resolve();
    expect(b.posts).toHaveLength(5);
    expect(JSON.parse(b.posts[0]?.body ?? '{}').error_event.breadcrumbs).toHaveLength(15);
    b.now.value += 60000;
    for (const tag of ['IMG', 'LINK']) b.fire('error', { target: new b.Element(tag) });
    await Promise.resolve();
    expect(b.posts).toHaveLength(6);
  });

  test('bounds delivery and sends nothing while on excluded SPA routes', async () => {
    const b = browser('/anymd/', receiver, true);
    b.location.pathname = '/anymd/pro/thanks';
    b.fire('anymd:navigation');
    b.fire('unhandledrejection');
    b.location.pathname = '/anymd/playground';
    b.fire('anymd:navigation');
    b.fire('unhandledrejection');
    await Promise.resolve();
    expect(b.posts).toHaveLength(1);
    const post = b.posts[0];
    expect(
      JSON.parse(post?.body ?? '{}').error_event.breadcrumbs.map(
        (crumb: { message: string }) => crumb.message
      )
    ).toEqual(['playground']);
    expect(post?.options.signal?.aborted).toBe(false);
    b.flushTimers();
    expect(post?.options.signal?.aborted).toBe(true);
  });

  test('quiet on foreign hosts, Pro pages, wrong endpoints, and effective or untrusted clicks', async () => {
    for (const b of [
      browser('https://preview.example/anymd/'),
      browser('/anymd/pro/thanks'),
      browser('/anymd/', `${receiver}x`),
    ]) {
      b.fire('unhandledrejection');
      await Promise.resolve();
      expect(b.posts).toHaveLength(0);
    }
    const b = browser();
    const button = new b.Element('BUTTON');
    for (let i = 0; i < 3; i++) b.click(button, false);
    b.flushTimers();
    for (let i = 0; i < 3; i++) b.click(button);
    await b.request();
    b.flushTimers();
    expect(b.posts).toHaveLength(0);
    for (let i = 0; i < 3; i++) b.click(button);
    b.flushTimers();
    await Promise.resolve();
    expect(b.posts).toHaveLength(1);
    expect(JSON.parse(b.posts[0]?.body ?? '{}').error_event.fingerprint).toBe(
      'anymd-docs:DeadClick:playground:button'
    );
  });
});
