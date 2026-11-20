// Static module worker: load wasm-bindgen's generated ESM without bundling it.
let engine;
self.onmessage = async ({data: {id, name, bytes}}) => {
  try {
    engine ??= import('/wasm/anymd_wasm.js').then(async (mod) => {
      await mod.default({module_or_path:'/wasm/anymd_wasm_bg.wasm'});
      return mod;
    });
    const mod = await engine;
    const started = performance.now();
    const result = JSON.parse(mod.convertDetailed(new Uint8Array(bytes), name));
    self.postMessage({id, outcome:'ok', result:{...result, ms:Math.round(performance.now() - started)}});
  } catch {
    self.postMessage({id, outcome:'error'});
  }
};
