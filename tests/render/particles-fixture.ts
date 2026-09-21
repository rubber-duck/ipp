/** Real generated-worker particle scenarios; launch/capture transport stays in the environment. */
import type {
  RenderWorldClient,
  FrameCapture,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  activateFixtureCamera,
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../integration/camera-fixtures.js";
let host: WorldPersistenceHostClient<RenderWorldClient>;
let client: RenderWorldClient;
let contract: any;
let effect: bigint;
const frames = new Map<string, FrameCapture>();

async function upload(kind: number, bytes: Uint8Array<ArrayBuffer>) {
  return (await client.createAsset(kind, bytes.buffer)).source;
}

export async function initialize(config: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  const canvas = document.createElement("canvas");
  canvas.id = "particles-canvas";
  canvas.width = 320;
  canvas.height = 240;
  document.body.replaceChildren(canvas);
  contract = await import(config.generatedModuleUrl);
  host = await contract.IppHostClient.connectWorker(
    config.workerScriptUrl,
    config.wasmUrl,
    { canvas: canvas.transferControlToOffscreen(), timeoutMs: 10000 },
  );
  client = await host.createWorld({ symbolicId: "particles" });
  const camera = await activateFixtureCamera(client);
  successfulBatch(
    await client.batch(
      componentFields(client, "Transform", {
        x: 0,
        y: 0,
        z: 6,
        qx: 0,
        qy: 0,
        qz: 0,
        qw: 1,
      }).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id: camera },
        component: client.components.Transform!.id,
        field,
      })),
    ),
  );
  const ref = { kind: "alias", alias: 1 } as const;
  const result = successfulBatch(
    await client.batch([
      createEntity(1, "effect"),
      insertComponent(client, "Transform", ref, {}),
      insertComponent(client, "ParticleEmitter", ref, {
        burst: 2000,
        rate: 0,
        speed: 0,
        lifetime: 10000,
        shape: 1,
        extent_x: 1.5,
        extent_y: 1,
        extent_z: 0,
        seed: 7,
        size: 0.15,
      }),
      insertComponent(client, "ParticleSprite", ref, {
        r: 0,
        g: 1,
        b: 0,
        end_opacity: 1,
      }),
    ]),
  );
  effect = aliasId(result, 1);
}

export async function update(
  component: string,
  values: Record<string, number | string | boolean>,
) {
  successfulBatch(
    await client.batch(
      componentFields(client, component, values).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id: effect },
        component: client.components[component]!.id,
        field,
      })),
    ),
  );
}

export async function capture(label: string, draws = 1) {
  const state = await client.inspect();
  const deadline = performance.now() + 10000;
  for (;;) {
    const frame = await client.presentation!.capture(state.tick);
    if (frame.drawCalls === draws) {
      frames.set(label, frame);
      const pixels = new Uint8Array(frame.pixels);
      let green = 0,
        red = 0,
        blue = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        if (pixels[i + 1]! > 100 && pixels[i + 1]! > pixels[i]! + 30) green++;
        if (pixels[i]! > 100 && pixels[i]! > pixels[i + 1]! + 30) red++;
        if (pixels[i + 2]! > 100 && pixels[i + 2]! > pixels[i]! + 30) blue++;
      }
      return {
        green,
        red,
        blue,
        draws: frame.drawCalls,
        center: [
          ...pixels.subarray(
            (120 * frame.width + 160) * 4,
            (120 * frame.width + 160) * 4 + 3,
          ),
        ],
      };
    }
    if (performance.now() > deadline)
      throw new Error(`Particle draws ${frame.drawCalls}, expected ${draws}`);
  }
}

export function captureDataUrl(label: string) {
  const f = frames.get(label)!;
  const canvas = document.createElement("canvas");
  canvas.width = f.width;
  canvas.height = f.height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(new Uint8ClampedArray(f.pixels), f.width, f.height),
      0,
      0,
    );
  return canvas.toDataURL();
}

export function equal(a: string, b: string) {
  const x = new Uint8Array(frames.get(a)!.pixels),
    y = new Uint8Array(frames.get(b)!.pixels);
  return x.every((v, i) => v === y[i]);
}

export async function recover() {
  client.presentation!.loseContext();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  client.presentation!.restoreContext();
}

export async function mesh(customVertex = false) {
  const bytes = new Uint8Array(16 + 4 * 32 + 12);
  const view = new DataView(bytes.buffer);
  bytes.set([73, 80, 80, 77]);
  [2, 4, 6].forEach((v, i) => view.setUint32(4 + i * 4, v, true));
  let offset = 16;
  for (const p of [
    [-0.5, -0.5, 0, 1, 1, 1, 0, 0],
    [0.5, -0.5, 0, 1, 1, 1, 1, 0],
    [0.5, 0.5, 0, 1, 1, 1, 1, 1],
    [-0.5, 0.5, 0, 1, 1, 1, 0, 1],
  ])
    for (const v of p) {
      view.setFloat32(offset, v, true);
      offset += 4;
    }
  [0, 1, 2, 0, 2, 3].forEach((v, i) => view.setUint16(offset + i * 2, v, true));
  const source = await upload(1, bytes);
  const shader = await upload(
    13,
    contract.encodeShaderDefinition({
      recipe: { backend: "glsl-es-300", instancing: true },
      parameters: {},
      backends: {
        "glsl-es-300": {
          vertex: customVertex
            ? "void materialVertex() { ippDefaultVertex(); }"
            : "",
          fragment: "vec4 materialFragment() { return vec4(0,0,1,1); }",
        },
      },
    }),
  );
  const ref = { kind: "handle", id: effect } as const;
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: ref,
        component: client.components.ParticleSprite!.id,
      },
      insertComponent(client, "ParticleMesh", ref, { source }),
      insertComponent(client, "CustomMaterial", ref, { source: shader }),
    ]),
  );
}

export async function playback(time: number) {
  const bytes = new Uint8Array(16 + 24 + 120);
  const v = new DataView(bytes.buffer);
  bytes.set([73, 80, 80, 67]);
  [1, 1, 2].forEach((n, i) => v.setUint32(4 + i * 4, n, true));
  for (let f = 0; f < 2; f++) {
    v.setFloat32(16 + 12 * f, f, true);
    v.setUint32(20 + 12 * f, 40 + 60 * f, true);
    v.setUint32(24 + 12 * f, 1, true);
    const at = 40 + 60 * f;
    v.setBigUint64(at, 10n, true);
    [0, 2, -1 + 2 * f, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1].forEach((n, i) =>
      v.setFloat32(at + 8 + i * 4, n, true),
    );
  }
  const source = await upload(15, bytes);
  const ref = { kind: "handle", id: effect } as const;
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: ref,
        component: client.components.ParticleEmitter!.id,
      },
      insertComponent(client, "ParticlePlayback", ref, { source, time }),
    ]),
  );
}

export async function close() {
  await host?.close();
  frames.clear();
  document.querySelector("#particles-canvas")?.remove();
}

export async function restoreLive() {
  await update("ParticleEmitter", { enabled: false });
  const bytes = await host.saveWorld();
  await host.detachWorld();
  client = await host.loadWorld(bytes, { symbolicId: "restored-particles" });
  const state = await client.inspect();
  effect = state.entities.find((e) => e.metadata.symbolicId === "effect")!.id;
  const camera = state.entities.find((e) =>
    e.effective.some((c) => c.component === client.components.Camera!.id),
  )!.id;
  client.sendCommand({ type: "CameraActivateCommand", entity: camera });
  return capture("loaded-empty", 0);
}

export async function customVertex() {
  const source = await upload(
    13,
    contract.encodeShaderDefinition({
      recipe: { instancing: true },
      parameters: {},
      backends: {
        "glsl-es-300": {
          vertex: "void materialVertex() { ippDefaultVertex(); }",
          fragment: "vec4 materialFragment() { return vec4(0,0,1,1); }",
        },
      },
    }),
  );
  await update("CustomMaterial", { source });
}

export async function removeProducer() {
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: effect },
        component: client.components.ParticlePlayback!.id,
      },
    ]),
  );
  return capture("producer-removed", 0);
}

export async function transparency() {
  const ref = { kind: "handle", id: effect } as const;
  const next = { kind: "alias", alias: 1 } as const;
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: ref,
        component: client.components.ParticleMesh!.id,
      },
      {
        kind: "removeComponent",
        entity: ref,
        component: client.components.CustomMaterial!.id,
      },
      insertComponent(client, "ParticleEmitter", ref, {
        burst: 1,
        rate: 0,
        speed: 0,
        lifetime: 10000,
        size: 1,
      }),
      insertComponent(client, "ParticleSprite", ref, {
        r: 1,
        g: 0,
        b: 0,
        opacity: 0.5,
        end_opacity: 0.5,
      }),
      createEntity(1, "far-particle"),
      insertComponent(client, "Transform", next, {}),
      insertComponent(client, "ParticleEmitter", next, {
        burst: 1,
        rate: 0,
        speed: 0,
        lifetime: 10000,
        size: 1,
      }),
      insertComponent(client, "ParticleSprite", next, {
        r: 0,
        g: 0,
        b: 1,
        opacity: 0.5,
        end_opacity: 0.5,
      }),
    ]),
  );
  await update("Transform", { z: 1 });
  return capture("alpha-order", 2);
}
