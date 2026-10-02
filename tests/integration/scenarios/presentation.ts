import {
  renderDiagnostics,
  type RenderStatisticsSnapshot,
} from "../../../packages/ipp-client/src/diagnostics.js";
import type {
  AssetWorldClient,
  Client,
  HostClientBase,
  PresentedCapture,
  PresentationView,
  Command,
} from "@ipp/client";
import {
  canvasOutput,
  sameOutputReference,
} from "../../../packages/ipp-client/src/references.js";
import { presentationTesting } from "../../../packages/ipp-client/src/testing.js";
export { nativePresentationTransport } from "../../../packages/ipp-client/src/native-presentation.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import type { PresentationTransferProbe } from "../presentation-transport.js";
import {
  ATTACHMENTS,
  CAMERA,
  CANVAS,
  SURFACE,
  selectSystems,
} from "../system-selections.js";
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

function capturedImage(image: PresentedCapture) {
  return {
    width: image.view.binding.viewport.width,
    height: image.view.binding.viewport.height,
    pixels: Array.from(new Uint8Array(image.pixels)),
    frame: {
      view: image.view,
      sequence: image.sequence,
      publication: image.publication,
    },
  };
}

/**
 * Selection, completed captures and their fences through the production
 * presentation protocol; an instrumentation build continues with context
 * loss, held capture transfers and render statistics through the testing channel.
 */
export async function explicitPresentation(
  host: HostClientBase<Client>,
  probe: PresentationTransferProbe,
  instrumentation = true,
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

  // One request rebinds the selected output and selects the new binding.
  const resizeWait = rejected(
    host.presentation.frame(view, { afterSequence: 0xffff_ffff_ffff_ffffn }),
    ["staleView"],
  );
  const beforeResize = view;
  view = await host.presentation.resize(view, {
    width: 80,
    height: 48,
    devicePixelRatio: 2,
  });
  const resized = view.binding;
  await resizeWait;
  check(
    view.selection !== beforeResize.selection &&
      resized.generation.serial !== beforeResize.binding.generation.serial &&
      (await host.getRootOutputBinding(worlds[1]!.reference))?.generation
        .serial === resized.generation.serial,
    "Resize did not answer a fresh selection of a fresh root binding",
  );
  await rejected(host.presentation.resize(beforeResize, viewport), [
    "staleView",
  ]);
  await rejected(
    host.presentation.resize(view, {
      ...viewport,
      width: surface.maxWidth + 1,
    }),
    ["invalidViewport"],
  );
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
  async function close() {
    for (const client of clients) await client.close();
    for (const world of worlds) await host.destroyWorld(world.reference);
  }
  if (!instrumentation) {
    // Statistics answer in every build; a testing control fails at the call.
    check(
      (await renderDiagnostics(host)?.statistics())?.frame,
      "The production render build answers renderer statistics",
    );
    let refused: unknown;
    try {
      presentationTesting(host).loseContext();
    } catch (error) {
      refused = error;
    }
    check(
      refused instanceof Error &&
        /require an instrumentation build/.test(refused.message),
      `A testing control must fail at the call against the production build: ${String(refused)}`,
    );
    const report = {
      surface,
      first: { sequence: first.sequence, publication: first.publication },
      second: { sequence: second.sequence, publication: second.publication },
      images: [first, second, resizedCapture].map(capturedImage),
    };
    await close();
    return report;
  }
  const renderer = renderDiagnostics(host);
  check(renderer, "Real renderer lacks test diagnostics channel");
  const testing = presentationTesting(host);
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

  const largeView = await host.presentation.resize(view, {
    width: 192,
    height: 128,
    devicePixelRatio: 2,
  });
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
    images: [first, second, completed, blank].map(capturedImage),
    statistics: await renderer.statistics(),
  };
  await close();
  return report;
}

/** Renderer work that releasing and rebuilding a presented output's state repeats. */
function retainedWork(statistics: RenderStatisticsSnapshot) {
  return {
    cacheRepaints: statistics.surfaces.totalSurfaceCacheRepaints,
    cacheAllocations: statistics.surfaces.totalSurfaceCacheAllocations,
    guiRebuilds: statistics.gui.totalGuiRebuilds,
    guiAllocations: statistics.gui.totalGuiAllocations,
    glyphPopulates: statistics.gui.totalGlyphPopulates,
  };
}

type RetainedWork = ReturnType<typeof retainedWork>;

function workDelta(before: RetainedWork, after: RetainedWork): RetainedWork {
  return {
    cacheRepaints: after.cacheRepaints - before.cacheRepaints,
    cacheAllocations: after.cacheAllocations - before.cacheAllocations,
    guiRebuilds: after.guiRebuilds - before.guiRebuilds,
    guiAllocations: after.guiAllocations - before.guiAllocations,
    glyphPopulates: after.glyphPopulates - before.glyphPopulates,
  };
}

const NO_WORK: RetainedWork = {
  cacheRepaints: 0,
  cacheAllocations: 0,
  guiRebuilds: 0,
  guiAllocations: 0,
  glyphPopulates: 0,
};

/** JSON text of diagnostic values, which carry bigint identities. */
function describe(value: unknown): string {
  return JSON.stringify(value, (_, field: unknown) =>
    typeof field === "bigint" ? field.toString() : field,
  );
}

/** Light text pixels inside a rectangle of a top-left RGBA8 capture. */
function lightPixels(
  capture: PresentedCapture,
  [left, top, right, bottom]: readonly [number, number, number, number],
) {
  const { width } = capture.view.binding.viewport;
  const pixels = new Uint8Array(capture.pixels);
  let count = 0;
  for (let y = top; y < bottom; y++)
    for (let x = left; x < right; x++) {
      const at = (y * width + x) * 4;
      if (pixels[at]! > 160 && pixels[at + 1]! > 160 && pixels[at + 2]! > 160)
        count++;
    }
  return count;
}

/** A canvas World with a filled box and a label in its own font. */
async function labelledCanvas(
  host: HostClientBase<Client>,
  symbolicId: string,
  extent: readonly [number, number],
  fill: readonly [number, number, number],
  text: string,
  fontBytes: ArrayBuffer,
) {
  const world = await host.createWorld({
    selectedSystems: selectSystems(CANVAS),
    symbolicId,
    canvas: { extent, unitsPerMetre: 96 },
  });
  const client = await host.openWorld(world.reference);
  const font = await (client as unknown as AssetWorldClient).createAsset(
    17,
    fontBytes,
  );
  const box = { kind: "alias", alias: 1 } as const;
  const label = { kind: "alias", alias: 2 } as const;
  successfulBatch(
    await client.batch([
      createEntity(1, `${symbolicId}-fill`),
      insertComponent(client, "CanvasBox", box, {
        width: extent[0],
        height: extent[1],
      }),
      insertComponent(client, "CanvasStyle", box, {
        red: fill[0],
        green: fill[1],
        blue: fill[2],
      }),
      createEntity(2, `${symbolicId}-label`),
      insertComponent(client, "CanvasStyle", label, { x: 4, y: 4 }),
      insertComponent(client, "CanvasText", label, {
        text,
        source: font.source,
        font_size: 16,
      }),
    ]),
  );
  return { world, client };
}

/**
 * A presented root keeps the renderer's retained state across viewport
 * resizes. The root canvas presents two child canvases in slots at 96 units
 * per metre: one through a Surface cache image, one directly from retained
 * GUI batches and glyph streams. Each `presentation.resize` rebinds and
 * reselects the root in one request, so successive resizes draw at the new
 * extent without a cache repaint or allocation, a retained batch rebuild or a
 * glyph population. A separate rebind and an explicit clear each leave a
 * frame with nothing selected, which releases that state and repeats the work.
 *
 * The labels sit in child canvases because a root canvas's own content is
 * clipped to its extent, which a resize changes.
 */
export async function retainedResize(
  host: HostClientBase<Client>,
  fontBytes: ArrayBuffer,
) {
  const diagnostics = renderDiagnostics(host);
  check(diagnostics, "Real render diagnostics absent");
  const parentWorld = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CANVAS, SURFACE),
    symbolicId: "resized-root",
    canvas: { extent: [96, 96], unitsPerMetre: 96 },
  });
  const parent = await host.openWorld(parentWorld.reference);
  const cached = await labelledCanvas(
    host,
    "resized-cached",
    [96, 64],
    [1, 0, 0],
    "CACHED",
    fontBytes,
  );
  const direct = await labelledCanvas(
    host,
    "resized-direct",
    [96, 24],
    [0, 0, 1],
    "DIRECT",
    fontBytes,
  );
  let view: PresentationView | undefined;
  try {
    const attachment = parent.components.WorldAttachment!;
    const slot = (
      alias: number,
      name: string,
      child: { world: { reference: typeof parentWorld.reference } },
      height: number,
      y: number,
    ): Command[] => {
      const entity = { kind: "alias", alias } as const;
      return [
        createEntity(alias, name),
        insertComponent(parent, "CanvasStyle", entity, { y }),
        insertComponent(parent, "Surface", entity, {
          width: 1,
          height: height / 96,
        }),
        {
          kind: "insertComponent",
          entity,
          component: attachment.id,
          fields: [
            {
              offset: attachment.fields.mode!.offset,
              value: { kind: "u32", value: 1 },
            },
            {
              offset: attachment.fields.child!.offset,
              value: { kind: "world", value: child.world.reference },
            },
          ],
        },
      ];
    };
    successfulBatch(
      await parent.batch([
        ...slot(1, "cached-slot", cached, 64, 0),
        insertComponent(
          parent,
          "SurfaceCache",
          { kind: "alias", alias: 1 },
          {
            direct_distance: 0,
            texels_per_metre: 96,
            max_refresh_hz: 1,
          },
        ),
        ...slot(2, "direct-slot", direct, 24, 72),
      ]),
    );
    const root = canvasOutput(parentWorld.reference);
    const afterOutputs = [
      root,
      canvasOutput(cached.world.reference),
      canvasOutput(direct.world.reference),
    ];
    const surface = await host.presentation.surface();
    view = await host.presentation.select(
      surface,
      await host.setRootOutput(root, {
        width: 160,
        height: 112,
        devicePixelRatio: 1,
      }),
    );

    /** Capture until the cached image is reused and two frames do equal work. */
    async function settled(current: PresentationView) {
      let image = await host.presentation.capture(current, { afterOutputs });
      let previous: RetainedWork | undefined;
      for (let frame = 0; frame < 240; frame++) {
        const statistics = await diagnostics!.statistics();
        const work = retainedWork(statistics);
        const [cache] = statistics.surfaces.surfaceCaches;
        if (
          previous &&
          JSON.stringify(previous) === JSON.stringify(work) &&
          image.failedDrawCalls === 0 &&
          labelled(image) &&
          statistics.gui.glyphPages > 0 &&
          statistics.surfaces.surfaceCaches.length === 1 &&
          cache?.mode === "reused"
        )
          return { image, work, statistics };
        previous = work;
        image = await host.presentation.capture(current, {
          afterSequence: image.sequence,
        });
      }
      throw new Error("Resized root presentation never settled");
    }

    /** Both labels are drawn: the cached one and the direct one below it. */
    function labelled(image: PresentedCapture) {
      return (
        lightPixels(image, [4, 4, 92, 24]) > 20 &&
        lightPixels(image, [4, 76, 92, 96]) > 20
      );
    }

    function drawn(image: PresentedCapture, width: number, height: number) {
      check(
        image.view.binding.viewport.width === width &&
          image.view.binding.viewport.height === height &&
          image.pixels.byteLength === width * height * 4,
        `Capture is not ${width} x ${height}`,
      );
      // Both children's fills and labels are drawn at every extent.
      pixel(image, 88, 56, [255, 0, 0, 255]);
      pixel(image, 90, 92, [0, 0, 255, 255]);
      check(labelled(image), "A label is missing");
    }

    const initial = await settled(view);
    drawn(initial.image, 160, 112);
    const sizes = [
      [200, 128],
      [176, 144],
      [224, 120],
      [160, 112],
    ] as const;
    const resizes = [];
    const images = [initial.image];
    let previous = initial;
    for (const [width, height] of sizes) {
      const replaced = view;
      const staleWait = rejected(
        host.presentation.frame(replaced, {
          afterSequence: 0xffff_ffff_ffff_ffffn,
        }),
        ["staleView"],
      );
      view = await host.presentation.resize(replaced, {
        width,
        height,
        devicePixelRatio: 1,
      });
      await staleWait;
      check(
        sameOutputReference(view.binding.output, root) &&
          view.selection > replaced.selection &&
          view.binding.generation.serial > replaced.binding.generation.serial &&
          view.surface.context === replaced.surface.context,
        "Resize did not answer a fresh view of the same output",
      );
      const image = await host.presentation.capture(view, { afterOutputs });
      const later = await host.presentation.capture(view, {
        afterSequence: image.sequence,
      });
      const statistics = await diagnostics.statistics();
      const work = retainedWork(statistics);
      drawn(image, width, height);
      drawn(later, width, height);
      const delta = workDelta(previous.work, work);
      check(
        JSON.stringify(delta) === JSON.stringify(NO_WORK) &&
          statistics.surfaces.surfaceCaches[0]?.mode === "reused",
        `Resizing to ${width} x ${height} repeated retained work: ${describe({ delta, cache: statistics.surfaces.surfaceCaches })}`,
      );
      resizes.push({ width, height, selection: view.selection, delta });
      images.push(image);
      previous = { image, work, statistics };
    }

    // A separate rebind leaves the surface unselected for a frame, so the
    // renderer releases the root's state and the next selection rebuilds it.
    const rebound = await host.setRootOutput(root, view.binding.viewport);
    view = await host.presentation.select(surface, rebound);
    const afterRebind = await settled(view);
    const rebindDelta = workDelta(previous.work, afterRebind.work);
    // An explicit clear releases it too.
    await host.presentation.clear(view);
    view = await host.presentation.select(surface, rebound);
    const afterClear = await settled(view);
    const clearDelta = workDelta(afterRebind.work, afterClear.work);
    for (const [name, delta] of [
      ["rebind", rebindDelta],
      ["clear", clearDelta],
    ] as const)
      check(
        delta.cacheAllocations > 0 &&
          delta.cacheRepaints > 0 &&
          delta.guiRebuilds > 0 &&
          delta.guiAllocations > 0 &&
          delta.glyphPopulates > 0,
        `A ${name} deselection kept the root's retained state: ${describe(delta)}`,
      );
    drawn(afterClear.image, 160, 112);
    return {
      initial: initial.work,
      resizes,
      rebindDelta,
      clearDelta,
      images: [...images, afterRebind.image, afterClear.image].map(
        capturedImage,
      ),
    };
  } finally {
    if (view) await host.presentation.clear(view).catch(() => {});
    await Promise.all([
      parent.close(),
      cached.client.close(),
      direct.client.close(),
    ]);
    for (const world of [parentWorld, cached.world, direct.world])
      await host.destroyWorld(world.reference);
  }
}
