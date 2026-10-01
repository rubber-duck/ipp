import type {
  Client,
  HostClientBase,
  PresentedCapture,
  PresentationView,
  Command,
} from "@ipp/client";
import { canvasOutput } from "../../../packages/ipp-client/src/references.js";
import { presentationTesting } from "../../../packages/ipp-client/src/testing.js";
export { nativePresentationTransport } from "../../../packages/ipp-client/src/native-presentation.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import type { PresentationTransferProbe } from "../presentation-transport.js";
import { CAMERA, CANVAS, selectSystems } from "../system-selections.js";
export {
  presentationTransport,
  workerTransport,
} from "../presentation-transport.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function rejected(promise: Promise<unknown>, reasons: string[]) {
  const error = await promise.then(
    () => undefined,
    (error: unknown) => error,
  );
  check(
    error instanceof Error &&
      "reason" in error &&
      reasons.includes(String(error.reason)),
    `Expected ${reasons.join("/")}, got ${String(error)}`,
  );
}

function pixel(
  capture: PresentedCapture,
  x: number,
  y: number,
  expected: number[],
) {
  const { width, height } = capture.view.binding.viewport;
  check(
    capture.pixels.byteLength === width * height * 4,
    "Capture extent differs from its exact view",
  );
  const actual = new Uint8Array(capture.pixels, (y * width + x) * 4, 4);
  check(
    expected.every((value, index) => Math.abs(actual[index]! - value) <= 2),
    `Pixel (${x},${y}): ${[...actual]} expected ${expected}`,
  );
}

export async function connectionPresentationLifetime(
  connect: () => Promise<HostClientBase<Client>>,
) {
  const owner = await connect();
  const observer = await connect();
  const world = await owner.createWorld({
    selectedSystems: selectSystems(CAMERA),
    symbolicId: "connection-independent-view",
  });
  try {
    const client = await owner.openWorld(world.reference);
    const outcome = successfulBatch(
      await client.batch([
        createEntity(1, "camera"),
        insertComponent(client, "Camera", { kind: "alias", alias: 1 }),
      ]),
    );
    const output = await owner.bindOutput(
      world.reference,
      aliasId(outcome, 1),
      "camera",
    );
    const binding = await owner.setRootOutput(output, {
      width: 64,
      height: 64,
      devicePixelRatio: 1,
    });
    const view = await owner.presentation.select(
      await owner.presentation.surface(),
      binding,
    );
    const before = await owner.presentation.capture(view);
    await owner.close();
    check(
      (await observer.getRootOutputBinding(world.reference))?.generation
        .serial === binding.generation.serial,
      "Configuring connection close cleared independently living root",
    );
    const after = await observer.presentation.capture(view, {
      afterSequence: before.sequence,
    });
    check(
      after.view.selection === view.selection &&
        after.view.surface.context === view.surface.context,
      "Configuring connection close reset physical selection/context",
    );
    return {
      before: { view: before.view, sequence: before.sequence },
      after: { view: after.view, sequence: after.sequence },
    };
  } finally {
    await owner.close();
    await observer.destroyWorld(world.reference);
    await observer.close();
  }
}

export async function explicitPresentation(
  host: HostClientBase<Client>,
  probe: PresentationTransferProbe,
) {
  const surface = await host.presentation.surface();
  const viewport = { width: 96, height: 64, devicePixelRatio: 1 };
  const worlds = await Promise.all(
    ["red", "blue", "camera"].map((symbolicId) =>
      host.createWorld({
        selectedSystems:
          symbolicId === "camera"
            ? selectSystems(CAMERA)
            : selectSystems(CANVAS),
        symbolicId,
        ...(symbolicId === "camera"
          ? {}
          : { canvas: { extent: [96, 64], unitsPerMetre: 100 } as const }),
      }),
    ),
  );
  const clients = await Promise.all(
    worlds.map((world) => host.openWorld(world.reference)),
  );
  const [red, blue, camera] = clients as [Client, Client, Client];
  async function canvas(client: Client, name: string, color: "red" | "blue") {
    const root = { kind: "alias", alias: 1 } as const;
    const shape = { kind: "alias", alias: 2 } as const;
    const edits: Command[] = [
      createEntity(1, `${name}-canvas`),
      createEntity(2, `${name}-shape`),
      insertComponent(client, "CanvasBox", shape, { width: 96, height: 64 }),
      insertComponent(client, "CanvasStyle", shape, {
        red: Number(color === "red"),
        green: 0,
        blue: Number(color === "blue"),
      }),
      {
        kind: "placeEntity",
        entity: shape,
        placement: { parent: root, before: null },
      },
    ];
    if (color === "red") {
      const marker = { kind: "alias", alias: 3 } as const;
      edits.push(
        createEntity(3, "top-left-marker"),
        insertComponent(client, "CanvasBox", marker, { width: 32, height: 20 }),
        insertComponent(client, "CanvasStyle", marker, {
          red: 0,
          green: 1,
          blue: 0,
        }),
        {
          kind: "placeEntity",
          entity: marker,
          placement: { parent: root, before: null },
        },
      );
    }
    successfulBatch(await client.batch(edits));
    return canvasOutput(client.worldReference!);
  }
  const redOutput = await canvas(red, "red", "red");
  const blueOutput = await canvas(blue, "blue", "blue");
  const redBinding = await host.setRootOutput(redOutput, viewport);
  const blueBinding = await host.setRootOutput(blueOutput, viewport);
  let view = await host.presentation.select(surface, redBinding);
  async function capture(current: PresentationView) {
    for (let attempt = 0; attempt < 90; attempt++) {
      const image = await host.presentation.capture(current);
      if (image.drawCalls > 0 && image.failedDrawCalls === 0) return image;
    }
    throw new Error("Selected Canvas never became drawable");
  }
  const first = await capture(view);
  pixel(first, 8, 8, [0, 255, 0, 255]);
  pixel(first, 8, 56, [255, 0, 0, 255]);
  const peer = await host.openWorld(worlds[1]!.reference);
  await peer.close();
  await red.close();
  const independent = await capture(view);
  check(
    independent.view.binding.output.world.id === worlds[0]!.reference.id,
    "Session lifetime retargeted the surface",
  );
  pixel(independent, 70, 40, [255, 0, 0, 255]);

  const waiting = rejected(
    host.presentation.frame(view, { afterSequence: 0xffff_ffff_ffff_ffffn }),
    ["staleView"],
  );
  view = await host.presentation.select(surface, blueBinding);
  await waiting;
  const second = await capture(view);
  pixel(second, 8, 8, [0, 0, 255, 255]);
  check(
    second.sequence > first.sequence &&
      second.publication.revision !== first.publication.revision,
    "Different root capture reused the earlier frame stamp",
  );
  await host.presentation.clear(first.view);
  pixel(await capture(view), 70, 40, [0, 0, 255, 255]);
  const cycle = await host.presentation.select(surface, redBinding);
  pixel(await capture(cycle), 8, 8, [0, 255, 0, 255]);
  await rejected(host.presentation.frame(first.view), ["staleView"]);
  check(
    cycle.binding.generation.serial === first.view.binding.generation.serial &&
      cycle.selection !== first.view.selection,
    "Physical selection cycle revived an old view under the same root binding",
  );
  view = await host.presentation.select(surface, blueBinding);
  await rejected(host.presentation.frame(second.view), ["staleView"]);

  const sameBindingWait = rejected(
    host.presentation.capture(view, { afterSequence: 0xffff_ffff_ffff_ffffn }),
    ["staleView"],
  );
  const rebound = await host.setRootOutput(blueOutput, viewport);
  await sameBindingWait;
  await host.clearRootOutput(blueBinding);
  check(
    (await host.getRootOutputBinding(worlds[1]!.reference))?.generation
      .serial === rebound.generation.serial,
    "Stale root cleanup cleared equal-value replacement",
  );
  view = await host.presentation.select(surface, rebound);
  const reboundCapture = await capture(view);
  check(
    reboundCapture.view.binding.generation.serial !==
      second.view.binding.generation.serial,
    "Equal-value root rebind reused the generation",
  );
  successfulBatch(
    await camera.batch([
      createEntity(1, "healthy-peer-camera"),
      insertComponent(camera, "Camera", { kind: "alias", alias: 1 }),
    ]),
  );
  // A new committed World edit replaces the captured publication.
  successfulBatch(await blue.batch([createEntity(51, "changed")]));
  await rejected(
    host.presentation.capture(view, {
      publication: reboundCapture.publication,
    }),
    ["obsoletePublication"],
  );

  const resizeWait = rejected(
    host.presentation.frame(view, { afterSequence: 0xffff_ffff_ffff_ffffn }),
    ["staleView"],
  );
  const resized = await host.setRootOutput(blueOutput, {
    width: 80,
    height: 48,
    devicePixelRatio: 2,
  });
  await resizeWait;
  view = await host.presentation.select(surface, resized);
  const resizedCapture = await capture(view);
  pixel(resizedCapture, 79, 47, [0, 0, 255, 255]);
  check(
    resizedCapture.view.binding.viewport.devicePixelRatio === 2,
    "DPR fence changed silently",
  );
  const oversized = await host.setRootOutput(redOutput, {
    ...viewport,
    width: surface.maxWidth + 1,
  });
  await rejected(host.presentation.select(surface, oversized), [
    "invalidViewport",
  ]);
  pixel(await capture(view), 40, 24, [0, 0, 255, 255]);

  const clearWait = rejected(
    host.presentation.capture(view, { afterSequence: 0xffff_ffff_ffff_ffffn }),
    ["staleView"],
  );
  await host.presentation.clear(view);
  await clearWait;
  await rejected(host.presentation.frame(view), ["staleView"]);
  view = await host.presentation.select(surface, resized);
  const diagnostics = host.renderDiagnostics;
  check(diagnostics, "Real renderer lacks test diagnostics channel");
  const testing = presentationTesting(diagnostics);
  const lossWait = rejected(
    host.presentation.capture(view, { afterSequence: 0xffff_ffff_ffff_ffffn }),
    ["unavailable", "staleView", "drawFailed"],
  );
  testing.loseContext();
  await lossWait;
  check(
    (await host.getRootOutputBinding(worlds[1]!.reference))?.generation
      .serial === resized.generation.serial,
    "Context loss destroyed Host root configuration",
  );
  successfulBatch(await camera.batch([createEntity(2, "healthy-during-loss")]));
  testing.restoreContext();
  let restored = surface;
  for (let attempt = 0; attempt < 200; attempt++) {
    await new Promise((resolve) => setTimeout(resolve, 10));
    try {
      restored = await host.presentation.surface();
    } catch {
      continue;
    }
    if (restored.context !== surface.context) break;
  }
  check(
    restored.context !== surface.context,
    "Restored context reused its identity",
  );
  await rejected(host.presentation.frame(view), ["staleView"]);
  view = await host.presentation.select(restored, resized);
  pixel(await capture(view), 20, 20, [0, 0, 255, 255]);
  pixel(first, 8, 8, [0, 255, 0, 255]);
  pixel(first, 8, 56, [255, 0, 0, 255]);

  const largeBinding = await host.setRootOutput(blueOutput, {
    width: 192,
    height: 128,
    devicePixelRatio: 2,
  });
  const largeView = await host.presentation.select(restored, largeBinding);
  const held = probe.holdCaptureReply();
  const transferred = host.presentation.capture(largeView);
  await held;
  const captureId = probe.captured();
  const replacement = await host.setRootOutput(redOutput, viewport);
  await host.presentation.select(restored, replacement);
  testing.loseContext();
  for (let attempt = 0; attempt < 200; attempt++) {
    try {
      await host.presentation.surface();
    } catch {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  await rejected(host.presentation.surface(), ["unavailable"]);
  probe.releaseReply();
  const completed = await transferred;
  check(
    completed.view.surface.context === largeView.surface.context &&
      completed.view.selection === largeView.selection,
    "Completed capture was relabelled after switch/context loss",
  );
  pixel(completed, 180, 120, [0, 0, 255, 255]);
  check(
    probe.reads(captureId).length === 2 &&
      probe.reads(captureId)[0] === 0n &&
      probe.reads(captureId)[1] === 65536n &&
      probe.releases(captureId) === 1,
    "Capture chunks or exact transfer release lost their identity",
  );
  testing.restoreContext();
  const previousContext = restored.context;
  for (let attempt = 0; attempt < 200; attempt++) {
    await new Promise((resolve) => setTimeout(resolve, 10));
    try {
      restored = await host.presentation.surface();
    } catch {
      continue;
    }
    if (restored.context !== previousContext) break;
  }
  check(
    restored.context !== previousContext,
    "Second context restore reused a lifetime",
  );

  const cameraEntity = (await camera.inspect()).entities.find(
    (entity) => entity.metadata.symbolicId === "healthy-peer-camera",
  );
  check(cameraEntity, "Camera identity missing");
  const cameraOutput = await host.bindOutput(
    worlds[2]!.reference,
    cameraEntity.id,
    "camera",
  );
  const cameraBinding = await host.setRootOutput(cameraOutput, viewport);
  const cameraView = await host.presentation.select(restored, cameraBinding);
  const blank = await host.presentation.capture(cameraView);
  check(
    blank.drawCalls === 0 &&
      blank.failedDrawCalls === 0 &&
      blank.pixels.byteLength === 96 * 64 * 4,
    "A valid empty Camera was not a successful presentation",
  );
  successfulBatch(
    await camera.batch([
      insertComponent(
        camera,
        "Transform",
        { kind: "handle", id: cameraEntity.id },
        { x: 3e38 },
      ),
    ]),
  );
  await rejected(host.presentation.capture(cameraView), ["drawFailed"]);
  successfulBatch(
    await camera.batch([
      insertComponent(
        camera,
        "Transform",
        { kind: "handle", id: cameraEntity.id },
        { x: 0 },
      ),
    ]),
  );
  await host.presentation.frame(cameraView);
  const staleOutputWait = rejected(
    host.presentation.frame(cameraView, {
      afterSequence: 0xffff_ffff_ffff_ffffn,
    }),
    ["staleView"],
  );
  successfulBatch(
    await camera.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: cameraEntity.id },
        component: camera.components.Camera!.id,
      },
    ]),
  );
  await staleOutputWait;
  await rejected(host.presentation.capture(cameraView), ["staleView"]);
  const report = {
    surface,
    restored,
    first: {
      view: first.view,
      sequence: first.sequence,
      publication: first.publication,
    },
    second: {
      view: second.view,
      sequence: second.sequence,
      publication: second.publication,
    },
    blank: { sequence: blank.sequence, drawCalls: blank.drawCalls },
    transfer: {
      captureId,
      chunks: probe.reads(captureId),
      releases: probe.releases(captureId),
    },
    images: [first, second, completed, blank].map((image) => ({
      width: image.view.binding.viewport.width,
      height: image.view.binding.viewport.height,
      pixels: Array.from(new Uint8Array(image.pixels)),
      frame: {
        view: image.view,
        sequence: image.sequence,
        publication: image.publication,
      },
    })),
    statistics: await diagnostics.statistics(),
  };
  for (const client of clients) await client.close();
  for (const world of worlds) await host.destroyWorld(world.reference);
  return report;
}
