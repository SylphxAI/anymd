import { solidStart } from '@solidjs/start/config';
import tailwindcss from '@tailwindcss/vite';
import { nitro } from 'nitro/vite';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';

// Relative imports keep the whole converter in one immutable directory.
function studioAssets() {
  const files = ['studio-worker.js', 'wasm/anymd_wasm.js', 'wasm/anymd_wasm_bg.wasm'].map(name => ({
    name,
    source: readFileSync(new URL('./public/' + name, import.meta.url)),
  }));
  const hash = createHash('sha256');
  for (const { source } of files) hash.update(createHash('sha256').update(source).digest());
  const directory = '_build/assets/studio-' + hash.digest('hex');
  return {
    url: '/' + directory + '/' + files[0].name,
    plugin: {
      name: 'studio-assets',
      applyToEnvironment: environment => environment.name === 'client',
      generateBundle() {
        for (const { name, source } of files) {
          this.emitFile({ type: 'asset', fileName: directory + '/' + name, source });
        }
      },
    } satisfies Plugin,
  };
}

export default defineConfig(({ command }) => {
  const studio = command === 'build' ? studioAssets() : undefined;
  return {
    define: { __STUDIO_WORKER_URL__: JSON.stringify(studio ? studio.url : '/studio-worker.js') },
    resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
    build: { target: 'es2022' },
    plugins: [
      ...(studio ? [studio.plugin] : []),
      tailwindcss(),
      solidStart({ middleware: './src/middleware.ts' }),
      nitro({
        preset: 'node-server',
        // Nitro serves static files before SolidStart's request middleware.
        routeRules: { '/**': { headers: { 'Strict-Transport-Security': 'max-age=63072000; includeSubDomains' } } },
      }),
    ],
  };
});
