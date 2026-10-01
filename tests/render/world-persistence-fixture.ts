import { presentationTesting } from "../../packages/ipp-client/src/testing.js";
import type {
  AnimationWorldClient,
  AnimationControllerSnapshot,
  PresentedCapture,
  WorldPersistenceHostClient,
  WorldReference,
} from "@ipp/client";
import { AnimationFixture, check } from "../integration/animation-fixtures.js";
import { compareImages, summarizeImage } from "./image-assertions.js";
import {
  RootPresentation,
  capturedImage,
  recoverRestoredContext,
} from "./root-presentation.js";
import {
  CONSTRAINTS,
  SCENE,
  selectSystems,
} from "../integration/system-selections.js";

type Configuration = { generated: string; workerScript: string; wasm: string };
const VIEWPORT = { width: 320, height: 240 };
let configuration: Configuration;
let host: WorldPersistenceHostClient<AnimationWorldClient> | undefined;
let client: AnimationWorldClient;
let presentation: RootPresentation | undefined;
let worlds: WorldReference[] = [];
let saved: Uint8Array;
let controller: bigint;
let source: string;
let savedController: AnimationControllerSnapshot;
const frames = new Map<string, PresentedCapture>();

async function connect(config: Configuration) {
  configuration = config;
  const contract = await import(config.generated);
  const canvas = document.createElement("canvas");
  canvas.width = VIEWPORT.width;
  canvas.height = VIEWPORT.height;
  document.body.replaceChildren(canvas);
  host = await contract.IppHostClient.connectWorker(
    config.workerScript,
    config.wasm,
    {
      canvas: canvas.transferControlToOffscreen(),
    },
  );
  return contract;
}

async function openSaved() {
  await connect(configuration);
  const graph = await host!.loadWorld(saved);
  worlds = [...graph.created.values()];
  client = await host!.openWorld(graph.root);
}

async function present(camera: bigint) {
  presentation = await RootPresentation.camera(
    host!,
    worldReference(),
    camera,
    VIEWPORT,
  );
}

function worldReference(): WorldReference {
  const reference = client.worldReference;
  check(reference, "World client has no World reference");
  return reference;
}

async function closeHost() {
  const current = host;
  host = undefined;
  if (!current) return;
  try {
    await presentation?.close();
    for (const session of current.sessions.values()) await session.close();
    for (const world of worlds) await current.destroyWorld(world);
  } finally {
    presentation = undefined;
    worlds = [];
    await current.close();
  }
}

async function ready() {
  const deadline = performance.now() + 15_000;
  for (;;) {
    const state = await client.inspect();
    const failed = state.resources.find(
      (resource) => resource.status === "failed",
    );
    check(!failed, `External source failed: ${failed?.error}`);
    if (
      state.resources.length >= 2 &&
      state.resources.every((resource) => resource.status === "loaded")
    )
      return state;
    check(
      performance.now() < deadline,
      "External resource readiness timed out",
    );
    await client.waitForFrame(state.tick);
  }
}

async function capture(label: string) {
  const state = await client.inspect();
  check(presentation, "Camera presentation is not selected");
  const frame = await presentation.capture();
  check(
    presentation.sourceTick(frame) > state.tick,
    "Capture does not include the inspected World state",
  );
  frames.set(label, frame);
  return {
    label,
    summary: summarizeImage(capturedImage(frame)),
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
  };
}

export async function prepare(config: Configuration, clipSource: string) {
  const contract = await connect(config);
  source = clipSource;
  const created = await host!.createWorld({
    selectedSystems: selectSystems(SCENE, CONSTRAINTS),
    symbolicId: "external-animation",
  });
  worlds = [created.reference];
  client = await host!.openWorld(created.reference);
  const fixture = new AnimationFixture(client, contract, async () => {});
  const camera = await fixture.create("camera", {
    Transform: { z: 6 },
    Camera: { projection: 1, focus_distance: 6, ortho_height: 4 },
  });
  await present(camera);
  const targets = [];
  for (const [index, x] of [-1, 1].entries())
    targets.push(
      await fixture.create(`cube-${index}`, {
        Transform: { x, y: -0.5, sx: 0.3, sy: 0.3, sz: 0.3 },
        MeshInstance: { source: "ipp://mesh/cube?width=2&height=2&length=2" },
        UnlitMaterial: { r: 0, g: 0, b: 1 },
      }),
    );
  controller = await client.createAnimationController({
    drivers: targets.map((target) => ({
      source,
      track: 0,
      target,
      property: {
        component: client.components.Transform!.id,
        offsets: [client.components.Transform!.fields.y!.offset],
      },
    })),
  });
  await ready();
  await client.controlAnimationController(controller, { action: "play" });
  await client.controlAnimationController(controller, { action: "pause" });
  await client.controlAnimationController(controller, {
    action: "seek",
    time: 0.75,
  });
  const paused = (await client.inspect()).controllers?.find(
    ({ id }) => id === controller,
  );
  check(paused, "Prepared controller missing");
  await client.transitionAnimationController(controller, {
    description: paused.description,
    duration: 2,
    easing: "smoothstep",
    startTime: { policy: "seek", time: 1.5 },
  });
  await client.controlAnimationController(controller, { action: "play" });
  const firstTransitionDeadline = performance.now() + 10_000;
  for (;;) {
    await client.waitForFrame();
    const state = await client.inspect();
    const transitioning = state.controllers?.find(
      ({ id }) => id === controller,
    );
    if ((transitioning?.transition?.elapsed ?? 0) > 0.02) break;
    check(
      performance.now() < firstTransitionDeadline,
      "Initial persistence transition did not advance",
    );
  }
  await client.controlAnimationController(controller, { action: "pause" });
  const firstTransition = (await client.inspect()).controllers?.find(
    ({ id }) => id === controller,
  );
  check(firstTransition?.transition, "Initial transition disappeared");
  await client.transitionAnimationController(controller, {
    description: firstTransition.description,
    duration: 1.5,
    easing: "linear",
    startTime: { policy: "seek", time: 0.25 },
  });
  await client.controlAnimationController(controller, { action: "play" });
  const interruptedTransitionDeadline = performance.now() + 10_000;
  for (;;) {
    await client.waitForFrame();
    const state = await client.inspect();
    const transitioning = state.controllers?.find(
      ({ id }) => id === controller,
    );
    if ((transitioning?.transition?.elapsed ?? 0) > 0.02) break;
    check(
      performance.now() < interruptedTransitionDeadline,
      "Interrupted persistence transition did not advance",
    );
  }
  await client.controlAnimationController(controller, { action: "pause" });
  savedController = (await client.inspect()).controllers?.find(
    ({ id }) => id === controller,
  )!;
  check(
    savedController.transition !== undefined,
    "Prepared transition missing",
  );
  check(
    savedController.state === "paused" &&
      savedController.transition.duration === 1.5 &&
      savedController.transition.elapsed > 0 &&
      savedController.transition.elapsed <
        savedController.transition.duration &&
      savedController.transition.easing === "linear" &&
      !savedController.transition.pending,
    "Prepared transition did not reach a paused in-flight sample",
  );
  const before = await capture("saved-paused");
  saved = await host!.saveWorld(client.session);
  const again = await host!.saveWorld(client.session);
  check(
    saved.length === again.length &&
      saved.every((byte, i) => byte === again[i]),
    "Unchanged paused World save is nondeterministic",
  );
  check(
    new TextDecoder().decode(saved).includes(source),
    "Save changed the external clip reference",
  );
  check(
    !new TextDecoder().decode(saved).includes("bundle://"),
    "Save embedded a bundle",
  );
  await closeHost();
  return { bytes: saved.length, before, controller: savedController };
}

export async function restore() {
  await openSaved();
  const state = await ready();
  check(
    state.controllers?.length === 1,
    "Controller missing after fresh-worker load",
  );
  const restored = state.controllers[0]!;
  check(
    restored.id === controller &&
      restored.state === "paused" &&
      restored.time === savedController.time,
    "Saved controller identity/clock lost",
  );
  check(restored.transition !== undefined, "Saved transition missing");
  check(
    savedController.transition !== undefined,
    "Original saved transition missing",
  );
  check(
    restored.transition.duration === savedController.transition.duration &&
      restored.transition.elapsed === savedController.transition.elapsed &&
      restored.transition.easing === savedController.transition.easing &&
      restored.transition.pending === savedController.transition.pending,
    "Saved transition origin/progress lost",
  );
  check(
    restored.description.drivers.length === 2 &&
      restored.description.drivers.every(
        (driver) =>
          driver.source === source &&
          state.entities.some((entity) => entity.id === driver.target),
      ),
    "Multi-entity controller binding lost",
  );
  const camera = state.entities.find(
    (entity) => entity.metadata.symbolicId === "camera",
  );
  check(camera, "Saved camera missing");
  await present(camera.id);
  const after = await capture("restored-paused");
  return {
    after,
    difference: compareImages(
      capturedImage(frames.get("saved-paused")!),
      capturedImage(frames.get("restored-paused")!),
    ),
  };
}

export async function recoverAndStop() {
  check(presentation, "Camera presentation is not selected");
  presentationTesting(presentation.diagnostics).loseContext();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  presentationTesting(presentation.diagnostics).restoreContext();
  await recoverRestoredContext(presentation);
  await ready();
  await capture("recovered-paused");
  const recovery = compareImages(
    capturedImage(frames.get("restored-paused")!),
    capturedImage(frames.get("recovered-paused")!),
  );
  await client.controlAnimationController(controller, { action: "stop" });
  const state = await client.inspect();
  for (const entity of state.entities.filter((entity) =>
    entity.metadata.symbolicId?.startsWith("cube-"),
  )) {
    const transform = entity.components.find(
      (component) => component.component === client.components.Transform!.id,
    );
    check(
      transform?.fields.y === -0.5,
      "Stop did not restore saved underlying transform",
    );
  }
  const stopped = await capture("restored-stopped");
  const difference = compareImages(
    capturedImage(frames.get("restored-paused")!),
    capturedImage(frames.get("restored-stopped")!),
  );
  await closeHost();
  return { recovery, stopped, difference };
}

export async function restoreAndComplete() {
  await openSaved();
  let state = await ready();
  const camera = state.entities.find(
    (entity) => entity.metadata.symbolicId === "camera",
  );
  check(camera, "Saved camera missing");
  await present(camera.id);
  await client.controlAnimationController(controller, { action: "play" });
  const deadline = performance.now() + 10_000;
  for (;;) {
    await client.waitForFrame();
    state = await client.inspect();
    const restored = state.controllers?.find(({ id }) => id === controller);
    check(restored, "Restored controller disappeared");
    if (!restored.transition) {
      await client.controlAnimationController(controller, { action: "pause" });
      state = await client.inspect();
      const stableRestored = state.controllers?.find(
        ({ id }) => id === controller,
      );
      check(stableRestored, "Completed restored controller disappeared");
      for (const entity of state.entities.filter((entry) =>
        entry.metadata.symbolicId?.startsWith("cube-"),
      )) {
        const transform = entity.components.find(
          (component) =>
            component.component === client.components.Transform!.id,
        );
        check(
          Math.abs(Number(transform?.fields.y) - (-0.5 + stableRestored.time)) <
            1e-4,
          "Completed restored transition did not expose its destination sample",
        );
      }
      const completed = await capture("restored-transition-completed");
      await closeHost();
      return { controller: stableRestored, completed };
    }
    check(
      performance.now() < deadline,
      "Restored interrupted transition did not complete",
    );
  }
}

export async function restoreUnavailable() {
  await openSaved();
  const deadline = performance.now() + 10_000;
  for (;;) {
    const state = await client.inspect();
    check(
      state.entities.length === 3,
      "Unavailable asset prevented World publication",
    );
    const restored = state.controllers?.[0];
    check(
      restored?.state === "paused" && restored.time === savedController.time,
      "Unavailable source lost saved clock",
    );
    check(
      restored.description.drivers.every((driver) => driver.source === source),
      "Unavailable reference was rewritten",
    );
    if (
      state.resources.some(
        (resource) =>
          resource.source === source && resource.status === "failed",
      )
    )
      return {
        entities: state.entities.length,
        time: restored.time,
        transition: restored.transition,
      };
    check(
      performance.now() < deadline,
      "Unavailable source did not report failure",
    );
    await client.waitForFrame(state.tick);
  }
}

export function captureDataUrl(label: string) {
  const frame = frames.get(label);
  check(frame, `Missing capture ${label}`);
  const { width, height, pixels } = capturedImage(frame);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(new Uint8ClampedArray(pixels.slice(0)), width, height),
      0,
      0,
    );
  return canvas.toDataURL("image/png");
}

export async function close() {
  await closeHost();
}
