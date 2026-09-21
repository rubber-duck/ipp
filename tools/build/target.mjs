/** Node-only operations on explicitly prepared target artifacts. */
import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  assembleBrowserHost,
  assembleClientSupport,
} from "../../packages/ipp-client/tools/assemble.mjs";
import { bundleBrowser } from "./helpers.mjs";
import { instantiate } from "./wasm.mjs";

const [action, path, output] = process.argv.slice(2);
if (action === "export") {
  const exported = await instantiate(path);
  const pointer = exported.ipp_contract_ptr() >>> 0;
  const length = exported.ipp_contract_len() >>> 0;
  await writeFile(
    output,
    new Uint8Array(exported.memory.buffer, pointer, length),
  );
} else if (action === "support") {
  assembleClientSupport(path);
} else if (action === "world") {
  const generated = await import(
    pathToFileURL(resolve(path, "generated.js")).href
  );
  assert.equal(generated.CAPABILITIES.builtinAssets, true);
  assert.equal(generated.CAPABILITIES.textures, true);
  assert.equal(generated.CAPABILITIES.picking, true);
  if (output === "wasm") {
    const exported = await instantiate(resolve(path, "export.wasm"));
    const runtime = await instantiate(resolve(path, "runtime.wasm"));
    assert.equal(runtime.ipp_schema_hash(), exported.ipp_schema_hash());
    assert.equal(
      BigInt.asUintN(64, runtime.ipp_schema_hash()),
      generated.SCHEMA_HASH,
    );
    assert.equal("ipp_contract_ptr" in runtime, false);
    await assembleBrowserHost(path);
  }
} else if (action === "bundle") {
  await bundleBrowser(path, output);
} else {
  throw new Error(
    "Expected export, support, world or bundle with explicit paths",
  );
}
