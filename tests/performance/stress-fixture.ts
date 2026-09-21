/** Standard saved Blender World; controls remain ordinary generated-client requests. */
import type {
  AnimationDriverDescription,
  Command,
  FrameCapture,
  WorldPersistenceHostClient,
} from "@ipp/client";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";
import type { BlenderDiskManifest } from "../../integrations/blender/client/disk-import.js";
import { check } from "../integration/animation-fixtures.js";
import { addCullingBounds } from "./culling.js";
import {
  addStressFeatures,
  type FeatureContract,
  type StressFixture,
} from "./features.js";
import { checkStressFeatures } from "./feature-checks.js";

let host: WorldPersistenceHostClient<BlenderClient>;
let client: BlenderClient;
let manifest: BlenderDiskManifest;
let controllers: bigint[] = [];
let entities = new Map<string, bigint>();
let nativeEmitters: bigint[] = [];
let cullingTargets: bigint[] = [];
let restart = 0;
let bundle: string;
let contract: FeatureContract;
let fixture: StressFixture;
const captures = new Map<string, FrameCapture>();

export async function open(
  urls: { generated: string; workerScript: string; wasm: string },
  base: string,
) {
  bundle = base;
  const module = await import(urls.generated);
  contract = module;
  const canvas = document.createElement("canvas");
  canvas.width = 800;
  canvas.height = 600;
  document.body.replaceChildren(canvas);
  host = await module.IppHostClient.connectWorker(
    urls.workerScript,
    urls.wasm,
    {
      canvas: canvas.transferControlToOffscreen(),
      timeoutMs: 60000,
      resourceUrls: [{ prefix: "https://stress.ipp.invalid/", baseUrl: base }],
      logLevel: "error",
    },
  );
}

async function batches<T>(
  values: readonly T[],
  action: (value: T) => Promise<unknown>,
) {
  for (let offset = 0; offset < values.length; offset += 32)
    await Promise.all(values.slice(offset, offset + 32).map(action));
}

export async function load(
  groupSize: number,
  configuration: StressFixture,
  persistence = false,
) {
  fixture = configuration;
  const started = performance.now();
  let previous = started;
  const stages: Record<string, number> = {};
  const mark = (name: string) => {
    const now = performance.now();
    stages[name] = now - previous;
    previous = now;
  };
  manifest = await (await fetch(bundle + "manifest.json")).json();
  const bytes = new Uint8Array(
    await (await fetch(bundle + "benchmark.ipp")).arrayBuffer(),
  );
  mark("fetch");
  client = await host.loadWorld(bytes);
  mark("loadWorld");
  let savedBytes: number | undefined;
  if (persistence) {
    const saved = await host.saveWorld();
    savedBytes = saved.length;
    mark("saveWorld");
    const world = client.world!.id;
    await host.detachWorld();
    await host.destroyWorld(world);
    mark("detachAndDestroy");
    client = await host.loadWorld(saved);
    mark("reloadWorld");
  }
  check(
    (await client.inspect()).controllers!.length === 0,
    "clips-only World unexpectedly has controllers",
  );
  mark("inspectImported");
  const features = await addStressFeatures(
    client,
    contract,
    fixture,
    async (kind, bytes) => {
      return (await client.createAsset(kind, bytes.buffer)).source;
    },
  );
  mark("addFeatures");
  const state = await client.inspect();
  mark("inspectFeatures");
  cullingTargets = state.entities
    .filter((entity) => {
      const components = entity.base.map((value) => value.component);
      return (
        components.includes(client.components.MeshInstance!.id) &&
        !components.includes(client.components.BoundingGeometry!.id) &&
        !components.includes(client.components.ParticleEmitter!.id) &&
        !components.includes(client.components.ParticlePlayback!.id)
      );
    })
    .map((entity) => entity.id);
  entities = new Map(
    state.entities
      .filter((entity) => entity.metadata.symbolicId !== null)
      .map((entity) => [entity.metadata.symbolicId!, entity.id]),
  );
  nativeEmitters = state.entities
    .filter((entity) =>
      entity.base.some(
        (value) => value.component === client.components.ParticleEmitter!.id,
      ),
    )
    .map((entity) => entity.id);
  check(
    [...entities.keys()].filter((name) => name.startsWith("parented-light-"))
      .length === 20,
    "all parented lights must survive import",
  );
  const camera = entities.get(manifest.camera!)!;
  const groups: AnimationDriverDescription[][] = [];
  for (let i = 0; i < manifest.clips.length; i += groupSize) {
    groups.push(
      manifest.clips.slice(i, i + groupSize).flatMap((entry) =>
        entry.clip.properties.map((property, track) => ({
          target: entities.get(entry.target)!,
          property,
          track,
          source: entry.clip.source,
        })),
      ),
    );
  }
  console.info(
    `Benchmark restored ${state.entities.length} entities; preparing ${groups.length} controllers`,
  );
  controllers = [features.controller];
  await batches(groups, async (drivers) => {
    controllers.push(
      await client.createAnimationController({
        drivers,
        speed: 1,
        looping: false,
      }),
    );
  });
  console.info("Benchmark controllers created; loading clip assets");
  mark("createControllers");
  const deadline = performance.now() + 600000;
  let resources;
  for (;;) {
    resources = [];
    let after = 0n;
    do {
      const page = await client.inspectPage({ collection: "resources", after });
      resources.push(...page.resources);
      after = page.next;
    } while (after !== 0n);
    if (resources.every((resource) => resource.status === "loaded")) break;
    check(
      !resources.some((resource) => resource.status === "failed"),
      "resource load failed",
    );
    check(
      performance.now() < deadline,
      `resources did not become ready: ${resources.filter((r) => r.status === "loaded").length}/${resources.length}`,
    );
  }
  mark("assetsReady");
  await batches(controllers, (id) =>
    client.controlAnimationController(id, { action: "play" }),
  );
  await batches(controllers, (id) =>
    client.controlAnimationController(id, { action: "pause" }),
  );
  await seek(0.5);
  console.info("Benchmark assets ready; activating camera");
  client.sendCommand({ type: "CameraActivateCommand", entity: camera });
  mark("playPauseSeek");
  return {
    fileBytes: bytes.length,
    savedBytes,
    stages,
    entities: state.entities.length,
    clips: manifest.clips.length,
    controllers: controllers.length,
    drivers: groups.reduce((n, group) => n + group.length, features.drivers),
    nativeEmitters: nativeEmitters.length,
    resources: resources.length,
    setupMs: performance.now() - started,
  };
}

export async function verifyFeatures() {
  return checkStressFeatures(
    client,
    fixture,
    seek,
    entities.get(manifest.camera!)!,
  );
}

/** Hold every other input fixed and vary only the imported mesh-pose weight. */
export async function poseCloseup(name: string) {
  await seek(0);
  const target = entities.get(name)!;
  const anchor = name === "walker-00-human" ? "walker-00-rig" : name;
  const page = await client.inspectPage({
    collection: "entities",
    target: entities.get(anchor)!,
  });
  const transform = page.entities[0]!.effective.find(
    (c) => c.component === client.components.Transform!.id,
  )!.fields;
  const camera = await createDetailCamera(
    {
      x: Number(transform.x),
      y: name.startsWith("walker") ? 3 : 1.6,
      z: Number(transform.z) + 10,
    },
    0.65,
  );
  const entry = manifest.clips.find(
    (clip) =>
      clip.target === name &&
      clip.clip.properties.some(
        (p) =>
          "component" in p && p.component === client.components.MeshPose!.id,
      ),
  );
  check(entry, `${name}: imported vertex-pose clip missing`);
  const controller = await client.createAnimationController({
    drivers: entry.clip.properties.flatMap((property, track) =>
      "component" in property &&
      property.component === client.components.MeshPose!.id
        ? [{ source: entry.clip.source, property, track, target }]
        : [],
    ),
  });
  await client.controlAnimationController(controller, { action: "play" });
  await client.controlAnimationController(controller, { action: "pause" });
  return { controller, camera };
}

export async function seek(time: number) {
  await batches(controllers, (id) =>
    client.controlAnimationController(id, { action: "seek", time }),
  );
}

export async function enableCulling() {
  const targets = cullingTargets;
  cullingTargets = [];
  return addCullingBounds(client, targets);
}

export async function warmNative(seconds: number) {
  const start = (await summary()).time;
  const deadline = performance.now() + 300000;
  while ((await summary()).time - start < seconds) {
    check(performance.now() < deadline, "native particle warmup timed out");
  }
}

export async function play() {
  await seek(0);
  await batches(controllers, (id) =>
    client.controlAnimationController(id, { action: "play" }),
  );
  await restartNative();
}

export async function restartNative() {
  restart++;
  const component = client.components.ParticleEmitter!;
  const outcome = await client.batch(
    nativeEmitters.map((id) => ({
      kind: "setField" as const,
      entity: { kind: "handle" as const, id },
      component: component.id,
      field: {
        offset: component.fields.restart!.offset,
        value: { kind: "u32" as const, value: restart },
      },
    })),
  );
  check(outcome.ok, "particle restart failed");
}

export async function particleView() {
  const page = await client.inspectPage({
    collection: "entities",
    target: entities.get("particles-native")!,
  });
  const transform = page.entities[0]!.effective.find(
    (c) => "qx" in c.fields,
  )!.fields;
  return createDetailCamera(
    { x: Number(transform.x), y: 4, z: Number(transform.z) + 15 },
    0.7,
  );
}

export async function pause() {
  await batches(controllers, (id) =>
    client.controlAnimationController(id, { action: "pause" }),
  );
}

async function createDetailCamera(
  placement: { x: number; y: number; z: number },
  fov: number,
) {
  const camera = client.components.Camera!;
  const position = client.components.Transform!;
  const commands: Command[] = [
    {
      kind: "create",
      alias: 1,
      metadata: { symbolicId: null, classes: [] },
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: camera.id,
      fields: [
        {
          offset: camera.fields.fov_y!.offset,
          value: { kind: "f32", value: fov },
        },
        {
          offset: camera.fields.far!.offset,
          value: { kind: "f32", value: 1000 },
        },
      ],
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: position.id,
      fields: Object.entries(placement).map(([key, value]) => ({
        offset: position.fields[key]!.offset,
        value: { kind: "f32" as const, value },
      })),
    },
  ];
  const outcome = await client.batch(commands);
  check(outcome.ok, "closeup camera creation failed");
  const id = outcome.aliases[0]!.id;
  client.sendCommand({ type: "CameraActivateCommand", entity: id });
  return id;
}

/** Isolate the imported pose clip while the rest of the World remains paused. */
export async function rigCloseup() {
  await seek(0);
  const rig = entities.get("walker-00-rig")!;
  const page = await client.inspectPage({
    collection: "entities",
    target: rig,
  });
  const transform = page.entities[0]!.effective.find(
    (c) => "qx" in c.fields,
  )!.fields;
  const id = await createDetailCamera(
    { x: Number(transform.x), y: 3, z: Number(transform.z) + 15 },
    0.6,
  );
  const entry = manifest.clips.find(
    (clip) =>
      clip.target === "walker-00-rig" &&
      clip.clip.properties.some((p) => "joints" in p),
  )!;
  check(entry, "Rigify pose clip missing");
  const controller = await client.createAnimationController({
    drivers: entry.clip.properties.map((property, track) => ({
      source: entry.clip.source,
      property,
      track,
      target: rig,
    })),
  });
  await client.controlAnimationController(controller, { action: "play" });
  await client.controlAnimationController(controller, { action: "pause" });
  return { controller, camera: id };
}

export async function rigSeek(controller: bigint, time: number) {
  await client.controlAnimationController(controller, { action: "seek", time });
}

export async function closeRigView(controller: bigint, camera: bigint) {
  await client.deleteAnimationController(controller);
  await closeDetailView(camera);
}

export async function closeDetailView(camera: bigint) {
  client.sendCommand({
    type: "CameraActivateCommand",
    entity: entities.get(manifest.camera!)!,
  });
  const outcome = await client.batch([
    { kind: "delete", entity: { kind: "handle", id: camera } },
  ]);
  check(outcome.ok, "closeup camera removal failed");
}

export async function summary() {
  return client.inspectPage({ collection: "summary" });
}

export async function probes(names: string[]) {
  return Promise.all(
    names.map(async (name) => {
      const id = entities.get(name);
      check(id, `missing entity ${name}`);
      const page = await client.inspectPage({
        collection: "entities",
        target: id,
      });
      return { name, entity: page.entities[0] };
    }),
  );
}

export async function capture(label: string) {
  const state = await summary();
  const frame = await client.presentation!.capture(state.tick);
  captures.set(label, frame);
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(
        new Uint8ClampedArray(frame.pixels),
        frame.width,
        frame.height,
      ),
      0,
      0,
    );
  const pixels = new Uint8Array(frame.pixels);
  let foreground = 0;
  for (let i = 0; i < pixels.length; i += 4)
    if (pixels[i]! + pixels[i + 1]! + pixels[i + 2]! > 25) foreground++;
  check(
    frame.drawCalls >
      (label.startsWith("rig-") ||
      label.startsWith("native-") ||
      label.startsWith("pose-")
        ? 0
        : 20) && foreground > 1000,
    "benchmark frame is empty",
  );
  return {
    png: canvas.toDataURL(),
    draws: frame.drawCalls,
    triangles: frame.triangles,
    backend: frame.backend,
    foreground,
  };
}

export function difference(first: string, second: string) {
  const a = captures.get(first)!,
    b = captures.get(second)!;
  const ap = new Uint8Array(a.pixels),
    bp = new Uint8Array(b.pixels);
  let changed = 0,
    maximum = 0;
  for (let i = 0; i < ap.length; i += 4) {
    const delta = Math.max(
      ...[0, 1, 2].map((lane) => Math.abs(ap[i + lane]! - bp[i + lane]!)),
    );
    if (delta) changed++;
    maximum = Math.max(maximum, delta);
  }
  return { changedPixels: changed, maxChannelDifference: maximum };
}

export async function close() {
  await host.close();
}
