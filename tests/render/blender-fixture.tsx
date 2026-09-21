import type { FrameCapture } from "@ipp/client";
import { createRoot, Entity, Transform, type ReactWorldRoot } from "@ipp/react";
import type { BlenderViewerHandle } from "../../examples/blender-viewer/main.js";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";
import type { BlenderSnapshot } from "../../integrations/blender/client/types.js";
import { compareImages, summarizeImage } from "./image-assertions.js";

const captures = new Map<string, FrameCapture>();
let overlay: ReactWorldRoot | undefined;

function viewer(): BlenderViewerHandle {
  const current = window.ippBlender;
  if (!current) throw new Error("Blender viewer is not ready");
  return current;
}

export async function observe(revision = 0, timeoutMs = 30_000) {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    const current = window.ippBlender;
    if (current?.error) throw new Error(current.error);
    if (current?.latest && current.latest.revision >= revision) {
      await current.adapter.flush();
      const inspection = await current.canvas.client.inspect();
      const failed = inspection.resources.find(
        (resource) => resource.status === "failed",
      );
      if (failed) throw new Error(`Resource failed: ${failed.error}`);
      if (
        inspection.resources.every((resource) => resource.status === "loaded")
      )
        return {
          session: current.canvas.client.session,
          exportSession: current.latest.session,
          revision: current.latest.revision,
          entities: [...current.latest.entities],
          inspection,
          diagnostics: current.latest.diagnostics,
          readyMilliseconds:
            performance.now() - current.adapter.importProfile.started,
        };
      await current.canvas.client.waitForFrame(inspection.tick);
    } else {
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
    }
    if (performance.now() >= deadline)
      throw new Error(`Timed out waiting for Blender revision ${revision}`);
  }
}

export async function capture(
  label: string,
  revision = 0,
  timeoutMs = 30_000,
  stable = false,
) {
  const state = await observe(revision, timeoutMs);
  if (stable) {
    // Timing ends at readiness. Wall-clock-driven effects cannot produce an
    // exact cross-run image; stop autoplay and withdraw native emitters only
    // for this comparison capture, after validating their imported resources.
    const client = viewer().canvas.client as BlenderClient;
    for (const controller of state.inspection.controllers ?? [])
      if (controller.state === "playing")
        client.playback(controller.id, { action: "stop" });
    const emitter = client.components.ParticleEmitter;
    if (emitter) {
      const outcome = await client.batch(
        state.inspection.entities
          .filter((entity) =>
            entity.base.some((component) => component.component === emitter.id),
          )
          .map((entity) => ({
            kind: "removeComponent",
            entity: { kind: "handle", id: entity.id },
            component: emitter.id,
          })),
      );
      if (!outcome.ok)
        throw new Error("Could not hold native effects for comparison");
    }
    state.inspection = await client.inspect();
  }
  const frame = await viewer().canvas.capture();
  if (frame.session !== state.session || frame.tick < state.inspection.tick)
    throw new Error("Blender capture belongs to an earlier session/frame");
  captures.set(label, frame);
  return {
    ...state,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    tick: frame.tick,
    contextGeneration: frame.contextGeneration,
    summary: summarizeImage(frame),
    backend: frame.backend,
  };
}

export async function overrideTransform(id: string, x: number) {
  if (!overlay) overlay = createRoot(viewer().canvas.client);
  const current = viewer();
  const handle = current.latest?.entities.get(id);
  const name = (await current.canvas.client.inspect()).entities.find(
    (entity) => entity.id === handle,
  )?.metadata.symbolicId;
  if (!name) throw new Error(`No named Blender entity ${id}`);
  await overlay.render(
    <Entity bindTo={name}>
      <Transform x={x} />
    </Entity>,
  );
  return viewer().canvas.client.inspect();
}

export async function releaseComponentStateOverlay() {
  await overlay?.unmount();
  overlay = undefined;
  return viewer().canvas.client.inspect();
}

export async function seek(target: string, time: number) {
  const current = viewer();
  const client = current.canvas.client as BlenderClient;
  const inspection = await client.inspect();
  const targetHandle = current.latest?.entities.get(target);
  const controllers =
    inspection.controllers?.filter((controller) =>
      controller.description.drivers.some(
        (driver) => driver.target === targetHandle,
      ),
    ) ?? [];
  if (!controllers.length) throw new Error(`No animation targets ${target}`);
  for (const controller of controllers) {
    client.playback(controller.id, { action: "play" });
    client.playback(controller.id, { action: "pause" });
    client.playback(controller.id, { action: "seek", time });
  }
  return client.inspect();
}

export async function stopPlayers() {
  const client = viewer().canvas.client as BlenderClient;
  for (const controller of (await client.inspect()).controllers ?? [])
    client.playback(controller.id, { action: "stop" });
  return client.inspect();
}

/** Fail a real runtime batch after edits and creation; the next export repairs it. */
export async function rejectInvalidRevision() {
  const current = viewer();
  const connection = new URLSearchParams(location.hash.slice(1));
  const url = new URL("/v1/scene", connection.get("endpoint")!);
  url.searchParams.set("token", connection.get("token")!);
  const snapshot: BlenderSnapshot = await (await fetch(url)).json();
  snapshot.revision++;
  const cube = snapshot.scene.entities.find(
    (entity) => entity.id === "fixture-cube",
  )!;
  cube.transform!.x = 50;
  snapshot.scene.entities.push({
    id: "partial-entity",
    name: "partial-entity",
  });
  const client = current.canvas.client;
  const batch = client.batchChunk.bind(client);
  client.batchChunk = (id, commands) =>
    batch(id, [
      ...commands,
      { kind: "delete", entity: { kind: "handle", id: 0xffffffffffffffffn } },
    ]);
  let error = "";
  try {
    await current.adapter.apply(snapshot);
  } catch (caught) {
    error = String(caught);
  } finally {
    client.batchChunk = batch;
  }
  return { error, revision: current.latest!.revision };
}

export function compare(first: string, second: string) {
  return compareImages(requireCapture(first), requireCapture(second));
}

export function colorCounts(label: string) {
  const pixels = new Uint8Array(requireCapture(label).pixels);
  const counts = { red: 0, green: 0, blue: 0, yellow: 0 };
  for (let index = 0; index < pixels.length; index += 4) {
    const r = pixels[index]!,
      g = pixels[index + 1]!,
      b = pixels[index + 2]!;
    if (r > 210 && g < 60 && b < 60) counts.red++;
    if (g > 210 && r < 60 && b < 60) counts.green++;
    if (b > 210 && r < 60 && g < 60) counts.blue++;
    if (r > 210 && g > 210 && b < 60) counts.yellow++;
  }
  return counts;
}

export async function restoreContext() {
  const presentation = viewer().canvas.client.presentation!;
  presentation.loseContext();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  presentation.restoreContext();
  await viewer().canvas.client.waitForFrame();
}

export function captureMetadata(label: string) {
  const { pixels: _pixels, ...metadata } = requireCapture(label);
  return metadata;
}

export function captureDataUrl(label: string) {
  const frame = requireCapture(label);
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d")!;
  context.putImageData(
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

function requireCapture(label: string) {
  const capture = captures.get(label);
  if (!capture) throw new Error(`Missing capture ${label}`);
  return capture;
}

/** Real controller rejection after one acknowledgement, repaired by a full revision. */
export async function correctPartialControllerRevision() {
  const current = viewer();
  const connection = new URLSearchParams(location.hash.slice(1));
  const url = new URL("/v1/scene", connection.get("endpoint")!);
  url.searchParams.set("token", connection.get("token")!);
  const snapshot: BlenderSnapshot = await (await fetch(url)).json();
  const animation = snapshot.scene.animations?.[0];
  if (!animation)
    throw new Error(
      "Controller recovery fixture requires an exported animation",
    );
  const session = current.canvas.client.session;
  const revision = current.latest!.revision;
  snapshot.revision = revision + 1;
  snapshot.scene.animations = ["review-first", "review-second"].map((id) => ({
    ...animation,
    id,
    autoplay: false,
  }));
  const client = current.canvas.client as BlenderClient;
  const create = client.createAnimationController.bind(client);
  const acknowledged: bigint[] = [];
  let calls = 0;
  client.createAnimationController = async (description) => {
    calls++;
    const handle = await create(
      calls === 2
        ? {
            ...description,
            drivers: description.drivers.map((driver) => ({
              ...driver,
              target: 0xffffffffffffffffn,
            })),
          }
        : description,
    );
    acknowledged.push(handle);
    return handle;
  };
  try {
    let error = "";
    try {
      await current.adapter.apply(snapshot);
    } catch (failure) {
      error = String(failure);
    }
    if (
      !error ||
      acknowledged.length !== 1 ||
      current.latest!.revision !== revision
    )
      throw new Error(
        "Partial controller failure advanced or lost acknowledgement state",
      );
    const intermediate = await client.inspectPage({
      collection: "controllers",
    });
    if (
      !intermediate.controllers!.some(
        (controller) => controller.id === acknowledged[0],
      )
    )
      throw new Error("Acknowledged controller was lost after rejection");
    const corrected = await current.adapter.apply(snapshot);
    const final = await client.inspectPage({ collection: "controllers" });
    if (
      calls !== 3 ||
      final.controllers!.length !== 2 ||
      !final.controllers!.some(
        (controller) => controller.id === acknowledged[0],
      )
    )
      throw new Error(
        "Correction duplicated or replaced an acknowledged controller",
      );
    return {
      error,
      calls,
      controllers: final.controllers!.length,
      previousRevision: revision,
      correctedRevision: corrected.revision,
      sameSession: client.session === session,
    };
  } finally {
    client.createAnimationController = create;
  }
}

export function importMeasurements() {
  const current = viewer();
  return {
    ...current.adapter.importProfile,
    ready: performance.now() - current.adapter.importProfile.started,
    clips: current.latest?.clips.length,
  };
}

/** A late local encoding error must preserve earlier complete runtime frames. */
export async function correctCommandEncodingFailure() {
  const current = viewer();
  const connection = new URLSearchParams(location.hash.slice(1));
  const url = new URL("/v1/scene", connection.get("endpoint")!);
  url.searchParams.set("token", connection.get("token")!);
  const snapshot: BlenderSnapshot = await (await fetch(url)).json();
  snapshot.revision++;
  const count = snapshot.scene.entities.length;
  for (let index = 0; index < 5000; index++)
    snapshot.scene.entities.push({
      id: `encoding-recovery-${index}`,
      name: `encoding-recovery-${index}`,
      transform: {
        x: index === 4999 ? Number.NaN : 0,
        y: 0,
        z: 0,
        qx: 0,
        qy: 0,
        qz: 0,
        qw: 1,
        sx: 1,
        sy: 1,
        sz: 1,
      },
    });
  let error = "";
  try {
    await current.adapter.apply(snapshot);
  } catch (caught) {
    error = String(caught);
  }
  const before = await current.canvas.client.inspect();
  const identities = before.entities
    .filter((entity) =>
      entity.metadata.symbolicId?.startsWith("encoding-recovery-"),
    )
    .map((entity) => [entity.metadata.symbolicId, entity.id] as const);
  snapshot.scene.entities.at(-1)!.transform!.x = 0;
  await current.adapter.apply(snapshot);
  const after = await current.canvas.client.inspect();
  const restored = new Map(
    after.entities.map((entity) => [entity.metadata.symbolicId, entity.id]),
  );
  const preserved = identities.every(([name, id]) => restored.get(name) === id);
  snapshot.scene.entities.length = count;
  snapshot.revision++;
  await current.adapter.apply(snapshot);
  return {
    error,
    preserved,
    acknowledged: identities.length,
    entities: (await current.canvas.client.inspect()).entities.length,
  };
}

/** Capture the last completed GPU frame while authored transforms are held mid-batch. */
export async function captureCommandBatchBoundary() {
  const current = viewer();
  const client = current.canvas.client;
  await stopPlayers();
  const state = await client.inspect();
  const handle = current.latest!.entities.get("fixture-cube")!;
  const transform = client.components.Transform!;
  const original = state.entities
    .find((entity) => entity.id === handle)!
    .base.find((component) => component.component === transform.id)!.fields
    .x as number;
  const write = (x: number) => ({
    kind: "setField" as const,
    entity: { kind: "handle" as const, id: handle },
    component: transform.id,
    field: {
      offset: transform.fields.x!.offset,
      value: { kind: "f32" as const, value: x },
    },
  });
  const before = await current.canvas.capture();
  captures.set("batch-before", before);
  const id = await client.beginBatch();
  const first = await client.batchChunk(id, [write(1000)]);
  if (!first.ok) throw new Error("Batch fixture transform rejected");
  const held = await client.presentation!.capture(first.tick);
  captures.set("batch-held", held);
  const second = await client.batchChunk(id, []);
  await client.endBatch(id);
  await client.waitForFrame(first.tick);
  const complete = await current.canvas.capture();
  captures.set("batch-complete", complete);
  const restored = await client.batch([write(original)]);
  if (!restored.ok) throw new Error("Batch fixture restoration rejected");
  const after = await current.canvas.capture();
  captures.set("batch-restored", after);
  return {
    firstTick: first.tick,
    secondTick: second.tick,
    heldTick: held.tick,
    completeTick: complete.tick,
    heldDifference: compareImages(before, held),
    completedDifference: compareImages(before, complete),
    restoredDifference: compareImages(before, after),
    renderer: complete.backend.unmaskedRenderer,
  };
}
