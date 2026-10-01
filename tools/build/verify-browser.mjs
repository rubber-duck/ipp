/** Assemble clients from actual WASM exports and verify the final lean hosts. */
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  assembleBrowserHost,
  CLIENT_SUPPORT_MODULES,
} from "../../packages/ipp-client/tools/assemble.mjs";

import { artifact, bundleBrowser } from "./helpers.mjs";
import { instantiate } from "./wasm.mjs";

const root = resolve(import.meta.dirname, "../..");
/**
 * Whether RenderService calls the whole-Surface cache device methods, so the
 * final runtime imports the cache bridge and embeds surface_cache.frag. Set
 * when the ipp-s1ge.2.3 service cache is integrated.
 */
const SURFACE_CACHE_LINKED = true;
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
const { configuration, features, builtins, directory } = request;
const reports = [];
{
  const rendering = features.includes("render");
  const particles = features.includes("particles");
  const surfaces = features.includes("surfaces");
  const gui = features.includes("gui");
  const meshPoses = features.includes("mesh-poses");
  const shadows = features.includes("shadows");
  const skeletalAnimation = features.includes("skeletal-animation");
  await mkdir(directory, { recursive: true });
  const exportPath = resolve(directory, "export.wasm");
  const runtimePath = resolve(directory, "runtime.wasm");
  const contractPath = resolve(directory, "contract.bin");
  const sourcePath = resolve(directory, "generated.ts");

  // Exercise ordinary built-in sources and optional private debug rendering.
  const exported = await instantiate(exportPath);
  const hash = BigInt.asUintN(64, exported.ipp_schema_hash());
  // Export initialization may allocate and detach the old memory buffer.
  const contractPointer = exported.ipp_contract_ptr() >>> 0;
  const contractLength = exported.ipp_contract_len() >>> 0;
  const bytes = new Uint8Array(
    exported.memory.buffer,
    contractPointer,
    contractLength,
  ).slice();
  assert.deepEqual(bytes, new Uint8Array(await readFile(contractPath)));
  const client = await import(
    pathToFileURL(resolve(directory, "generated.js")).href
  );
  assert.equal(client.SCHEMA_HASH, hash);
  assert.equal(client.CAPABILITIES.snapshot, true);
  for (const method of ["saveWorld", "loadWorld"])
    assert.equal(
      method in client.IppHostClient.prototype,
      true,
      `${method} availability differs from the snapshot capability`,
    );
  assert.equal(client.CAPABILITIES.skeletalAnimation, skeletalAnimation);
  assert.equal(client.CAPABILITIES.meshPoses, meshPoses);
  assert.equal(!!client.components.MeshPose, meshPoses);
  assert.equal(
    typeof client.encodeSkeletonAsset === "function",
    skeletalAnimation,
  );
  assert.equal(
    typeof client.encodeSkinnedMesh === "function",
    skeletalAnimation,
  );
  const source = await readFile(sourcePath, "utf8");
  assert.equal(
    source.includes('case "setFieldIf"'),
    true,
    "baseline command codecs are missing",
  );
  assert.equal(client.CAPABILITIES.spatial, true);
  assert.equal(client.CAPABILITIES.textures, true);
  assert.equal(client.CAPABILITIES.builtinAssets, builtins);
  assert.equal(client.CAPABILITIES.picking, true);
  assert.equal(client.CAPABILITIES.pbr, true);
  assert.equal(client.CAPABILITIES.shadows, shadows);
  assert.equal(client.CAPABILITIES.surfaces, surfaces);
  assert.equal(!!client.components.Surface, surfaces);
  assert.equal(typeof client.encodeSurfaceItems, "undefined");
  assert.equal(typeof client.encodeSurfaceEdit, "undefined");
  assert.equal("editSurface" in client.IppClient.prototype, false);
  assert.equal(client.CAPABILITIES.gui, gui);
  assert.equal(client.components.GuiRoot, undefined);
  assert.equal(typeof client.encodeGuiEdits, "undefined");
  for (const method of [
    "editGui",
    "editGuiBatch",
    "editGuiBatchChunk",
    "submitGuiInput",
    "semanticSnapshot",
    "createGuiNodeHandle",
    "guiSnapshot",
    "guiSnapshotPage",
    "replaceControl",
  ])
    assert.equal(method in client.IppClient.prototype, false);
  assert.equal("semanticAction" in client.IppClient.prototype, false);
  assert.equal("subscribeGuiEffects" in client.IppClient.prototype, gui);
  assert.equal("guiAction" in client.Entity, gui);
  assert.equal(!!client.components.Light, true);
  assert.equal(!!client.components.PbrMaterial, true);
  assert.equal(client.CAPABILITIES.debugGeometry, true);
  assert.equal("onRenderStateUpdated" in client.IppClient.prototype, true);
  assert.equal(source.includes("ASSET_TEXTURE"), true);
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
    assert.equal(
      source.includes(name),
      false,
      `${name} leaked into the client`,
    );
    assert.equal(manifestSource.includes(`export const ${name} =`), true);
  }
  assert.equal(source.includes("generated-manifest"), false);
  assert.equal(Number.isSafeInteger(client.MAX_MESSAGE_BYTES), true);
  for (const method of [
    "uploadMesh",
    "uploadTexture",
    "loadBuiltinMesh",
    "loadBuiltinTexture",
  ]) {
    assert.equal(
      method in client.IppClient.prototype,
      false,
      `${method} leaked into the scene client`,
    );
  }
  for (const method of ["registerAsset", "onResourceChange"]) {
    assert.equal(
      method in client.IppClient.prototype,
      true,
      `${method} availability differs from the scene capability`,
    );
  }
  const hostArtifacts = await assembleBrowserHost(directory, {
    rendering,
    shadows,
    skeletalAnimation,
    meshPoses,
    particles,
    surfaces,
    gui,
  });
  if (rendering) {
    const bridge = await readFile(resolve(directory, "webgl.js"), "utf8");
    assert.equal(
      bridge.includes("IPP_GUI"),
      false,
      "GUI capability leaked into the bridge as an unresolved global",
    );
    assert.equal(
      bridge.includes("draw_surface_path"),
      surfaces,
      "Surface bridge dispatch differs from selected capability",
    );
    for (const name of [
      ...SURFACE_CACHE_BRIDGE_IMPORTS,
      "surfaceCacheTargetsLive",
    ])
      assert.equal(
        bridge.includes(name),
        surfaces,
        `Surface cache bridge dispatch ${name} differs from selected capability`,
      );
    for (const name of [
      "draw_gui_batch",
      "create_gui_batch",
      "write_gui_batch",
      "create_glyph_atlas_page",
    ])
      assert.equal(
        bridge.includes(name),
        surfaces,
        `Canvas bridge dispatch ${name} differs from selected capability`,
      );
    assert.equal(
      bridge.includes("set_lighting"),
      true,
      "baseline lighting bindings are missing",
    );
    assert.equal(
      bridge.includes("create_shadow_map"),
      shadows,
      "shadow bindings leaked into lean bridge",
    );
    assert.equal(
      bridge.includes("create_texture"),
      true,
      "texture bridge dispatch differs from selected capability",
    );
  }
  assert.equal(typeof client.IppClient, "function");
  assert.equal(typeof client.IppClient.connectWebSocket, "function");
  assert.equal("connect" in client.IppClient, false);
  assert.deepEqual(Object.keys(client.components), [
    "Scalar",
    "LinearDriver",
    "Transform",
    "UnlitMaterial",
    "MeshInstance",
    "UnlitTexture",
    "Camera",
    "PbrMaterial",
    "Light",
    ...(skeletalAnimation ? ["Skeleton", "Skin"] : []),
    "BoundingGeometry",
    "PickingGeometry",
    ...(meshPoses ? ["MeshPose"] : []),
    ...(skeletalAnimation ? ["ParentJoint"] : []),
    "LookAt",
    "BaseColorTexture",
    "CustomMaterial",
    ...(particles
      ? [
          "ParticleEmitter",
          "ParticlePlayback",
          "ParticleSprite",
          "ParticleMesh",
        ]
      : []),
    ...(surfaces ? ["Surface"] : []),
    ...(surfaces ? ["SurfaceCache"] : []),
    "WorldAttachment",
    ...(surfaces
      ? [
          "CanvasStyle",
          "CanvasText",
          "CanvasGlyphRun",
          "CanvasDrawing",
          "CanvasBitmap",
          "CanvasBox",
        ]
      : []),
    ...(gui
      ? [
          "GuiBehavior",
          "GuiButton",
          "GuiCheckbox",
          "GuiSlider",
          "GuiTextInput",
          "GuiLayout",
          "GuiTheme",
          "GuiSkin",
          "GuiFont",
          "GuiThemeMotion",
          "GuiScrollView",
          "GuiVirtualList",
          "GuiVirtualItem",
          "CanvasBounds",
        ]
      : []),
  ]);
  assert.equal(client.components.UnlitTexture.id, 6);
  assert.equal(client.components.BaseColorTexture.id, 19);
  assert.deepEqual(Object.keys(client.TARGET.features), [
    "builtin-assets",
    "shadows",
    "mesh-poses",
    "skeletal-animation",
    "particles",
    "surfaces",
    "gui",
  ]);

  const runtime = await instantiate(runtimePath);
  assert.equal(
    BigInt.asUintN(64, runtime.ipp_schema_hash()),
    hash,
    "final runtime differs from its export",
  );
  assert.equal(
    runtime.ipp_contract_ptr,
    undefined,
    "schema export leaked into runtime",
  );
  assert.equal(
    runtime.ipp_fixture_ptr,
    undefined,
    "fixture leaked into runtime",
  );
  assert.equal(
    runtime.ipp_fixture_check,
    undefined,
    "fixture code leaked into runtime",
  );
  const runtimeBytes = await readFile(runtimePath);
  assert.equal(
    runtimeBytes.includes(Buffer.from("u_lights[IPP_MAX_LIGHTS")),
    rendering,
    "lit shader inclusion differs from PBR selection",
  );
  assert.equal(
    runtimeBytes.includes(Buffer.from("u_shadow_settings")),
    shadows,
    "shadow shader leaked into a lean host",
  );
  for (const recipe of [
    "ipp://mesh/plane?",
    "ipp://mesh/plane-outline?",
    "ipp://mesh/sphere?",
    "ipp://mesh/pill?",
    "ipp://mesh/cube-outline?",
    "ipp://mesh/sphere-outline?",
    "ipp://mesh/pill-outline?",
    "ipp://texture/uv-grid?",
  ]) {
    assert.equal(
      runtimeBytes.includes(Buffer.from(recipe)),
      builtins,
      `${recipe} provider inclusion differs from builtin-assets selection`,
    );
  }
  const runtimeModule = await WebAssembly.compile(runtimeBytes);
  const glImports = WebAssembly.Module.imports(runtimeModule);
  assert.equal(
    glImports.some((entry) => entry.module === "ipp_gl"),
    rendering,
    "GL imports leaked into a headless build",
  );
  assert.equal(
    glImports.some((entry) => entry.module === "ipp_presentation"),
    rendering,
  );
  assert.equal(
    glImports.some((entry) => entry.module === "ipp_diagnostics"),
    features.includes("diagnostics"),
    "diagnostic imports differ from selected capability",
  );
  assert.equal(
    typeof runtime.ipp_diagnostics_set_level === "function",
    features.includes("diagnostics"),
    "diagnostic configuration leaked into a lean build",
  );
  assert.equal(
    runtimeBytes.includes(Buffer.from("batch.begin")),
    features.includes("diagnostics"),
    "diagnostic strings differ from selected capability",
  );
  assert.equal(
    runtimeBytes.includes(Buffer.from("#version 300 es")),
    rendering,
    "shader inclusion differs from render capability",
  );
  assert.equal(typeof runtime.ipp_render_attach === "function", rendering);
  assert.equal(typeof runtime.ipp_resource_poll === "function", true);
  assert.equal(typeof runtime.ipp_resource_complete === "function", true);
  assert.equal(typeof runtime.ipp_resource_input_reserve === "function", true);
  for (const name of [
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
  ]) {
    assert.equal(
      typeof runtime[name],
      "function",
      `missing runtime export ${name}`,
    );
  }
  assert.equal(
    glImports.some((entry) => entry.name.includes("create_texture")),
    rendering,
    "texture imports differ from selected capability",
  );
  assert.equal(
    runtimeBytes.includes(Buffer.from("sampler2D")),
    rendering,
    "texture shader leaked into a build without textures",
  );
  assert.equal(
    runtimeBytes.includes(Buffer.from("v_surface_position")),
    rendering && surfaces,
    "Surface shaders differ from selected capability",
  );
  assert.equal(
    glImports.some((entry) => entry.name === "create_gui_batch"),
    rendering && surfaces,
    "Canvas batch imports differ from selected capability",
  );
  // Statistics, records and testing overrides are diagnostics exports; the
  // frame summary and viewport limits are always present in render builds.
  const diagnostics = features.includes("diagnostics");
  for (const [name, expected] of [
    ["ipp_render_tick", false],
    ["ipp_render_resize", false],
    ["ipp_render_draw_calls", false],
    ["ipp_render_triangles", false],
    ["ipp_render_failed_draw_calls", false],
    ["ipp_render_invalid_camera", false],
    ["ipp_render_detach", rendering],
    ["ipp_render_max_viewport_width", rendering],
    ["ipp_render_max_viewport_height", rendering],
    ["ipp_render_statistics_ptr", rendering && diagnostics],
    ["ipp_render_statistics_len", rendering && diagnostics],
    ["ipp_render_set_exhaustive_draw_checks", rendering && diagnostics],
    ["ipp_render_set_glyph_atlas_limits", rendering && gui && diagnostics],
    [
      "ipp_render_surface_cache_records_ptr",
      rendering && surfaces && diagnostics,
    ],
    [
      "ipp_render_surface_cache_records_len",
      rendering && surfaces && diagnostics,
    ],
    [
      "ipp_render_set_surface_cache_budget",
      rendering && surfaces && diagnostics,
    ],
    ["ipp_resource_buffered_bytes", diagnostics],
    ["ipp_render_uploaded_bytes", false],
    ["ipp_render_glyph_pages", false],
    ["ipp_render_surface_cache_repaints", false],
  ])
    assert.equal(
      typeof runtime[name] === "function",
      expected,
      `render export ${name} differs from the selected capabilities`,
    );
  // The linker keeps the cache bridge imports and the composite shader only
  // once RenderService repaints and composites cached Surfaces (ipp-s1ge.2.3).
  if (SURFACE_CACHE_LINKED) {
    for (const name of SURFACE_CACHE_BRIDGE_IMPORTS)
      assert.equal(
        glImports.some((entry) => entry.name === name),
        rendering && surfaces,
        `Surface cache import ${name} differs from selected capability`,
      );
    assert.equal(
      runtimeBytes.includes(Buffer.from("u_surface_cache")),
      rendering && surfaces,
      "surface_cache.frag inclusion differs from selected capability",
    );
  }
  assert.equal(
    glImports.some((entry) => entry.name === "set_skin_palette"),
    rendering && skeletalAnimation,
  );
  assert.equal(
    runtimeBytes.includes(Buffer.from("u_joints")),
    rendering && skeletalAnimation,
  );
  assert.equal(
    glImports.some((entry) => entry.name === "draw_pose"),
    rendering && meshPoses,
  );
  assert.equal(runtimeBytes.includes(Buffer.from("u_pose_weight")), meshPoses);
  if (rendering) {
    const bridge = await readFile(resolve(directory, "webgl.js"), "utf8");
    assert.equal(bridge.includes("draw_pose"), meshPoses);
  }
  reports.push({
    configuration,
    defaultFeatures: builtins,
    features,
    schemaHash: hash.toString(),
    components: Object.keys(client.components),
    exports: Object.keys(runtime),
    artifacts: await Promise.all(
      [
        ...new Set([
          runtimePath,
          exportPath,
          ...hostArtifacts,
          sourcePath.replace(/\.ts$/, ".js"),
          ...CLIENT_SUPPORT_MODULES.map((name) =>
            resolve(directory, name.replace(/\.ts$/, ".js")),
          ),
        ]),
      ].map(artifact),
    ),
  });
}
await writeFile(
  resolve(directory, "build-report.json"),
  `${JSON.stringify(reports[0], null, 2)}\n`,
);
console.log(`Verified matching ${configuration} WASM client and final host.`);
