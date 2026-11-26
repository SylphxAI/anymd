// Static module worker: load wasm-bindgen's generated ESM without bundling it.
let engine;
self.onmessage = async ({data: {id, name, bytes}}) => {
  let mod;
  try {
    engine ??= import('./wasm/anymd_wasm.js').then(async (mod) => {
      await mod.default();
      return mod;
    });
    mod = await engine;
  } catch {
    self.postMessage({id, outcome:'start-error'});
    return;
  }
  try {
    const started = performance.now();
    const result = JSON.parse(mod.convertDetailed(new Uint8Array(bytes), name));
    self.postMessage({id, outcome:'ok', result:{...result, ms:Math.round(performance.now() - started)}});
  } catch {
    self.postMessage({id, outcome:'read-error'});
  }
};
