import { renderDiagnostics } from "../../../packages/ipp-client/src/diagnostics.js";
import { presentationTesting } from "../../../packages/ipp-client/src/testing.js";
import type { PresentedCapture } from "@ipp/client";
import { createRoot, Entity, Transform, type ReactWorldRoot } from "@ipp/react";
import type { BlenderViewerHandle } from "../../../examples/blender-viewer/main.js";
import { sameOutput } from "../../../examples/blender-viewer/camera.js";
import type { BlenderSnapshot } from "../../../integrations/blender/client/types.js";
import {
  compareImages,
  summarizeImage,
  rgbaDataUrl,
} from "../../harness/page/images.js";
import { capturedPixels } from "../../rendering/canvas/support/canvas-page.js";

const captures = new Map<string, PresentedCapture>();
/** A React root that binds a Blender entity and writes its Transform. */
let overrideRoot: ReactWorldRoot | undefined;

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
      await current.presentation;
      await current.canvas.flush();
      const inspection = await current.canvas.client.inspect();
      const failed = inspection.resources.find(
        (resource) => resource.status === "failed",
      );
      if (failed) throw new Error(`Resource failed: ${failed.error}`);
      if (
        inspection.resources.every(
          (resource) => resource.status === "loaded",
        ) &&
        sameOutput(current.output, current.canvas.view?.binding.output)
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
    const client = viewer().client;
    for (const controller of state.inspection.controllers ?? [])
      if (controller.state === "playing")
        client.playback(controller.id, { action: "stop" });
    const emitter = client.components.ParticleEmitter;
    if (emitter) {
      const outcome = await client.batch(
        state.inspection.entities
          .filter((entity) =>
            entity.components.some(
              (component) => component.component === emitter.id,
            ),
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
  const current = viewer();
  await current.client.waitForFrame(state.inspection.tick);
  const barrier = await current.canvas.frame();
  const frame = await current.canvas.capture({
    afterSequence: barrier.sequence,
  });
  if (
    viewer() !== current ||
    current.client.session !== state.session ||
    !sameOutput(current.output, frame.view.binding.output)
  )
    throw new Error("Blender capture belongs to another session/output");
  captures.set(label, frame);
  return {
    ...state,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    sequence: frame.sequence,
    publication: frame.publication,
    contextGeneration: frame.view.surface.context,
    summary: summarizeImage(capturedPixels(frame)),
    failedDrawCalls: frame.failedDrawCalls,
    statistics: await diagnostics().statistics(),
  };
}

export async function overrideTransform(id: string, x: number) {
  if (!overrideRoot) overrideRoot = createRoot(viewer().canvas.client);
  const current = viewer();
  const handle = current.latest?.entities.get(id);
  const name = (await current.canvas.client.inspect()).entities.find(
    (entity) => entity.id === handle,
  )?.metadata.symbolicId;
  if (!name) throw new Error(`No named Blender entity ${id}`);
  await overrideRoot.render(
    <Entity bindTo={name}>
      <Transform x={x} />
    </Entity>,
  );
  return viewer().canvas.client.inspect();
}

/** Unmount the override root; unmount deletes nothing, so the bound cube keeps the written Transform. */
export async function unmountTransformOverride() {
  await overrideRoot?.unmount();
  overrideRoot = undefined;
  return viewer().canvas.client.inspect();
}

export async function seek(target: string, time: number) {
  const current = viewer();
  const client = current.client;
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
  const client = viewer().client;
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
  const batch = client.batch.bind(client);
  client.batch = (commands) =>
    batch([
      ...commands,
      { kind: "delete", entity: { kind: "handle", id: 0xffffffffffffffffn } },
    ]);
  let error = "";
  try {
    await current.adapter.apply(snapshot);
  } catch (caught) {
    error = String(caught);
  } finally {
    client.batch = batch;
  }
  return { error, revision: current.latest!.revision };
}

export function compare(first: string, second: string) {
  return compareImages(
    capturedPixels(requireCapture(first)),
    capturedPixels(requireCapture(second)),
  );
}

export function colorCounts(label: string) {
  const pixels = new Uint8Array(requireCapture(label).pixels);
  const counts = {
    red: 0,
    green: 0,
    blue: 0,
    yellow: 0,
    pixels: pixels.length / 4,
  };
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
  const current = viewer();
  const previous = await current.canvas.host.presentation.surface();
  const testing = presentationTesting(diagnostics());
  const deadline = performance.now() + 20_000;
  testing.loseContext();
  for (;;) {
    try {
      await current.canvas.frame();
    } catch (error) {
      if (!(error instanceof Error) || !("reason" in error)) throw error;
      if (error.reason !== "unavailable" && error.reason !== "staleView")
        throw error;
      break;
    }
    if (performance.now() >= deadline)
      throw new Error("Blender presentation did not observe context loss");
    await animationFrames(1);
  }
  testing.restoreContext();
  for (;;) {
    try {
      const surface = await current.canvas.host.presentation.surface();
      if (surface.context > previous.context) break;
    } catch (error) {
      if (!(error instanceof Error) || !("reason" in error)) throw error;
      if (error.reason !== "unavailable" && error.reason !== "staleView")
        throw error;
    }
    if (performance.now() >= deadline)
      throw new Error("Blender presentation context did not recover");
    await animationFrames(1);
  }
  await current.canvas.recoverPresentation();
}

function diagnostics() {
  const value = renderDiagnostics(viewer().canvas.host);
  if (!value) throw new Error("Blender fixture requires render diagnostics");
  return value;
}

export function captureMetadata(label: string) {
  const { pixels: _pixels, ...metadata } = requireCapture(label);
  return metadata;
}

export function captureDataUrl(label: string) {
  const frame = requireCapture(label);
  const { width, height } = frame.view.binding.viewport;
  return rgbaDataUrl({ width, height, pixels: frame.pixels });
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
  const client = current.client;
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

/** Commands in one full batch page of the viewer's generated client. */
function pageCommands(): number {
  return (
    viewer().canvas.client as unknown as {
      commandPageLimits: { commands: number };
    }
  ).commandPageLimits.commands;
}

export function importMeasurements() {
  const current = viewer();
  return {
    ...current.adapter.importProfile,
    ready: performance.now() - current.adapter.importProfile.started,
    clips: current.latest?.clips.length,
    pageCommands: pageCommands(),
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

/** Capture completed GPU frames while a batch's first page waits for its final page. */
export async function captureCommandBatchBoundary() {
  const current = viewer();
  const client = current.canvas.client;
  await stopPlayers();
  const state = await client.inspect();
  const handle = current.latest!.entities.get("fixture-cube")!;
  const transform = client.components.Transform!;
  const original = state.entities
    .find((entity) => entity.id === handle)!
    .components.find((component) => component.component === transform.id)!
    .fields.x as number;
  const write = (x: number) => ({
    kind: "setField" as const,
    entity: { kind: "handle" as const, id: handle },
    component: transform.id,
    field: {
      offset: transform.fields.x!.offset,
      value: { kind: "f32" as const, value: x },
    },
  });
  await client.waitForFrame(state.tick);
  const beforeBarrier = await current.canvas.frame();
  const before = await current.canvas.capture({
    afterSequence: beforeBarrier.sequence,
  });
  captures.set("batch-before", before);
  const evaluatedBefore = (await client.waitForFrame(0n)).tick;
  // A full first page of moves leaves at once; the batch applies at finish().
  const batch = client.openBatch();
  batch.write(Array.from({ length: pageCommands() + 1 }, () => write(1000)));
  await animationFrames(6);
  const openTick = (await client.waitForFrame(evaluatedBefore)).tick;
  const openBarrier = await current.canvas.frame();
  const open = await current.canvas.capture({
    afterSequence: openBarrier.sequence,
  });
  const applied = await batch.finish();
  if (!applied.ok) throw new Error("Batch fixture transform rejected");
  const evaluated = await client.waitForFrame(applied.tick);
  const completeBarrier = await current.canvas.frame();
  const complete = await current.canvas.capture({
    afterSequence: completeBarrier.sequence,
  });
  captures.set("batch-complete", complete);
  const restored = await client.batch([write(original)]);
  if (!restored.ok) throw new Error("Batch fixture restoration rejected");
  await client.waitForFrame(restored.tick);
  const restoredBarrier = await current.canvas.frame();
  const after = await current.canvas.capture({
    afterSequence: restoredBarrier.sequence,
  });
  captures.set("batch-restored", after);
  return {
    evaluatedBefore,
    openTick,
    pages: batch.pages,
    pageCommands: pageCommands(),
    completeTick: evaluated.tick,
    openDifference: compareImages(capturedPixels(before), capturedPixels(open)),
    completedDifference: compareImages(
      capturedPixels(before),
      capturedPixels(complete),
    ),
    restoredDifference: compareImages(
      capturedPixels(before),
      capturedPixels(after),
    ),
    renderer: (await diagnostics().statistics()).device.unmaskedRenderer,
  };
}

/** Yield `count` page animation frames; the worker schedules its own frames alongside. */
async function animationFrames(count: number): Promise<void> {
  for (let index = 0; index < count; index += 1)
    await new Promise((resolve) => requestAnimationFrame(resolve));
}
