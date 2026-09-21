/** Backend-independent material scenarios driven through the generated worker client. */
import type {
  RenderWorldClient,
  AnimationWorldClient,
  FrameCapture,
  DynamicValue,
  WorldPersistenceHostClient,
  ShaderDefinition,
} from "@ipp/client";
import {
  activateFixtureCamera,
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import { poseMesh } from "./mesh-pose-assets.js";
import { compareImages } from "./image-assertions.js";

let client: RenderWorldClient & AnimationWorldClient;
let contract: any;
let host: WorldPersistenceHostClient<RenderWorldClient & AnimationWorldClient>;
const entities = new Map<string, bigint>();
const captures = new Map<string, FrameCapture>();
let editor:
  | Awaited<
      ReturnType<typeof import("../react/fixture.js").createShaderPreview>
    >
  | undefined;

export async function shaderEditor(body?: string, tint = [0, 0, 1, 1]) {
  if (!editor) {
    const module = "/target/react-build/fixture.js";
    editor = await (await import(module)).createShaderPreview(client);
  }
  if (body !== undefined) await editor!.update(body, tint);
}

export async function beginShaderReplacement(
  body: string,
  tint: readonly number[],
) {
  await editor!.beginUpdate(body, tint);
}

export async function finishShaderReplacement() {
  await editor!.completeUpdate();
}

export async function closeShaderEditor() {
  await editor?.close();
  editor = undefined;
}

export async function initialize(configuration: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  const canvas = document.createElement("canvas");
  canvas.id = "custom-materials-canvas";
  canvas.width = 320;
  canvas.height = 240;
  document.body.replaceChildren(canvas);
  contract = await import(configuration.generatedModuleUrl);
  host = await contract.IppHostClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    { canvas: canvas.transferControlToOffscreen(), timeoutMs: 10000 },
  );
  client = await host.createWorld({ symbolicId: "custom-materials" });
  const camera = await activateFixtureCamera(client);
  entities.set("camera", camera);
  await update("camera", "Transform", {
    x: 0,
    y: 0,
    z: 6,
    qx: 0,
    qy: 0,
    qz: 0,
    qw: 1,
  });
  const source = await shader({
    parameters: { tint: "vec4" },
    backends: {
      "glsl-es-300": { fragment: "vec4 materialFragment() { return p_tint; }" },
    },
  });
  // Uploaded fixture ownership, including the immutable shader, follows the same path as meshes.
  const meshBytes = new Uint8Array(
    await (await fetch("/target/gallery-build/cube.mesh")).arrayBuffer(),
  );
  const mesh = await upload(1, meshBytes);
  for (const [name, x, color] of [
    ["left", -0.9, [0, 1, 0, 1]],
    ["right", 0.9, [1, 0, 0, 1]],
  ] as const) {
    const ref = { kind: "alias", alias: 1 } as const;
    const result = successfulBatch(
      await client.batch([
        createEntity(1, name),
        insertComponent(client, "Transform", ref, {
          x,
          sx: 0.65,
          sy: 0.65,
          sz: 0.65,
        }),
        insertComponent(client, "MeshInstance", ref, { source: mesh }),
        insertComponent(client, "CustomMaterial", ref, { source }),
        insertComponent(client, "UnlitMaterial", ref, { r: 0, g: 0, b: 1 }),
        {
          kind: "setDynamicProperty",
          entity: ref,
          component: client.components.CustomMaterial!.id,
          name: "tint",
          value: { kind: "vec4", value: color },
        },
      ]),
    );
    entities.set(name, aliasId(result, 1));
  }
}

async function upload(kind: number, bytes: Uint8Array<ArrayBuffer>) {
  return (await client.createAsset(kind, bytes.buffer)).source;
}
export async function shader(definition: ShaderDefinition) {
  return upload(13, contract.encodeShaderDefinition(definition));
}
export async function select(definition: ShaderDefinition, name = "left") {
  await update(name, "CustomMaterial", { source: await shader(definition) });
}
export async function parameter(
  entity: string,
  name: string,
  value: DynamicValue | null,
) {
  const target = { kind: "handle", id: entities.get(entity)! } as const;
  successfulBatch(
    await client.batch([
      value === null
        ? {
            kind: "removeDynamicProperty",
            entity: target,
            component: client.components.CustomMaterial!.id,
            name,
          }
        : {
            kind: "setDynamicProperty",
            entity: target,
            component: client.components.CustomMaterial!.id,
            name,
            value,
          },
    ]),
  );
}
export async function update(
  name: string,
  component: string,
  values: Record<string, string | number | boolean>,
) {
  const entity = { kind: "handle", id: entities.get(name)! } as const;
  successfulBatch(
    await client.batch(
      componentFields(client, component, values).map((field) => ({
        kind: "setField",
        entity,
        component: client.components[component]!.id,
        field,
      })),
    ),
  );
}
export async function remove(name: string, component: string) {
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: entities.get(name)! },
        component: client.components[component]!.id,
      },
    ]),
  );
}
export async function capture(label: string, draws = 2) {
  const state = await client.inspect();
  const deadline = performance.now() + 10000;
  for (;;) {
    const frame = await client.presentation!.capture(state.tick);
    if (frame.drawCalls === draws) {
      captures.set(label, frame);
      const pixels = new Uint8Array(frame.pixels);
      const sample = (x: number, y: number) => [
        ...pixels.subarray(
          (y * frame.width + x) * 4,
          (y * frame.width + x) * 4 + 4,
        ),
      ];
      return {
        ...captureMetadata(label),
        left: sample(106, 120),
        right: sample(214, 120),
        center: sample(160, 120),
      };
    }
    if (performance.now() > deadline)
      throw new Error(`Expected two draws, got ${frame.drawCalls}`);
  }
}
export function darkRamp(label: string) {
  const frame = captures.get(label)!;
  const pixels = new Uint8Array(frame.pixels);
  return Array.from(
    { length: 34 },
    (_, i) => pixels[(120 * frame.width + 90 + i) * 4]!,
  );
}
export function captureMetadata(label: string) {
  const { pixels: _, ...metadata } = captures.get(label)!;
  return metadata;
}
export function captureDataUrl(label: string) {
  const frame = captures.get(label)!;
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(
        new Uint8ClampedArray(frame.pixels.slice(0)),
        frame.width,
        frame.height,
      ),
      0,
      0,
    );
  return canvas.toDataURL("image/png");
}
export function difference(a: string, b: string) {
  return compareImages(captures.get(a)!, captures.get(b)!);
}
export async function recoverContext() {
  client.presentation!.loseContext();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  client.presentation!.restoreContext();
}
export async function overlays() {
  const result = successfulBatch(
    await client.batch([
      { kind: "createStateOverlayOwner", alias: 1 },
      {
        kind: "attachEntityOverlayBinding",
        owner: { kind: "alias", alias: 1 },
        alias: 2,
        symbolicId: "left",
        mode: "bound",
      },
      {
        kind: "attachComponentStateOverlay",
        owner: { kind: "alias", alias: 1 },
        binding: { kind: "alias", alias: 2 },
        alias: 3,
        component: client.components.CustomMaterial!.id,
        mode: "bound",
        fields: [],
      },
      {
        kind: "updateDynamicComponentStateOverlay",
        owner: { kind: "alias", alias: 1 },
        overlay: { kind: "alias", alias: 3 },
        properties: { tint: { kind: "vec4", value: [0, 0, 1, 1] } },
        clear: [],
      },
    ]),
  );
  return result.stateOverlays[0]!.id.toString();
}
export async function releaseOverlay(owner: string) {
  successfulBatch(
    await client.batch([
      {
        kind: "releaseStateOverlayOwner",
        owner: { kind: "handle", id: BigInt(owner) },
      },
    ]),
  );
}
export async function inspect() {
  const state = await client.inspect();
  return JSON.parse(
    JSON.stringify(state, (_, value) =>
      typeof value === "bigint" ? value.toString() : value,
    ),
  );
}
export async function close() {
  await closeShaderEditor();
  await host?.close();
  entities.clear();
  captures.clear();
  document.querySelector("#custom-materials-canvas")?.remove();
}

export async function packing() {
  const values: Record<string, DynamicValue> = {
    scalar: { kind: "f32", value: 0.25 },
    signed: { kind: "i32", value: -7 },
    unsigned: { kind: "u32", value: 19 },
    enabled: { kind: "bool", value: true },
    pair: { kind: "vec2", value: [0.25, 0.75] },
    triple: { kind: "vec3", value: [1, 2, 3] },
    quad: { kind: "vec4", value: [4, 5, 6, 7] },
    small: { kind: "mat2", value: [1, 2, 3, 4] },
    medium: { kind: "mat3", value: [1, 2, 3, 4, 5, 6, 7, 8, 9] },
    large: {
      kind: "mat4",
      value: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    },
  };
  for (const [name, value] of Object.entries(values))
    await parameter("left", name, value);
  await select({
    parameters: Object.fromEntries(
      Object.entries(values).map(([name, value]) => [
        name,
        contract.shaderParameterKind(value),
      ]),
    ),
    backends: {
      "glsl-es-300": {
        fragment: `vec4 materialFragment() {
    bool valid = p_scalar == 0.25 && p_signed == -7 && p_unsigned == 19u && p_enabled && p_pair.y == 0.75 && p_triple.z == 3.0 && p_quad.w == 7.0 && p_small[1][0] == 3.0 && p_medium[2][1] == 8.0 && p_large[3][2] == 15.0;
    return valid ? vec4(0,1,0,1) : vec4(1,0,0,1);
  }`,
      },
    },
  });
}
export async function textures() {
  const texture = async (rgb: number[]) => {
    const bytes = new Uint8Array(20);
    bytes.set([73, 80, 80, 84]);
    const view = new DataView(bytes.buffer);
    [3, 1, 1].forEach((v, i) => view.setUint32(4 + i * 4, v, true));
    bytes.set([...rgb, 255], 16);
    return upload(2, bytes);
  };
  await parameter("left", "first", {
    kind: "asset",
    value: { kind: 2, source: await texture([255, 0, 0]) },
  });
  await parameter("left", "second", {
    kind: "asset",
    value: { kind: 2, source: await texture([0, 255, 0]) },
  });
  await select({
    parameters: { first: "texture2D", second: "texture2D" },
    backends: {
      "glsl-es-300": {
        fragment:
          "vec4 materialFragment() { return vec4(0.5 * (texture(p_first, v_uv).rgb + texture(p_second, v_uv).rgb), 1); }",
      },
    },
  });
}
/** Prove general asset transport independently of shader sampler interpretation. */
export async function assetBinding(
  mode: "unreferenced" | "required" | "texture",
) {
  const inspected = (await client.inspect()).entities.find(
    (e) => e.id === entities.get("left"),
  )!;
  const material = inspected.effective.find(
    (c) => c.component === client.components.CustomMaterial!.id,
  )!;
  const mesh = inspected.effective.find(
    (c) => c.component === client.components.MeshInstance!.id,
  )!;
  const reference: DynamicValue =
    mode === "texture"
      ? material.properties!.first!
      : {
          kind: "asset",
          value: { kind: 1, source: mesh.fields.source as string, variant: 0 },
        };
  await parameter("left", "input", reference);
  if (mode === "unreferenced")
    await parameter("left", "archivedMesh", reference);
  if (mode !== "unreferenced") {
    await select({
      parameters: { input: "texture2D" },
      backends: {
        "glsl-es-300": {
          fragment:
            "vec4 materialFragment() { return texture(p_input, v_uv); }",
        },
      },
    });
  }
  const observed = (await client.inspect()).entities
    .find((e) => e.id === entities.get("left"))!
    .effective.find(
      (c) => c.component === client.components.CustomMaterial!.id,
    )!.properties!.input!;
  return { reference, observed };
}

export async function animate() {
  const component = client.components.CustomMaterial!.id;
  const bytes = contract.encodeAnimationClip({
    duration: 2,
    tracks: [
      {
        property: { component, name: "tint" },
        keys: [
          {
            time: 0,
            value: {
              kind: "dynamic",
              value: { kind: "vec4", value: [0, 0, 0, 1] },
            },
            interpolation: { kind: "linear" },
          },
          {
            time: 2,
            value: {
              kind: "dynamic",
              value: { kind: "vec4", value: [0, 1, 0, 1] },
            },
            interpolation: { kind: "step" },
          },
        ],
      },
    ],
  });
  const source = await upload(10, bytes);
  const id = await client.createAnimationController({
    speed: 0,
    drivers: [
      {
        source,
        track: 0,
        target: entities.get("left")!,
        property: { component, name: "tint" },
      },
    ],
  });
  client.playback(id, { action: "play" });
  client.playback(id, { action: "seek", time: 1 });
  client.playback(id, { action: "pause" });
  await client.inspect();
  return id.toString();
}
export async function stopAnimation(id: string) {
  client.playback(BigInt(id), { action: "stop" });
  await client.inspect();
}
export async function ambient(value: number) {
  client.sendCommand({
    type: "RenderStateUpdateCommand",
    changes: { ambientLight: [value, value, value] },
  });
  await client.inspect();
}

export async function insert(
  name: string,
  component: string,
  values: Record<string, number | string | boolean>,
) {
  successfulBatch(
    await client.batch([
      insertComponent(
        client,
        component,
        { kind: "handle", id: entities.get(name)! },
        values,
      ),
    ]),
  );
}
export async function vertexFixture() {
  // IPPM v2 interleaves position, linear color and UV; the known color tests the ABI.
  const bytes = new Uint8Array(16 + 4 * 32 + 6 * 2),
    view = new DataView(bytes.buffer);
  bytes.set([73, 80, 80, 77]);
  [2, 4, 6].forEach((value, i) => view.setUint32(4 + i * 4, value, true));
  const corners = [
    [-1, -1, 0],
    [1, -1, 0],
    [1, 1, 0],
    [-1, 1, 0],
  ];
  let offset = 16;
  for (const [i, position] of corners.entries())
    for (const value of [
      ...position,
      0.25,
      0.5,
      0.75,
      i === 1 || i === 2 ? 1 : 0,
      i >= 2 ? 1 : 0,
    ]) {
      view.setFloat32(offset, value, true);
      offset += 4;
    }
  [0, 1, 2, 0, 2, 3].forEach((value, i) =>
    view.setUint16(offset + i * 2, value, true),
  );
  await update("left", "MeshInstance", { source: await upload(1, bytes) });
  await select({
    parameters: {},
    requiredAttributes: 3,
    backends: {
      "glsl-es-300": {
        fragment: "vec4 materialFragment() { return vec4(v_color, 1); }",
      },
    },
  });
}
export async function deformation() {
  await parameter("left", "displacement", { kind: "vec2", value: [0, 0] });
  await select({
    parameters: { displacement: "vec2" },
    backends: {
      "glsl-es-300": {
        vertex:
          "out vec3 authoredColor; void materialVertex() { ippDefaultVertex(); gl_Position.xy += p_displacement * gl_Position.w; authoredColor = a_color; }",
        fragment:
          "in vec3 authoredColor; vec4 materialFragment() { return vec4(authoredColor,1); }",
      },
    },
  });
}

export async function bounds() {
  await insert("left", "BoundingGeometry", {});
  await update("left", "Transform", { x: 100 });
  await select({
    parameters: {},
    backends: {
      "glsl-es-300": {
        vertex:
          "void materialVertex() { ippDefaultVertex(); gl_Position = vec4(-0.45 + a_position.x * 0.2, a_position.y * 0.2, 0, 1); }",
        fragment: "vec4 materialFragment() { return vec4(0,1,0,1); }",
      },
    },
  });
}

/** Compare standard pose/skinning helpers with an independently baked mesh. */
export async function poseAndSkin(enabled: boolean) {
  await select({
    recipe: { skinning: enabled, meshPose: enabled },
    parameters: { tint: "vec4" },
    backends: {
      "glsl-es-300": {
        vertex: "void materialVertex() { ippDefaultVertex(); }",
        fragment: "vec4 materialFragment() { return p_tint; }",
      },
    },
  });
  if (enabled) {
    const skeletonSource = await upload(
      contract.WIRE.ASSET_SKELETON,
      contract.encodeSkeletonAsset([{ parent: null }]),
    );
    const skinSource = await upload(
      contract.WIRE.ASSET_SKIN,
      contract.encodeSkinAsset([
        {
          joint: 0,
          inverseBind: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
        },
      ]),
    );
    const result = successfulBatch(
      await client.batch([
        createEntity(1, "custom-rig"),
        insertComponent(
          client,
          "Transform",
          { kind: "alias", alias: 1 },
          { x: -0.9, sx: 0.65, sy: 0.65, sz: 0.65 },
        ),
        insertComponent(
          client,
          "Skeleton",
          { kind: "alias", alias: 1 },
          {
            source: skeletonSource,
            joints: contract.encodeJointOverrides([
              { joint: 0, rotation: [0, 0, Math.sin(0.2), Math.cos(0.2)] },
            ]),
          },
        ),
      ]),
    );
    const skeleton = aliasId(result, 1);
    await update("left", "MeshInstance", {
      source: await upload(1, poseMesh(0, { skin: true })),
    });
    await insert("left", "MeshPose", {
      source: await upload(1, poseMesh(1)),
      weight: 0.5,
    });
    successfulBatch(
      await client.batch([
        insertComponent(
          client,
          "Skin",
          { kind: "handle", id: entities.get("left")! },
          { source: skinSource, skeleton },
        ),
      ]),
    );
  } else {
    await remove("left", "Skin");
    await remove("left", "MeshPose");
    await update("left", "MeshInstance", {
      source: await upload(1, poseMesh(0.5, { joint: true })),
    });
  }
}

export async function textureAnimation() {
  await parameter("left", "gain", { kind: "f32", value: 1 });
  await select({
    parameters: { first: "texture2D", second: "texture2D", gain: "f32" },
    backends: {
      "glsl-es-300": {
        fragment:
          "vec4 materialFragment() { return vec4(p_gain * 0.5 * (texture(p_first, v_uv).rgb + texture(p_second, v_uv).rgb), 1); }",
      },
    },
  });
  const snapshot = (await client.inspect()).entities.find(
    (e) => e.id === entities.get("left"),
  )!;
  const properties = snapshot.effective.find(
    (c) => c.component === client.components.CustomMaterial!.id,
  )!.properties!;
  const component = client.components.CustomMaterial!.id;
  const source = await upload(
    10,
    contract.encodeAnimationClip({
      duration: 2,
      tracks: [
        {
          property: { component, name: "first" },
          keys: [
            {
              time: 0,
              value: { kind: "dynamic", value: properties.first },
              interpolation: { kind: "step" },
            },
            {
              time: 1,
              value: { kind: "dynamic", value: properties.second },
              interpolation: { kind: "step" },
            },
          ],
        },
        {
          property: { component, name: "gain" },
          keys: [
            {
              time: 0,
              value: { kind: "dynamic", value: { kind: "f32", value: 1 } },
              interpolation: { kind: "linear" },
            },
            {
              time: 1,
              value: { kind: "dynamic", value: { kind: "f32", value: 0.5 } },
              interpolation: { kind: "step" },
            },
          ],
        },
        {
          property: {
            component: client.components.Transform!.id,
            offsets: [client.components.Transform!.fields.sx!.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: 0.65 },
              interpolation: { kind: "linear" },
            },
            {
              time: 1,
              value: { kind: "f32", value: 0.55 },
              interpolation: { kind: "step" },
            },
          ],
        },
      ],
    }),
  );
  const id = await client.createAnimationController({
    speed: 0,
    drivers: [
      {
        source,
        track: 0,
        target: entities.get("left")!,
        property: { component, name: "first" },
      },
      {
        source,
        track: 1,
        target: entities.get("left")!,
        property: { component, name: "gain" },
      },
      {
        source,
        track: 2,
        target: entities.get("left")!,
        property: {
          component: client.components.Transform!.id,
          offsets: [client.components.Transform!.fields.sx!.offset],
        },
      },
    ],
  });
  client.playback(id, { action: "play" });
  client.playback(id, { action: "seek", time: 1 });
  await client.inspect();
  return id.toString();
}

export async function seekAnimation(id: string, time: number) {
  client.playback(BigInt(id), { action: "seek", time });
  const snapshot = (await client.inspect()).entities.find(
    (e) => e.id === entities.get("left"),
  )!;
  return snapshot.effective.find(
    (c) => c.component === client.components.Transform!.id,
  )!.fields.sx;
}

export async function deviceLimit() {
  const state = (await client.inspect()).entities.find(
    (e) => e.id === entities.get("left"),
  )!;
  const texture = state.effective.find(
    (c) => c.component === client.components.CustomMaterial!.id,
  )!.properties!.first!;
  const parameters: Record<string, "texture2D"> = {};
  for (let i = 0; i < 64; i++) {
    parameters[`limit${i}`] = "texture2D";
    await parameter("left", `limit${i}`, texture);
  }
  // Samplers are optimized out, so the explicit resource-unit validation must reject this candidate.
  await select({
    parameters,
    backends: {
      "glsl-es-300": {
        fragment: "vec4 materialFragment() { return vec4(0,1,0,1); }",
      },
    },
  });
}

export async function saveWithoutShader() {
  await update("left", "CustomMaterial", {
    source: "file:///unavailable.shader",
  });
  const inputs = (await client.inspect()).entities
    .find((e) => e.id === entities.get("left"))!
    .effective.find(
      (c) => c.component === client.components.CustomMaterial!.id,
    )!.properties!;
  for (const [name, value] of Object.entries(inputs)) {
    if (value.kind === "asset")
      await parameter("left", name, {
        kind: "asset",
        value: {
          kind: value.value.kind,
          source: value.value.source.replace(
            "client://",
            "file:///unavailable/assets/",
          ),
          variant: 7,
        },
      });
  }
  const before = (await client.inspect()).entities.find(
    (e) => e.id === entities.get("left"),
  )!;
  const expected = before.effective.find(
    (c) => c.component === client.components.CustomMaterial!.id,
  )!.properties!;
  const module = "/target/react-build/fixture.js";
  const { createPendingAsset } = (await import(
    module
  )) as typeof import("../react/fixture.js");
  const oldAsset = await createPendingAsset(client);
  const oldSession = client.session;
  await oldAsset.close();
  const bytes = await host.saveWorld();
  await host.detachWorld();
  client = await host.loadWorld(bytes, {
    symbolicId: "restored-custom-materials",
  });
  const state = await client.inspect();
  const entity = state.entities.find(
    (e) => e.metadata.symbolicId === "left",
  )!.id;
  const after = (await client.inspect()).entities.find((e) => e.id === entity)!;
  const restored = after.effective.find(
    (c) => c.component === client.components.CustomMaterial!.id,
  )!.properties!;
  const nextAsset = await createPendingAsset(client);
  let assetSessionIsolated: boolean;
  try {
    oldAsset.release();
    nextAsset.release();
    await client.waitForFrame();
    assetSessionIsolated =
      client.session !== oldSession &&
      oldAsset.state() === undefined &&
      nextAsset.state()?.current?.source === nextAsset.source &&
      nextAsset.source !== oldAsset.source &&
      !(await client.inspect()).resources.some(
        (resource) => resource.source === oldAsset.source,
      );
  } finally {
    await nextAsset.close();
  }
  return { expected, restored, bytes: bytes.length, assetSessionIsolated };
}

export async function shadowScene() {
  const aim = (x: number, y: number, z: number) => {
    const yaw = Math.atan2(x, z) / 2,
      pitch = -Math.atan2(y, Math.hypot(x, z)) / 2;
    return {
      qx: Math.sin(pitch) * Math.cos(yaw),
      qy: Math.cos(pitch) * Math.sin(yaw),
      qz: -Math.sin(pitch) * Math.sin(yaw),
      qw: Math.cos(pitch) * Math.cos(yaw),
    };
  };
  await update("camera", "Transform", { x: 4, y: 5, z: 7, ...aim(4, 5, 7) });
  await update("camera", "Camera", { projection: 1, ortho_height: 6 });
  const mesh = await upload(
    1,
    new Uint8Array(
      await (await fetch("/target/gallery-build/cube.mesh")).arrayBuffer(),
    ),
  );
  await update("left", "MeshInstance", { source: mesh });
  await update("left", "Transform", {
    x: 0,
    y: 0.65,
    z: 0,
    sx: 0.65,
    sy: 0.65,
    sz: 0.65,
  });
  await update("right", "Transform", {
    x: 0,
    y: -0.05,
    z: 0,
    sx: 3.5,
    sy: 0.05,
    sz: 3,
  });
  await remove("right", "CustomMaterial");
  await insert("right", "PbrMaterial", {
    r: 0.65,
    g: 0.65,
    b: 0.65,
    roughness: 0.9,
    cast_shadows: false,
  });
  await select({
    recipe: { shadowPass: true },
    parameters: { coverage: "f32" },
    backends: {
      "glsl-es-300": {
        vertex: "void materialVertex() { ippDefaultVertex(); }",
        fragment: "vec4 materialFragment() { return vec4(0,1,0,p_coverage); }",
      },
    },
  });
  await parameter("left", "coverage", { kind: "f32", value: 1 });
  await update("left", "CustomMaterial", {
    alpha_mode: 1,
    casts_shadows: true,
  });
  successfulBatch(
    await client.batch([
      createEntity(1, "custom-spot"),
      insertComponent(
        client,
        "Transform",
        { kind: "alias", alias: 1 },
        { x: -2, y: 4, z: 2, ...aim(-2, 4, 2) },
      ),
      insertComponent(
        client,
        "Light",
        { kind: "alias", alias: 1 },
        {
          kind: 2,
          intensity: 55,
          range: 15,
          inner_cone: 0.45,
          outer_cone: 0.85,
          cast_shadows: true,
        },
      ),
    ]),
  );
}

/** Independent point expectations and explicit per-channel differences for failure artifacts. */
export function sampleEvidence(actual: number[], expected: number[]) {
  const png = (rgba: number[]) => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 32;
    const context = canvas.getContext("2d")!;
    context.fillStyle = `rgb(${rgba[0]} ${rgba[1]} ${rgba[2]})`;
    context.fillRect(0, 0, 32, 32);
    return canvas.toDataURL("image/png");
  };
  return {
    actual: png(actual),
    expected: png(expected),
    difference: png(expected.map((v, i) => Math.abs(v - actual[i]!))),
  };
}

export function differenceDataUrl(a: string, b: string) {
  const first = captures.get(a)!,
    second = captures.get(b)!;
  const bytes = new Uint8ClampedArray(first.pixels.slice(0)),
    reference = new Uint8Array(second.pixels);
  for (let i = 0; i < bytes.length; i++)
    bytes[i] = i % 4 === 3 ? 255 : Math.abs(bytes[i]! - reference[i]!);
  const canvas = document.createElement("canvas");
  canvas.width = first.width;
  canvas.height = first.height;
  canvas
    .getContext("2d")!
    .putImageData(new ImageData(bytes, first.width, first.height), 0, 0);
  return canvas.toDataURL("image/png");
}

export function shadowPixels(lit: string, shadowed: string) {
  const a = new Uint8Array(captures.get(lit)!.pixels),
    b = new Uint8Array(captures.get(shadowed)!.pixels);
  let count = 0;
  for (let i = 0; i < a.length; i += 4)
    if ([0, 1, 2].every((c) => a[i + c]! > b[i + c]! + 8)) count++;
  return count;
}

export async function shadowReceiver() {
  await parameter("left", "coverage", { kind: "f32", value: 1 });
  await update("left", "CustomMaterial", { casts_shadows: true });
  await insert("right", "CustomMaterial", {
    source: await shader({
      recipe: { lighting: true },
      parameters: {},
      backends: {
        "glsl-es-300": {
          fragment: `vec4 materialFragment() {
    vec3 n = ippSurfaceNormal();
    vec3 color = 0.3 * u_ambient;
    for (int i = 0; i < u_light_count; ++i) {
      float nl = max(dot(n, ippLightDirection(i, v_position)), 0.0);
      color += 0.2 * ippLightRadiance(i, v_position) * nl * ippShadowVisibility(i, nl);
    }
    return vec4(color, 1);
  }`,
        },
      },
    }),
    receives_light: true,
    receives_shadows: true,
  });
}

export async function flatPose(enabled: boolean) {
  await select({
    recipe: { meshPose: enabled, lighting: true },
    parameters: {},
    backends: {
      "glsl-es-300": {
        fragment:
          "vec4 materialFragment() { return vec4(abs(ippSurfaceNormal()), 1); }",
      },
    },
  });
  if (enabled) {
    await update("left", "MeshInstance", {
      source: await upload(1, poseMesh(0)),
    });
    await insert("left", "MeshPose", {
      source: await upload(1, poseMesh(1, { normals: false })),
      weight: 0.5,
    });
    await update("left", "CustomMaterial", { receives_light: true });
  } else {
    await remove("left", "MeshPose");
    await update("left", "MeshInstance", {
      source: await upload(1, poseMesh(0.5, { normals: false })),
    });
  }
}
