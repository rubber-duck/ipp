/** URLs of an assembled, target-matched browser distribution. */
export function browserRuntime(baseUrl: URL): {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
} {
  return {
    generatedModuleUrl: new URL("generated.js", baseUrl).href,
    workerScriptUrl: new URL("wasm-worker.js", baseUrl).href,
    wasmUrl: new URL("runtime.wasm", baseUrl).href,
  };
}
