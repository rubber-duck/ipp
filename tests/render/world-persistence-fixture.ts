import type {
  AnimationWorldClient,
  AnimationControllerSnapshot,
  CameraWorldClient,
  FrameCapture,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { AnimationFixture, check } from "../integration/animation-fixtures.js";
import { compareImages, summarizeImage } from "./image-assertions.js";

type WorldClient = AnimationWorldClient & CameraWorldClient;
type Configuration = { generated: string; workerScript: string; wasm: string };
let configuration: Configuration;
let host: WorldPersistenceHostClient<WorldClient> | undefined;
let client: WorldClient;
let saved: Uint8Array;
let controller: bigint;
let source: string;
let savedController: AnimationControllerSnapshot;
const frames = new Map<string, FrameCapture>();

async function connect(config: Configuration) {
  configuration = config;
  const contract = await import(config.generated);
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 240;
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
  const frame = await client.presentation!.capture(state.tick);
  check(frame.tick > state.tick, "Capture did not finish a new frame");
  frames.set(label, frame);
  return {
    label,
    summary: summarizeImage(frame),
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
  };
}

export async function prepare(config: Configuration, clipSource: string) {
  const contract = await connect(config);
  source = clipSource;
  client = await host!.createWorld({ symbolicId: "external-animation" });
  const fixture = new AnimationFixture(client, contract, async () => {});
  const camera = await fixture.create("camera", {
    Transform: { z: 6 },
    Camera: { projection: 1, focus_distance: 6, ortho_height: 4 },
  });
  client.sendCommand({ type: "CameraActivateCommand", entity: camera });
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
  saved = await host!.saveWorld();
  const again = await host!.saveWorld();
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
  await host!.close();
  host = undefined;
  return { bytes: saved.length, before, controller: savedController };
}

export async function restore() {
  await connect(configuration);
  client = await host!.loadWorld(saved);
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
  client.sendCommand({ type: "CameraActivateCommand", entity: camera.id });
  const after = await capture("restored-paused");
  return {
    after,
    difference: compareImages(
      frames.get("saved-paused")!,
      frames.get("restored-paused")!,
    ),
  };
}

export async function recoverAndStop() {
  client.presentation!.loseContext();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  client.presentation!.restoreContext();
  await ready();
  await capture("recovered-paused");
  const recovery = compareImages(
    frames.get("restored-paused")!,
    frames.get("recovered-paused")!,
  );
  await client.controlAnimationController(controller, { action: "stop" });
  const state = await client.inspect();
  for (const entity of state.entities.filter((entity) =>
    entity.metadata.symbolicId?.startsWith("cube-"),
  )) {
    const transform = entity.effective.find(
      (component) => component.component === client.components.Transform!.id,
    );
    check(
      transform?.fields.y === -0.5,
      "Stop did not restore saved underlying transform",
    );
  }
  const stopped = await capture("restored-stopped");
  const difference = compareImages(
    frames.get("restored-paused")!,
    frames.get("restored-stopped")!,
  );
  await host!.close();
  host = undefined;
  return { recovery, stopped, difference };
}

export async function restoreAndComplete() {
  await connect(configuration);
  client = await host!.loadWorld(saved);
  let state = await ready();
  const camera = state.entities.find(
    (entity) => entity.metadata.symbolicId === "camera",
  );
  check(camera, "Saved camera missing");
  client.sendCommand({ type: "CameraActivateCommand", entity: camera.id });
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
        const transform = entity.effective.find(
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
      await host!.close();
      host = undefined;
      return { controller: stableRestored, completed };
    }
    check(
      performance.now() < deadline,
      "Restored interrupted transition did not complete",
    );
  }
}

export async function restoreUnavailable() {
  await connect(configuration);
  client = await host!.loadWorld(saved);
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

export async function close() {
  await host?.close();
  host = undefined;
}
