/** Observations of the public application; all input uses the real browser adapter. */
import type { DemoApplication } from "../../../examples/demos/application.js";
import type { IppCanvasHandle } from "@ipp/react/web";
import { capturedImage } from "../../harness/page/presentation.js";
import {
  compareImages,
  rgbaDataUrl,
  summarizeImage,
} from "../../harness/page/images.js";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import { hostProfiling } from "../../../packages/ipp-client/src/profiling.js";
import type { Inspection, WorldReference } from "@ipp/client";

declare global {
  interface Window {
    ippDemoApplication?: DemoApplication;
    ippDemoCanvas?: IppCanvasHandle | undefined;
  }
}

const captures = new Map<string, ReturnType<typeof capturedImage>>();
function application() {
  const app = window.ippDemoApplication;
  if (!app) throw new Error("The demo application is not mounted");
  return app;
}
function canvas() {
  const current = window.ippDemoCanvas;
  if (!current) throw new Error("The active scene Canvas is not mounted");
  return current;
}
export async function settle() {
  await application().ready;
  await application().flush();
  await canvas().frame();
}
export async function state() {
  await settle();
  const app = application();
  const scene = await app.sceneClient!.inspect();
  const inspection = await app.inspectApplication();
  const output = app.sceneOutput;
  const camera = scene.entities.find(
    (entity) => output?.kind === "camera" && entity.id === output.entity,
  );
  return {
    ...inspection,
    scene,
    worlds: await canvas().host.listWorlds(),
    camera: camera?.components.find(
      (component) =>
        component.component === app.sceneClient!.components.Transform?.id,
    )?.fields,
    viewport: canvas().viewport,
  };
}
export async function point(id: string) {
  await settle();
  const app = application();
  const inspection = await app.inspectApplication();
  const root = await canvas().host.openWorld(inspection.cameraPath[0]!.parent);
  try {
    const deadline = performance.now() + 30_000;
    let current = inspection;
    for (;;) {
      const entity = current.shell.entities.find(
        (entity) => entity.metadata.symbolicId === id,
      );
      const fields = entity?.components.find(
        (component) => component.component === root.components.CanvasBounds?.id,
      )?.fields;
      const behavior = entity?.components.find(
        (component) => component.component === root.components.GuiBehavior?.id,
      )?.fields;
      // A completed frame can precede queued DOM pointer-up admission. Wait for
      // the previous click's native popup before targeting its retained children.
      if (
        behavior?.effective_visible === true &&
        behavior.effective_enabled === true &&
        behavior.available === true &&
        fields &&
        Number(fields.width) > 0 &&
        Number(fields.height) > 0
      ) {
        const viewport = canvas().viewport!;
        return {
          x:
            ((Number(fields.x) + Number(fields.width) / 2) *
              viewport.devicePixelRatio) /
            viewport.width,
          y:
            ((Number(fields.y) + Number(fields.height) / 2) *
              viewport.devicePixelRatio) /
            viewport.height,
          bounds: fields,
        };
      }
      if (performance.now() >= deadline)
        throw new Error(`Native GUI target is not visible and enabled: ${id}`);
      await settle();
      current = await app.inspectApplication();
    }
  } finally {
    await root.close();
  }
}
export async function action(name: string, value?: unknown) {
  await application().action(name, value);
  await settle();
}
export async function inspect() {
  await settle();
  return application().sceneClient!.inspect();
}

/** Observe the robot's actual projected control hits, independently of camera math. */
export async function platformerControls() {
  await settle();
  const app = application();
  const world = (app.controller as { movementWorld?: WorldReference })
    .movementWorld;
  if (!world) throw new Error("Robot movement World is not ready");
  const client = await canvas().host.openWorld(world);
  try {
    const panel = await client.inspect();
    const feedback = await client.inspectPage({ collection: "guiPointers" });
    const hovered = new Set(
      feedback.guiPointers
        ?.filter((record) => record.state.hovered)
        .map((record) => record.target.entity),
    );
    return {
      scene: await app.sceneClient!.inspect(),
      panel,
      options: app.options,
      componentIds: Object.fromEntries(
        Object.entries(client.components).map(([name, descriptor]) => [
          name,
          descriptor!.id,
        ]),
      ),
      hovered: panel.entities
        .filter((entity) => hovered.has(entity.id))
        .map((entity) => entity.metadata.symbolicId),
    };
  } finally {
    await client.close();
  }
}

/** Projected widget input uses evaluated scene and Canvas geometry, including the rotor. */
export async function cockpitGeometry() {
  await settle();
  const app = application();
  const shell = await app.inspectApplication();
  const anchor = shell.shell.entities.find(
    (entity) => entity.id === shell.cameraPath[0]?.anchor,
  );
  const placement = anchor?.components.find(
    (component) =>
      component.component === app.sceneClient!.components.CanvasStyle!.id,
  )?.fields;
  const surface = anchor?.components.find(
    (component) =>
      component.component === app.sceneClient!.components.FlatSurface!.id,
  )?.fields;
  if (!placement || !surface)
    throw new Error("The scene Camera Surface is absent");
  const snapshot = (await app.inspect()) as {
    scene: Inspection;
    panels: (Inspection & { world: WorldReference })[];
  };
  const panels = await Promise.all(
    snapshot.panels.map(async (panel) => {
      const client = await canvas().host.openWorld(panel.world);
      try {
        return {
          ...panel,
          canvas:
            (await client.inspectPage({ collection: "canvas" })).canvas ?? null,
        };
      } finally {
        await client.close();
      }
    }),
  );
  return {
    ...snapshot,
    panels,
    cameraSurface: {
      x: Number(placement.x),
      y: Number(placement.y),
      width: Number(surface.width),
      height: Number(surface.height),
    },
    componentIds: Object.fromEntries(
      Object.entries(app.sceneClient!.components).map(([name, descriptor]) => [
        name,
        descriptor!.id,
      ]),
    ),
    viewport: canvas().viewport!,
  };
}

export async function options(patch: Readonly<Record<string, unknown>>) {
  await application().update(patch);
  await settle();
}

/** Browser pane state without opening six extra Canvas observation sessions. */
export async function cockpitBrowserState() {
  await settle();
  const snapshot = (await application().inspect()) as { panels: Inspection[] };
  const entities = snapshot.panels[3]?.entities ?? [];
  const field = (id: string, name: string) =>
    String(
      entities
        .find((entity) => entity.metadata.symbolicId === id)
        ?.components.find((component) => name in component.fields)?.fields[
        name
      ] ?? "",
    );
  return {
    source: field("cockpit-browser-page", "source"),
    status: field("cockpit-browser-status", "text"),
    address: field("cockpit-browser-address/input", "text"),
    domSources: document.querySelectorAll("canvas > [drawable]").length,
  };
}
export async function capture(label: string) {
  await settle();
  const frame = await canvas().capture();
  const image = capturedImage(frame);
  captures.set(label, image);
  const pixels = new Uint8Array(image.pixels);
  let scenePixels = 0;
  for (let y = Math.floor(image.height / 3); y < image.height; y++)
    for (let x = 0; x < image.width; x++) {
      const offset = (y * image.width + x) * 4;
      if (
        [0, 1, 2].some(
          (axis) => Math.abs(pixels[offset + axis]! - pixels[axis]!) > 24,
        )
      )
        scenePixels++;
    }
  return {
    frame: {
      width: image.width,
      height: image.height,
      drawCalls: frame.drawCalls,
      triangles: frame.triangles,
    },
    summary: summarizeImage(image),
    scenePixels,
    dataUrl: rgbaDataUrl(image),
  };
}
export function difference(before: string, after: string) {
  return compareImages(captures.get(before)!, captures.get(after)!);
}
/** Raw captured output for shared renderer assertions; never reads the live canvas. */
export function image() {
  const current = [...captures.values()].at(-1);
  if (!current) throw new Error("No application frame was captured");
  return { ...current, pixels: new Uint8Array(current.pixels) };
}
export async function startTrace(maxEvents: number, maxArtifactBytes: number) {
  return hostProfiling(canvas().host).start({
    counters: false,
    trace: { maxEvents },
    maxArtifactBytes,
  });
}
export async function startProfile() {
  return hostProfiling(canvas().host).start({
    counters: true,
    gpu: "passes",
    glCalls: true,
  });
}
export async function stopTrace() {
  return hostProfiling(canvas().host).stop();
}
export async function cancelTrace(id: string) {
  await hostProfiling(canvas().host).release(id);
}

let cleanup: { worlds: bigint[]; error?: string } | undefined;
export function observeCleanup() {
  const app = application();
  const host = canvas().host;
  const original = app.dispose.bind(app);
  app.dispose = async () => {
    try {
      await original();
      cleanup = { worlds: (await host.listWorlds()).map((world) => world.id) };
    } catch (error) {
      cleanup = { worlds: [], error: String(error) };
      throw error;
    }
  };
}
export function cleanupState() {
  return cleanup;
}

// Bounded failure evidence observes real adapter calls without awaiting or
// replacing their admission promises.
const inputTraces = new WeakMap<DemoApplication, Record<string, unknown>[]>();
export function startCockpitInputTrace() {
  const app = application();
  if (inputTraces.has(app)) return;
  const trace: Record<string, unknown>[] = [];
  inputTraces.set(app, trace);
  const record = (entry: Record<string, unknown>) => {
    trace.push({
      at: performance.now(),
      capturing: app.capturingSceneInput,
      carousel: app.options.carousel,
      dragging: app.options.carouselDragging,
      ...entry,
    });
    if (trace.length > 64) trace.shift();
  };
  const handle = app.handlePhysicalInput.bind(app);
  app.handlePhysicalInput = (input, outcome) => {
    record({ stage: "input", input, outcome });
    const pending = handle(input, outcome);
    void pending.then(
      () => record({ stage: "completed", kind: input.kind }),
      (error) =>
        record({ stage: "failed", kind: input.kind, error: String(error) }),
    );
    return pending;
  };
  const sceneInput = app.scene?.sceneInput;
  if (sceneInput) {
    const cancel = sceneInput.cancel.bind(sceneInput);
    sceneInput.cancel = () => {
      record({
        stage: "scene-cancel",
        stack: new Error().stack?.split("\n").slice(1, 5),
      });
      cancel();
    };
  }
  const element =
    document.querySelector<HTMLCanvasElement>("#ipp-demo-canvas")!;
  for (const kind of [
    "pointerdown",
    "pointercancel",
    "lostpointercapture",
    "blur",
  ]) {
    element.addEventListener(kind, (event) =>
      record({
        stage: "dom",
        kind: event.type,
        pointer: "pointerId" in event ? event.pointerId : undefined,
        captured: element.hasPointerCapture(1),
        active: document.activeElement?.tagName,
      }),
    );
  }
}

export async function cockpitInputDiagnostics() {
  const app = application();
  const controller = app.controller as {
    panels: readonly { index: number; world: WorldReference }[];
  };
  const pointers = await Promise.all(
    controller.panels.slice(0, 6).map(async ({ index, world }) => {
      const client = await canvas().host.openWorld(world);
      try {
        const records = (
          await client.inspectPage({ collection: "guiPointers" })
        ).guiPointers;
        return { index, world, records };
      } finally {
        await client.close();
      }
    }),
  );
  return {
    trace: inputTraces.get(app),
    pointers,
    options: app.options,
    capturing: app.capturingSceneInput,
    active: document.activeElement?.tagName,
    captured: document
      .querySelector<HTMLCanvasElement>("#ipp-demo-canvas")!
      .hasPointerCapture(1),
  };
}

/** Read-only image sizing evidence after a completed frame. */
export async function renderStatistics() {
  return renderDiagnostics(canvas().host)!.statistics();
}
