import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

export async function instantiate(path) {
  const module = await WebAssembly.compile(await readFile(path));
  const imports = {};
  // Exporting target layouts must never touch the GPU. Link only throwing functions;
  // real rendering is separately exercised by the Chromium/OffscreenCanvas harness.
  for (const item of WebAssembly.Module.imports(module)) {
    assert.ok(
      ["ipp_gl", "ipp_diagnostics", "ipp_profiling"].includes(item.module),
    );
    assert.equal(item.kind, "function");
    imports[item.module] ??= {};
    imports[item.module][item.name] = () => {
      throw new Error("Contract export attempted host I/O");
    };
  }
  const instance = await WebAssembly.instantiate(module, imports);
  return instance.exports;
}
