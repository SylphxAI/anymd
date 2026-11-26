import { createHash } from 'node:crypto';
import { expect, type Response, type Route } from '@playwright/test';

const files = ['studio-worker.js', 'wasm/anymd_wasm.js', 'wasm/anymd_wasm_bg.wasm'] as const;
const path = /^\/_build\/assets\/studio-([a-f0-9]{64})\/(.+)$/;

export function studioAsset(origin: string, requestUrl: string) {
  const url = new URL(requestUrl);
  if (url.origin !== origin || url.search) return;
  const match = url.pathname.match(path);
  const file = files.find(file => file === match?.[2]);
  if (match && file) return { graph: match[1], file };
}

export function captureStudioAssets(origin: string) {
  const assets: { url: string; sha256: string; revision?: string }[] = [];
  return {
    async capture(route: Route) {
      const request = route.request();
      const url = request.url();
      expect(studioAsset(origin, url)).toBeDefined();
      expect(request.method()).toBe('GET');
      expect(request.postData()).toBeNull();
      // Give Chromium the exact bytes we hash. Reading a worker response after
      // conversion races with Studio terminating the worker and its CDP session.
      const response = await route.fetch({ maxRedirects: 0 });
      try {
        expect(response.ok(), url).toBe(true);
        const headers = response.headers();
        expect(headers['content-type']).toContain(url.endsWith('.wasm') ? 'application/wasm' : 'javascript');
        assets.push({ url, sha256: createHash('sha256').update(await response.body()).digest('hex'), revision: headers['x-sylphx-revision-sha'] });
        await route.fulfill({ response });
      } finally {
        await response.dispose();
      }
    },
    prove(document: Response | null, live: boolean, failedImport?: string) {
      const graph = studioAsset(origin, assets[0]?.url ?? origin)?.graph;
      expect(graph, 'Browser requested the content-addressed converter').toBeDefined();
      for (const asset of assets) expect(studioAsset(origin, asset.url)?.graph, asset.url).toBe(graph);
      if (failedImport) {
        expect(studioAsset(origin, failedImport)?.graph, 'Retry uses the same graph as the failed import').toBe(graph);
      }
      expect(document?.headers()['content-security-policy']).toMatch(/(?:^|; )worker-src 'self'(?:;|$)/);
      const hash = createHash('sha256');
      for (const file of files) {
        const matches = assets.filter(asset => studioAsset(origin, asset.url)?.file === file);
        expect(matches.length, file).toBeGreaterThan(0);
        const digests = new Set(matches.map(asset => asset.sha256));
        expect(digests.size, file).toBe(1);
        hash.update(Buffer.from(matches[0].sha256, 'hex'));
      }
      expect(hash.digest('hex'), 'The graph URL matches the worker, module and WASM bytes').toBe(graph);
      // Unchanged immutable assets may carry an earlier deployment's cache header.
      const revision = document?.headers()['x-sylphx-revision-sha'];
      if (live) {
        expect(revision).toMatch(/^[a-f0-9]{40}$/);
        console.log(JSON.stringify({ studio: document?.url(), revision, assets: [...new Map(assets.map(asset => [asset.url, asset])).values()] }));
      }
    },
  };
}
