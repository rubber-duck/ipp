/** Verify a browser distribution: its runtime, contract, client and assembled host. */
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  assembleBrowserHost,
  CLIENT_SUPPORT_MODULES,
} from "../../packages/ipp-client/tools/assemble.mjs";

import { artifact } from "./helpers.mjs";
import { instantiate } from "./wasm.mjs";

/** WebGL bridge imports of whole-Surface cache targets. */
const SURFACE_CACHE_BRIDGE_IMPORTS = [
  "surface_cache_limit",
  "create_surface_cache_target",
  "resize_surface_cache_target",
  "begin_surface_cache_target",
  "end_surface_cache_target",
  "draw_surface_cache",
  "delete_surface_cache_target",
];
const request = JSON.parse(await readFile(process.argv[2], "utf8"));
const { configuration, features, directory } = request;
const rendering = features.includes("render");
const instrumentation = features.includes("instrumentation");
await mkdir(directory, { recursive: true });
const runtimePath = resolve(directory, "runtime.wasm");
const contractPath = resolve(directory, "contract.bin");
const sourcePath = resolve(directory, "generated.ts");

// The contract was read from this runtime; the client must match it exactly.
const runtime = await instantiate(runtimePath);
const hash = BigInt.asUintN(64, runtime.ipp_schema_hash());
const contractPointer = runtime.ipp_contract_ptr() >>> 0;
const contractLength = runtime.ipp_contract_len() >>> 0;
assert.deepEqual(
  new Uint8Array(
    runtime.memory.buffer,
    contractPointer,
    contractLength,
  ).slice(),
  new Uint8Array(await readFile(contractPath)),
);
const client = await import(
  pathToFileURL(resolve(directory, "generated.js")).href
);
assert.equal(client.SCHEMA_HASH, hash);
assert.equal("CAPABILITIES" in client, false);
assert.equal("features" in client.TARGET, false);
const source = await readFile(sourcePath, "utf8");
assert.equal(
  source.includes('case "setFieldIf"'),
  true,
  "baseline command codecs are missing",
);
for (const method of ["saveWorld", "loadWorld"])
  assert.equal(method in client.IppHostClient.prototype, true);
for (const method of [
  "registerAsset",
  "onResourceChange",
  "subscribeGuiEffects",
])
  assert.equal(method in client.IppClient.prototype, true, method);
assert.equal("guiAction" in client.Entity, true);
for (const name of ["encodeSkeletonAsset", "encodeSkinnedMesh"])
  assert.equal(typeof client[name], "function", name);
// Retired authoring entry points stay absent from generated clients.
for (const method of [
  "editSurface",
  "editGui",
  "editGuiBatch",
  "editGuiBatchChunk",
  "submitGuiInput",
  "semanticSnapshot",
  "createGuiNodeHandle",
  "guiSnapshot",
  "guiSnapshotPage",
  "replaceControl",
  "semanticAction",
  "uploadMesh",
  "uploadTexture",
  "loadBuiltinMesh",
  "loadBuiltinTexture",
])
  assert.equal(method in client.IppClient.prototype, false, method);
assert.equal(client.components.GuiRoot, undefined);
assert.equal(source.includes("REQUEST_LOAD_BUILTIN_TEXTURE"), false);
// The shipped client carries resolved codec bounds; the descriptive wire
// manifest is a separate generated module that only tests and tools import.
const manifestSource = await readFile(
  resolve(directory, "generated-manifest.ts"),
  "utf8",
);
assert.equal(manifestSource.includes(`SCHEMA_HASH = ${hash}n;`), true);
for (const name of [
  "WIRE_TAG_LAYOUTS",
  "WIRE_LAYOUTS",
  "WIRE_CONVENTIONS",
  "ASSET_FORMATS",
]) {
  assert.equal(name in client, false, `${name} leaked into the client`);
  assert.equal(source.includes(name), false, `${name} leaked into the client`);
  assert.equal(manifestSource.includes(`export const ${name} =`), true);
}
assert.equal(source.includes("generated-manifest"), false);
assert.equal(Number.isSafeInteger(client.MAX_MESSAGE_BYTES), true);
assert.equal(typeof client.IppClient.connectWebSocket, "function");
assert.equal("connect" in client.IppClient, false);
assert.equal(client.components.UnlitTexture.id, 6);
assert.equal(client.components.BaseColorTexture.id, 19);

const hostArtifacts = await assembleBrowserHost(directory, {
  rendering,
  instrumentation,
});
const runtimeBytes = await readFile(runtimePath);
const glImports = WebAssembly.Module.imports(
  await WebAssembly.compile(runtimeBytes),
);
const imports = (module) => glImports.some((entry) => entry.module === module);
const importNamed = (name) => glImports.some((entry) => entry.name === name);
const embeds = (text) => runtimeBytes.includes(Buffer.from(text));

// Renderer axis: a headless distribution links no GL, shaders or bridge.
assert.equal(imports("ipp_gl"), rendering, "GL imports differ from renderer");
assert.equal(imports("ipp_presentation"), rendering);
assert.equal(typeof runtime.ipp_render_attach === "function", rendering);
for (const text of [
  "#version 300 es",
  "u_lights[IPP_MAX_LIGHTS",
  "u_shadow_settings",
  "sampler2D",
  "v_surface_position",
  "u_surface_cache",
  "u_joints",
  "u_pose_weight",
])
  assert.equal(embeds(text), rendering, `${text} differs from renderer`);
for (const name of [
  "create_texture",
  "create_gui_batch",
  "set_skin_palette",
  "draw_pose",
  ...SURFACE_CACHE_BRIDGE_IMPORTS,
])
  assert.equal(importNamed(name), rendering, `${name} differs from renderer`);
if (rendering) {
  const bridge = await readFile(resolve(directory, "webgl.js"), "utf8");
  for (const name of [
    "set_lighting",
    "create_texture",
    "create_shadow_map",
    "draw_surface_path",
    "draw_gui_batch",
    "write_gui_batch",
    "create_glyph_atlas_page",
    "draw_pose",
    "surfaceCacheTargetsLive",
    ...SURFACE_CACHE_BRIDGE_IMPORTS,
  ])
    assert.equal(bridge.includes(name), true, `bridge lacks ${name}`);
}

// Every distribution compiles every built-in provider.
for (const recipe of [
  "ipp://mesh/plane?",
  "ipp://mesh/sphere?",
  "ipp://mesh/pill-outline?",
  "ipp://texture/uv-grid?",
])
  assert.equal(embeds(recipe), true, `${recipe} provider is missing`);

// Every build logs and answers statistics; instrumentation adds the testing
// controls and the profiler.
assert.equal(imports("ipp_diagnostics"), true);
assert.equal(typeof runtime.ipp_diagnostics_set_level, "function");
assert.equal(embeds("batch.begin"), true);
assert.equal(imports("ipp_profiling"), instrumentation);
for (const name of [
  "ipp_resource_poll",
  "ipp_resource_complete",
  "ipp_resource_input_reserve",
  "ipp_host_open",
  "ipp_host_close",
  "ipp_connection_limit",
  "ipp_delivery_limit",
  "ipp_request_window",
  "ipp_connection_open",
  "ipp_connection_close",
  "ipp_connection_dispose",
  "ipp_connection_pending",
  "ipp_connection_failed",
  "ipp_receive",
  "ipp_tick",
  "ipp_connection_poll",
  "ipp_output_delivery_id",
  "ipp_output_copied",
  "ipp_delivery_complete",
])
  assert.equal(typeof runtime[name], "function", `missing export ${name}`);
// Statistics and records are present in every render build; testing overrides,
// the forced panic and profiling exports only with instrumentation.
for (const [name, expected] of [
  ["ipp_render_tick", false],
  ["ipp_render_resize", false],
  ["ipp_render_draw_calls", false],
  ["ipp_render_triangles", false],
  ["ipp_render_failed_draw_calls", false],
  ["ipp_render_invalid_camera", false],
  ["ipp_render_uploaded_bytes", false],
  ["ipp_render_glyph_pages", false],
  ["ipp_render_surface_cache_repaints", false],
  ["ipp_render_detach", rendering],
  ["ipp_render_max_viewport_width", rendering],
  ["ipp_render_max_viewport_height", rendering],
  ["ipp_render_statistics_ptr", rendering],
  ["ipp_render_statistics_len", rendering],
  ["ipp_render_gui_layout_ptr", rendering],
  ["ipp_render_surface_cache_records_ptr", rendering],
  ["ipp_render_surface_cache_records_len", rendering],
  ["ipp_resource_buffered_bytes", true],
  ["ipp_render_set_exhaustive_draw_checks", rendering && instrumentation],
  ["ipp_render_set_glyph_atlas_limits", rendering && instrumentation],
  ["ipp_render_set_surface_cache_budget", rendering && instrumentation],
  ["ipp_profile_shadow_draw_calls", rendering && instrumentation],
  ["ipp_profile_reset", instrumentation],
  ["ipp_instrumentation_panic", instrumentation],
])
  assert.equal(
    typeof runtime[name] === "function",
    expected,
    `export ${name} differs from the distribution's axes`,
  );

// The distribution's configuration, not the runtime's exports, decides whether
// it ships the profiler and the worker side of the testing controls.
const shipped = async (name) =>
  readFile(resolve(directory, name), "utf8").then(
    () => true,
    () => false,
  );
assert.equal(await shipped("profile-worker.js"), instrumentation);
assert.equal(await shipped("render-testing.js"), rendering && instrumentation);
const workerSource = await readFile(
  resolve(directory, "wasm-worker.js"),
  "utf8",
);
assert.equal(workerSource.includes("IPP_INSTRUMENTATION"), false);
assert.equal(workerSource.includes("ipp_profile_reset"), false);
if (rendering) {
  const renderWorker = await readFile(
    resolve(directory, "render-worker.js"),
    "utf8",
  );
  for (const text of ["context-loss", "ipp_render_set_", "glyph-atlas-limits"])
    assert.equal(
      renderWorker.includes(text),
      false,
      `render worker has ${text}`,
    );
}

// The panic hook reports the message and source location through the log sink
// before the abort, from this stripped `panic = "abort"` runtime.
if (instrumentation) {
  const lines = [];
  const module = await WebAssembly.compile(runtimeBytes);
  const imports = {};
  let memory;
  for (const item of WebAssembly.Module.imports(module)) {
    imports[item.module] ??= {};
    imports[item.module][item.name] = () => {
      throw new Error("The panic check attempted host I/O");
    };
  }
  imports.ipp_diagnostics.write = (level, pointer, length) =>
    lines.push([
      level,
      new TextDecoder().decode(
        new Uint8Array(memory.buffer, pointer >>> 0, length >>> 0),
      ),
    ]);
  const panicking = (await WebAssembly.instantiate(module, imports)).exports;
  memory = panicking.memory;
  assert.equal(panicking.ipp_diagnostics_set_level(1), 1);
  assert.throws(
    () => panicking.ipp_instrumentation_panic(),
    WebAssembly.RuntimeError,
  );
  assert.equal(lines.length, 1, JSON.stringify(lines));
  assert.equal(lines[0][0], 1);
  assert.match(
    lines[0][1],
    /^\[session=0\] panic at crates\/ipp-wasm\/src\/diagnostics\.rs:\d+:\d+: forced instrumentation panic$/,
  );
  console.log(`Panic hook logged: ${lines[0][1]}`);
}

await writeFile(
  resolve(directory, "build-report.json"),
  `${JSON.stringify(
    {
      configuration,
      features,
      schemaHash: hash.toString(),
      components: Object.keys(client.components),
      exports: Object.keys(runtime),
      artifacts: await Promise.all(
        [
          ...new Set([
            runtimePath,
            ...hostArtifacts,
            sourcePath.replace(/\.ts$/, ".js"),
            ...CLIENT_SUPPORT_MODULES.map((name) =>
              resolve(directory, name.replace(/\.ts$/, ".js")),
            ),
          ]),
        ].map(artifact),
      ),
    },
    null,
    2,
  )}\n`,
);
console.log(`Verified the ${configuration} WASM runtime, client and host.`);
