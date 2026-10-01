import type { RenderStatisticsSnapshot } from "@ipp/client/diagnostics";
import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import {
  nativePresentationTransport,
  presentationTesting,
  type GlyphAtlasLimits,
} from "../../packages/ipp-client/src/testing.js";
import {
  createRoot,
  Entity,
  Surface,
  SurfaceCache,
  type AttachedWorldHandle,
  type ReactWorldRoot,
  type SurfaceCacheProps,
} from "@ipp/react";
import { CanvasWorldSession } from "@ipp/react/web";
import { Drawing, Style } from "@ipp/react/gui";
import {
  terminalWorkloadLayers,
  type TerminalGlyphRowsCodec,
  type TerminalWorkload,
} from "../../examples/surface-terminal/workload.js";
import { clientAssetSource, entityLocalToSurfaceContent } from "@ipp/client";
import { compareGlyph } from "./surface-glyph-oracle.js";
import { probeErrorCheckBridge } from "./error-check-bridge.js";
import { probeSurfaceCacheBridge } from "./surface-cache-bridge.js";
import type {
  OutputReference,
  PresentedCapture,
  PickingWorldClient,
  RenderWorldClient,
  WorldPersistenceHostClient,
} from "@ipp/client";
import type { ReactNode } from "react";
import {
  Terminal,
  terminalLayers,
  TERMINAL_TEXT,
  type TerminalAssets,
} from "../../examples/surface-terminal/scene.js";
import {
  componentFields,
  createFixtureCamera,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import {
  exerciseSurfaceLifecycle,
  surfaceCachePolicy,
  canvasSnapshot,
  waitSurfaceAssets,
  type SurfaceTestClient,
} from "../integration/surface-scenario.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  CAMERA,
  SURFACE,
  selectSystems,
} from "../integration/system-selections.js";

type Client = SurfaceTestClient & RenderWorldClient & PickingWorldClient;
let host: WorldPersistenceHostClient<Client>;
let client: Client;
let presentation: CanvasWorldSession;
let cameraOutput: OutputReference;
let root: ReactWorldRoot;
let assets: TerminalAssets;
let glyphCodec: TerminalGlyphRowsCodec;
let camera: bigint;
let metrics: { units: number; ascender: number; line: number };
let workloadGlyphs: number[];
let unseenGlyphs: number[];
const frames = new Map<string, PresentedCapture>();
const ORTHO_HEIGHT = 3;
const references = new Map<string, HTMLCanvasElement>();
const failures: unknown[] = [];
const observedCanvasClients = new Set<SurfaceTestClient>();
const canvasSessions = new Map<string, Promise<SurfaceTestClient>>();
let lastSequence: bigint | undefined;

async function resizePresentation(
  width: number,
  height: number,
  devicePixelRatio = 1,
) {
  await presentation.selectOutput(cameraOutput, {
    width,
    height,
    devicePixelRatio,
  });
}

function rendererDiagnostics() {
  const diagnostics = renderDiagnostics(host);
  if (!diagnostics) throw new Error("Render diagnostics are unavailable");
  return diagnostics;
}

/** Authored SurfaceCache policies of cache scenario Surfaces, by symbolic id. */
const cachePolicies = new Map<string, SurfaceCachePolicy>();

/** The latest cache-aware scene, rebuilt when a policy changes. */
let cacheScene: (() => ReactNode) | null = null;

/** Present a scene that declares no cache policies. */
function present(scene: ReactNode) {
  cacheScene = null;
  return root.render(scene);
}

/** Present a scene whose Surfaces declare their current `cachePolicies`. */
function presentCached(scene: () => ReactNode) {
  cacheScene = scene;
  return root.render(scene());
}

/** The React SurfaceCache declaration of one scenario Surface, if any. */
function cacheDeclaration(symbolicId: string) {
  const policy = cachePolicies.get(symbolicId);
  return policy ? <SurfaceCache {...policy} /> : null;
}

function observeCanvas(anchor: string) {
  return (handle: AttachedWorldHandle) => {
    let tracked: Promise<SurfaceTestClient>;
    tracked = host.openWorld(handle.world).then((session) => {
      observedCanvasClients.add(session);
      const release = async () => {
        observedCanvasClients.delete(session);
        if (!session.closure) await session.close();
        if (canvasSessions.get(anchor) === tracked)
          canvasSessions.delete(anchor);
      };
      void handle.closed.then(release, release).catch(() => {});
      return session;
    });
    canvasSessions.set(anchor, tracked);
    void tracked.catch(() => {});
  };
}

async function canvasSession(anchor = "surface-terminal") {
  const session = canvasSessions.get(anchor);
  if (!session) throw new Error(`No attached Canvas session for ${anchor}`);
  return session;
}

async function canvasRoot(client: SurfaceTestClient) {
  const root = (await client.inspect()).entities.find(
    (entity) => entity.metadata.symbolicId === "canvas",
  );
  if (!root) throw new Error("Attached Canvas root disappeared");
  return root.id;
}

async function terminalCanvasSnapshot() {
  const session = await canvasSession();
  return canvasSnapshot(session, await canvasRoot(session));
}

async function terminalCanvasChildren() {
  const session = await canvasSession();
  const root = await canvasRoot(session);
  return (await session.inspect()).entities
    .filter((entity) => entity.link.parent === root)
    .sort((left, right) =>
      left.link.order < right.link.order
        ? -1
        : left.link.order > right.link.order
          ? 1
          : 0,
    );
}

function canvasDrawingEntity(
  id: string,
  source: TerminalAssets["panel"],
  position: readonly [number, number],
  scale: readonly [number, number],
  color: readonly [number, number, number, number],
): ReactNode {
  return (
    <Entity key={id} id={id}>
      <Style
        x={position[0]}
        y={position[1]}
        scale_x={scale[0]}
        scale_y={scale[1]}
        red={color[0]}
        green={color[1]}
        blue={color[2]}
        alpha={color[3]}
      />
      <Drawing source={source.source} />
    </Entity>
  );
}

/**
 * Connect through a worker presenting on a page canvas, or with `nativeHost`
 * to a native GLES testing host and its presentation channel.
 */
export async function initialize(
  config: { generatedModuleUrl: string } & (
    | { workerScriptUrl: string; wasmUrl: string }
    | { nativeHost: { url: string; presentationUrl: string } }
  ),
) {
  const contract = await import(config.generatedModuleUrl);
  if ("nativeHost" in config) {
    host = await contract.IppHostClient.connectTransport(
      nativePresentationTransport(
        config.nativeHost.url,
        config.nativeHost.presentationUrl,
      ),
      { timeoutMs: 20000 },
    );
  } else {
    const canvas = document.createElement("canvas");
    canvas.width = 320;
    canvas.height = 240;
    document.body.replaceChildren(canvas);
    host = await contract.IppHostClient.connectWorker(
      config.workerScriptUrl,
      config.wasmUrl,
      { canvas: canvas.transferControlToOffscreen(), timeoutMs: 20000 },
    );
  }
  const created = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CAMERA, SURFACE, LIFECYCLE),
    symbolicId: "surface-rendering",
  });
  client = await host.openWorld(created.reference);
  client.onRuntimeFailure((failure) => {
    failures.push(failure);
    console.error("Surface runtime failure", failure.message);
  });
  camera = await createFixtureCamera(client);
  for (const [component, values] of [
    ["Transform", { x: 0, y: 0, z: 6, qx: 0, qy: 0, qz: 0, qw: 1 }],
    ["Camera", { projection: 1, ortho_height: ORTHO_HEIGHT }],
  ] as const) {
    successfulBatch(
      await client.batch(
        componentFields(client, component, values).map((field) => ({
          kind: "setField",
          entity: { kind: "handle", id: camera },
          component: client.components[component]!.id,
          field,
        })),
      ),
    );
  }
  const load = async (kind: number, path: string) => {
    const response = await fetch(path);
    if (!response.ok) throw new Error(`Missing Surface fixture ${path}`);
    const bytes = await response.arrayBuffer();
    if (kind === 17) {
      const view = new DataView(bytes);
      metrics = {
        units: view.getUint32(8, true),
        ascender: view.getFloat32(12, true),
        line:
          view.getFloat32(12, true) -
          view.getFloat32(16, true) +
          view.getFloat32(20, true),
      };
    }
    return client.createAsset(kind, bytes);
  };
  const [font, panel, icon, bitmap] = await Promise.all([
    load(17, "/target/font-assets/shure-tech-mono.ippf"),
    load(18, "/target/surface-assets/panel.ippd"),
    load(18, "/target/surface-assets/icon.ippd"),
    load(2, "/target/surface-assets/badge.ippt"),
  ]);
  assets = { font, panel, icon, bitmap };
  const glyphs = await (
    await fetch("/target/surface-assets/glyphs.json")
  ).json();
  workloadGlyphs = Object.values(glyphs);
  unseenGlyphs = await (
    await fetch("/target/surface-assets/unseen-glyphs.json")
  ).json();
  const lifecycle = await exerciseSurfaceLifecycle(
    host,
    client,
    assets,
    glyphs.A,
    { encodeRowsTable: contract.encodeRowsTable },
  );
  const glyphRows =
    lifecycle.canvasClient.components.CanvasGlyphRun?.fields.glyphs?.rows;
  if (!glyphRows) throw new Error("CanvasGlyphRun omitted its rows layout");
  glyphCodec = {
    layout: glyphRows,
    encodeRowsTable: contract.encodeRowsTable,
  };
  await lifecycle.canvasClient.close();
  successfulBatch(
    await client.batch([
      { kind: "delete", entity: { kind: "handle", id: lifecycle.entity } },
    ]),
  );
  await client.waitForFrame();
  await host.destroyWorld(lifecycle.canvasWorld);
  presentation = new CanvasWorldSession({ host, client });
  root = presentation.createRoot();
  cameraOutput = await host.bindOutput(created.reference, camera, "camera");
  await present(
    <Terminal assets={assets} onWorld={observeCanvas("surface-terminal")}>
      {terminalLayers(assets)}
    </Terminal>,
  );
  await resizePresentation(320, 240);
  const initialCanvasClient = await canvasSession();
  await waitSurfaceAssets(
    initialCanvasClient,
    Object.values(assets).map((asset) => asset.source),
  );
  const fontFace = new FontFace(
    "SurfaceOracle",
    await (
      await fetch("/target/font-sources/shure-tech-mono.ttf")
    ).arrayBuffer(),
  );
  document.fonts.add(await fontFace.load());
  return {
    children: (await terminalCanvasSnapshot()).children.map(
      (item) => item.symbolicId,
    ),
  };
}

/**
 * Capture the frame presenting the latest inspected state, or with `next` the
 * next completed frame, which observes work a following frame would finish.
 */
export async function capture(label: string, options: { next?: boolean } = {}) {
  const captureStarted = performance.now();
  const update = pendingUpdate;
  pendingUpdate = undefined;
  if (failures.length)
    throw new Error(
      `Surface runtime failures: ${JSON.stringify(failures, (_, value) => (typeof value === "bigint" ? String(value) : value))}`,
    );
  const frame = await presentation.capture({
    afterOutputs: [cameraOutput],
    ...(options.next && lastSequence !== undefined
      ? { afterSequence: lastSequence }
      : {}),
  });
  lastSequence = frame.sequence;
  const presentedAt = performance.now();
  frames.set(label, frame);
  const pixels = new Uint8Array(frame.pixels);
  const { width, height, devicePixelRatio } = frame.view.binding.viewport;
  const statistics = await rendererDiagnostics().statistics();
  let textPixels = 0;
  for (let y = Math.round(height * 0.12); y < height * 0.65; y++) {
    for (let x = 12; x < width * 0.88; x++) {
      const offset = (y * width + x) * 4;
      if (
        pixels[offset + 1]! > 160 &&
        pixels[offset]! > 120 &&
        pixels[offset + 2]! > 120
      )
        textPixels++;
    }
  }
  const sample = (x: number, y: number) => [
    ...pixels.subarray((y * width + x) * 4, (y * width + x) * 4 + 4),
  ];
  return {
    width,
    height,
    devicePixelRatio,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    failedDrawCalls: frame.failedDrawCalls,
    // Cache records carry bigint entity identities; report them as decimal text.
    statistics: JSON.parse(
      JSON.stringify(statistics, (_, value) =>
        typeof value === "bigint" ? value.toString() : value,
      ),
    ) as RenderStatisticsSnapshot | null,
    textPixels,
    inPage: {
      // Update start to pixels delivered to the page, measured in the page,
      // for a capture following a workload update; null otherwise.
      updateToReadbackMs:
        update === undefined ? null : presentedAt - update.startedAt,
      updateMs:
        update === undefined ? null : update.appliedAt - update.startedAt,
      captureMs: presentedAt - captureStarted,
      readbackMs: statistics.readbackMs,
    },
    background: sample(width >> 1, Math.round(height * 0.875)),
    corner: sample(0, 0),
    bitmap: sample(Math.round(width * 0.8625), Math.round(height * 0.35)),
  };
}

/** In-page timestamps of the latest workload update, consumed by the next capture. */
let pendingUpdate: { startedAt: number; appliedAt: number } | undefined;

/** The same application fixture runs against analytic and retained builds. */
export async function workload(
  config: Omit<TerminalWorkload, "glyphs" | "unseenGlyphs"> & {
    panels?: number;
    angle?: number;
    width?: number;
    height?: number;
    /** Opt every panel into whole-Surface caching with this policy. */
    cache?: SurfaceCachePolicy;
  },
) {
  const startedAt = performance.now();
  pendingUpdate = undefined;
  await resizePresentation(config.width ?? 640, config.height ?? 480);
  for (const id of [...cachePolicies.keys()])
    if (id.startsWith("workload-")) cachePolicies.delete(id);
  if (config.cache)
    for (let index = 0; index < (config.panels ?? 1); index++)
      cachePolicies.set(`workload-${index}`, config.cache);
  await presentCached(() =>
    Array.from({ length: config.panels ?? 1 }, (_, index) => (
      <Terminal
        key={index}
        id={`workload-${index}`}
        worldId={`workload-content-${index}`}
        assets={assets}
        x={index * 0.1}
        z={-index * 0.02}
        angle={config.angle ?? 0}
        cache={cachePolicies.get(`workload-${index}`)}
      >
        {terminalWorkloadLayers(
          assets,
          {
            ...config,
            glyphs: workloadGlyphs,
            unseenGlyphs,
          },
          glyphCodec,
        )}
      </Terminal>
    )),
  );
  pendingUpdate = { startedAt, appliedAt: performance.now() };
}

/** Sizes of the application's printable and unseen glyph sets. */
export function glyphSets() {
  return { printable: workloadGlyphs.length, unseen: unseenGlyphs.length };
}

/** Bound the renderer's shared glyph atlas through the presentation channel. */
export function glyphAtlasLimits(limits: GlyphAtlasLimits) {
  presentationTesting(rendererDiagnostics()).setGlyphAtlasLimits(limits);
}

export async function clearWorkload(viewport?: {
  width: number;
  height: number;
}) {
  if (viewport) await resizePresentation(viewport.width, viewport.height);
  await present(null);
}

/**
 * Present a second World on this Host's graphics context. Detaching the
 * current World ends its presentation, so the shared renderer releases that
 * World's retained batches and glyph demand while it stays resident on the
 * Host. The second World copies the current camera; later fixture calls
 * address it.
 */
async function replacePresentedWorld() {
  const inspection = await client.inspect();
  const view = inspection.entities.find(({ id }) => id === camera)!;
  const fields = (component: "Transform" | "Camera") =>
    Object.fromEntries(
      Object.entries(
        view.components.find(
          (value) => value.component === client.components[component]!.id,
        )!.fields,
      ).filter(([, value]) => typeof value === "number"),
    ) as Record<string, number>;
  const placement = {
    Transform: fields("Transform"),
    Camera: fields("Camera"),
  };
  await presentation.close();
  await host.detachWorld(client.session);
  const created = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CAMERA, SURFACE, LIFECYCLE),
    symbolicId: "surface-second",
    temporary: true,
  });
  client = await host.openWorld(created.reference);
  client.onRuntimeFailure((failure) => {
    failures.push(failure);
    console.error("Second World runtime failure", failure.message);
  });
  camera = await createFixtureCamera(client);
  for (const [component, values] of Object.entries(placement))
    successfulBatch(
      await client.batch(
        componentFields(client, component, values).map((field) => ({
          kind: "setField",
          entity: { kind: "handle", id: camera },
          component: client.components[component]!.id,
          field,
        })),
      ),
    );
  presentation = new CanvasWorldSession({ host, client });
  root = presentation.createRoot();
  cameraOutput = await host.bindOutput(created.reference, camera, "camera");
  await resizePresentation(640, 480);
  lastSequence = undefined;
  cachePolicies.clear();
}

/** Raw RGBA pixels of a completed capture, base64 encoded for Node-side comparison. */
export async function capturePixels(label: string) {
  const frame = frames.get(label)!;
  const { width, height } = frame.view.binding.viewport;
  const encoded = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(new Blob([frame.pixels]));
  });
  return {
    width,
    height,
    pixels: encoded.slice(encoded.indexOf(",") + 1),
  };
}

export async function referenceText(label: string) {
  const frame = frames.get(label)!;
  const { width, height } = frame.view.binding.viewport;
  const canvas = document.createElement("canvas");
  const supersampling = 8;
  canvas.width = width * supersampling;
  canvas.height = height * supersampling;
  const context = canvas.getContext("2d")!;
  const scale = canvas.height / 3;
  context.fillStyle = "white";
  context.font = `${0.14 * scale}px SurfaceOracle`;
  context.textBaseline = "alphabetic";
  for (const [line, text] of TERMINAL_TEXT.split("\n").entries())
    context.fillText(
      text,
      canvas.width / 2 - 1.72 * scale,
      canvas.height / 2 -
        0.92 * scale +
        (metrics.ascender / metrics.units) * 0.14 * scale +
        ((line * metrics.line) / metrics.units) * 0.14 * scale,
    );
  const expected = context.getImageData(0, 0, canvas.width, canvas.height).data;
  references.set(label, canvas);
  const actual = new Uint8Array(frame.pixels);
  const linear = (encoded: number) => {
    const value = encoded / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };
  let intersection = 0,
    union = 0;
  for (let y = Math.round(height * 0.12); y < height * 0.65; y++) {
    // Sample left of the text, then invert linear compositing with its authored
    // 0.92 green tint so both rasterizers use the same coverage threshold.
    const backgroundGreen = linear(actual[(y * width + 12) * 4 + 1]!);
    for (let x = 12; x < width * 0.88; x++) {
      const offset = (y * width + x) * 4;
      let coverage = 0;
      for (let dy = 0; dy < supersampling; dy++)
        for (let dx = 0; dx < supersampling; dx++) {
          coverage +=
            expected[
              ((y * supersampling + dy) * canvas.width +
                x * supersampling +
                dx) *
                4 +
                3
            ]!;
        }
      const reference = coverage / supersampling ** 2 > 64;
      const renderedCoverage =
        (linear(actual[offset + 1]!) - backgroundGreen) /
        (0.92 - backgroundGreen);
      const rendered = renderedCoverage > 64 / 255;
      if (reference || rendered) union++;
      if (reference && rendered) intersection++;
    }
  }
  return { intersection, union, similarity: intersection / Math.max(union, 1) };
}

export function referenceDataUrl(label: string) {
  return references.get(label)!.toDataURL("image/png");
}

export async function glyphProbe(
  glyph: string,
  fontSize: number,
  angle: number,
  projected: boolean,
  label: string,
) {
  await resizePresentation(320, 240);
  successfulBatch(
    await client.batch(
      componentFields(client, "Camera", {
        projection: projected ? 0 : 1,
        fov_y: 0.49,
      }).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id: camera },
        component: client.components.Camera!.id,
        field,
      })),
    ),
  );
  const glyphLayers = terminalLayers(assets, {
    backgroundColor: [0, 0, 0, 1],
    text: glyph,
    textFontSize: fontSize,
    textPosition: [1.9 - 0.3 * fontSize, 1.2 + 0.25 * fontSize],
    textColor: [1, 1, 1],
  });
  await present(
    <Terminal
      assets={assets}
      angle={angle}
      onWorld={observeCanvas("surface-terminal")}
    >
      {[glyphLayers[0], glyphLayers[2]]}
    </Terminal>,
  );
  await capture(label);
  const reference = compareGlyph(
    frames.get(label)!,
    glyph,
    fontSize,
    angle,
    projected,
    metrics.ascender / metrics.units,
  );
  references.set(label, reference.canvas);
  return reference.result;
}

export async function changeView(angle: number, width = 320, height = 240) {
  await resizePresentation(width, height);
  await present(
    <Terminal
      assets={assets}
      angle={angle}
      onWorld={observeCanvas("surface-terminal")}
    >
      {terminalLayers(assets)}
    </Terminal>,
  );
  return (await terminalCanvasSnapshot()).children.map(
    (item) => item.symbolicId,
  );
}

export async function perspective(angle: number) {
  successfulBatch(
    await client.batch(
      componentFields(client, "Camera", { projection: 0, fov_y: 0.49 }).map(
        (field) => ({
          kind: "setField",
          entity: { kind: "handle", id: camera },
          component: client.components.Camera!.id,
          field,
        }),
      ),
    ),
  );
  return changeView(angle);
}

export async function orthographic() {
  successfulBatch(
    await client.batch(
      componentFields(client, "Camera", { projection: 1 }).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id: camera },
        component: client.components.Camera!.id,
        field,
      })),
    ),
  );
  return changeView(0);
}

export async function keyedLifecycle() {
  const layers = terminalLayers(assets);
  const before = await terminalCanvasChildren();
  [layers[4], layers[5]] = [layers[5]!, layers[4]!];
  await present(
    <Terminal assets={assets} onWorld={observeCanvas("surface-terminal")}>
      {layers}
    </Terminal>,
  );
  const reordered = await terminalCanvasSnapshot();
  const reorderedEntities = await terminalCanvasChildren();
  const cursor = layers.splice(3, 1)[0]!;
  await present(
    <Terminal assets={assets} onWorld={observeCanvas("surface-terminal")}>
      {layers}
    </Terminal>,
  );
  const removed = await terminalCanvasSnapshot();
  const removedEntities = await terminalCanvasChildren();
  layers.splice(3, 0, cursor);
  await present(
    <Terminal assets={assets} onWorld={observeCanvas("surface-terminal")}>
      {layers}
    </Terminal>,
  );
  const restored = await terminalCanvasSnapshot();
  const restoredEntities = await terminalCanvasChildren();
  const originalCursor = before.find(
    (item) => item.metadata.symbolicId === "cursor",
  );
  const replacementCursor = restoredEntities.find(
    (item) => item.metadata.symbolicId === "cursor",
  );
  return {
    reordered: reordered.children.map((item) => item.symbolicId),
    reorderedIdentityPreserved:
      reorderedEntities.length === before.length &&
      before.every((item) =>
        reorderedEntities.some(
          (candidate) =>
            candidate.metadata.symbolicId === item.metadata.symbolicId &&
            candidate.id === item.id,
        ),
      ),
    removed: removed.children.map((item) => item.symbolicId),
    cursorRemoved: !removedEntities.some(
      (item) => item.metadata.symbolicId === "cursor",
    ),
    restored: restored.children.map((item) => item.symbolicId),
    cursorReplaced:
      originalCursor !== undefined &&
      replacementCursor !== undefined &&
      originalCursor.id !== replacementCursor.id &&
      restored.children.find((item) => item.symbolicId === "cursor")?.components
        .CanvasDrawing !== undefined &&
      restored.children.find((item) => item.symbolicId === "cursor")?.components
        .CanvasStyle !== undefined,
  };
}

/**
 * React authoring of the SurfaceCache opt-in on its own root: mount without a
 * policy, opt in, edit, opt out and unmount, observed through inspection.
 */
export async function surfaceCacheDeclarations() {
  const policyRoot = createRoot(client);
  const panel = (cache?: SurfaceCacheProps) => (
    <Entity id="surface-cache-policy">
      <Surface width={1} height={1} />
      {cache ? <SurfaceCache {...cache} /> : null}
    </Entity>
  );
  const observed: unknown[] = [];
  await policyRoot.render(panel());
  const entity = (await client.inspect()).entities.find(
    (item) => item.metadata.symbolicId === "surface-cache-policy",
  )!.id;
  const observe = async () =>
    observed.push((await surfaceCachePolicy(client, entity)) ?? null);
  await observe();
  await policyRoot.render(panel({ direct_distance: 1.5, max_refresh_hz: 4 }));
  await observe();
  await policyRoot.render(
    panel({ direct_distance: 0, texels_per_metre: 64, max_refresh_hz: 4 }),
  );
  await observe();
  await policyRoot.render(panel());
  await observe();
  // Unmount deletes nothing; removing the declaration deletes the entity.
  await policyRoot.render(null);
  await policyRoot.unmount();
  return {
    observed,
    released: !(await client.inspect()).entities.some(
      (item) => item.id === entity,
    ),
  };
}

export async function paintOrder(reverse: boolean, angle = 0) {
  const layers = terminalLayers(assets);
  const probes = [
    canvasDrawingEntity(
      "paint-red",
      assets.panel,
      [1.9, 1.85],
      [0.4, 0.4],
      [1, 0, 0, 1],
    ),
    canvasDrawingEntity(
      "paint-green",
      assets.panel,
      [1.9, 1.85],
      [0.4, 0.4],
      [0, 1, 0, 1],
    ),
  ];
  if (reverse) probes.reverse();
  layers.push(
    ...probes,
    canvasDrawingEntity(
      "paint-clipped",
      assets.panel,
      [3.8, 1.2],
      [0.5, 0.5],
      [1, 0, 0, 1],
    ),
  );
  await present(
    <Terminal
      assets={assets}
      angle={angle}
      onWorld={observeCanvas("surface-terminal")}
    >
      {layers}
    </Terminal>,
  );
}

/** Compare a rear capture with the independently expected horizontal reflection. */
export function mirrorComparison(frontLabel: string, rearLabel: string) {
  const front = frames.get(frontLabel)!;
  const rear = frames.get(rearLabel)!;
  const { width, height } = front.view.binding.viewport;
  const rearViewport = rear.view.binding.viewport;
  if (width !== rearViewport.width || height !== rearViewport.height)
    throw new Error("Surface mirror captures have different dimensions");
  const a = new Uint8Array(front.pixels);
  const b = new Uint8Array(rear.pixels);
  let contentPixels = 0;
  let mismatchedPixels = 0;
  let totalError = 0;
  const background = [...a.subarray(0, 4)];
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const source = (y * width + x) * 4;
      const reflected = (y * width + (width - x - 1)) * 4;
      let pixelError = 0;
      let content = false;
      for (let channel = 0; channel < 4; channel++) {
        pixelError = Math.max(
          pixelError,
          Math.abs(a[source + channel]! - b[reflected + channel]!),
        );
        content ||= a[source + channel] !== background[channel];
      }
      if (content) contentPixels++;
      if (pixelError > 4) mismatchedPixels++;
      totalError += pixelError;
    }
  }
  return {
    contentPixels,
    mismatchedPixels,
    meanError: totalError / (width * height),
  };
}

/**
 * Place differently scaled markers at opposite content corners, then move one
 * downward. Returns rendered centroids/extents and the content point that the
 * production camera projection and Surface inverse assign to a rendered pixel.
 */
export async function orientationProbe(redY: number, label: string) {
  await resizePresentation(320, 240);
  const layers = terminalLayers(assets, {
    backgroundColor: [0, 0, 0, 1],
  }).slice(0, 1);
  layers.push(
    canvasDrawingEntity(
      "orientation-red",
      assets.panel,
      [0.6, redY],
      [0.4, 0.2],
      [1, 0, 0, 1],
    ),
    canvasDrawingEntity(
      "orientation-green",
      assets.panel,
      [3.2, 1.9],
      [0.2, 0.4],
      [0, 1, 0, 1],
    ),
  );
  await present(
    <Terminal assets={assets} onWorld={observeCanvas("surface-terminal")}>
      {layers}
    </Terminal>,
  );
  await capture(label);
  const frame = frames.get(label)!;
  const { width, height } = frame.view.binding.viewport;
  const pixels = new Uint8Array(frame.pixels);
  const region = (select: (r: number, g: number, b: number) => boolean) => {
    let count = 0,
      x = 0,
      y = 0,
      left = Infinity,
      right = -Infinity,
      top = Infinity,
      bottom = -Infinity;
    for (let row = 0; row < height; row++)
      for (let column = 0; column < width; column++) {
        const offset = (row * width + column) * 4;
        if (!select(pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!))
          continue;
        count++;
        x += column;
        y += row;
        left = Math.min(left, column);
        right = Math.max(right, column);
        top = Math.min(top, row);
        bottom = Math.max(bottom, row);
      }
    return {
      count,
      centroid: [x / count, y / count] as const,
      size: [right - left + 1, bottom - top + 1] as const,
    };
  };
  const red = region((r, g, b) => r > 180 && g < 90 && b < 90);
  const green = region((r, g, b) => g > 180 && r < 90 && b < 90);
  const projection = await client.query({
    type: "CameraProjectQuery",
    view: {
      kind: "bound",
      binding: frame.view.binding,
    },
    x: (red.centroid[0] + 0.5) / width,
    y: (red.centroid[1] + 0.5) / height,
    plane: { point: [0, 0, 0], normal: [0, 0, 1] },
  });
  if (!projection.ok || !projection.position)
    throw new Error(
      `Surface plane projection failed: ${JSON.stringify(projection, (_, value) => (typeof value === "bigint" ? String(value) : value))}`,
    );
  const hit = entityLocalToSurfaceContent(
    [projection.position[0], projection.position[1]],
    [3.8, 2.4],
  );
  return { red, green, hit };
}

export function sample(label: string, x: number, y: number) {
  const frame = frames.get(label)!;
  const { width } = frame.view.binding.viewport;
  const offset = (y * width + x) * 4;
  return [...new Uint8Array(frame.pixels).subarray(offset, offset + 4)];
}

export async function pendingFont() {
  const pending = clientAssetSource(client.session, 17, 9000n);
  await present(
    <Terminal assets={assets} onWorld={observeCanvas("surface-terminal")}>
      {terminalLayers(assets, { font: pending })}
    </Terminal>,
  );
}

export async function provideFont() {
  const pending = clientAssetSource(client.session, 17, 9000n);
  await client.registerAsset(
    pending,
    await (
      await fetch("/target/font-assets/shure-tech-mono.ippf")
    ).arrayBuffer(),
  );
  await waitSurfaceAssets(await canvasSession(), [pending.source]);
}

export async function recover() {
  const previous = await presentation.capture({ afterOutputs: [cameraOutput] });
  presentationTesting(rendererDiagnostics()).loseContext();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  presentationTesting(rendererDiagnostics()).restoreContext();
  await presentation.recoverPresentation();
  while (true) {
    const restored = await presentation.capture({
      afterOutputs: [cameraOutput],
    });
    if (restored.view.surface.context !== previous.view.surface.context) break;
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
  await waitPresentedCanvasAssets();
}

/**
 * Wait until every presented terminal Canvas World has its assets loaded: the
 * observed terminal scene uses all four, and streamed workload panels, which
 * the fixture does not observe, use the font and panel drawing.
 */
async function waitPresentedCanvasAssets() {
  const terminal = canvasSessions.get("surface-terminal");
  if (terminal) {
    await waitSurfaceAssets(
      await terminal,
      Object.values(assets).map((asset) => asset.source),
    );
    return;
  }
  const worlds = (await host.listWorlds()).filter(({ symbolicId }) =>
    symbolicId.startsWith("workload-content-"),
  );
  if (worlds.length === 0)
    throw new Error("No presented terminal Canvas World to recover");
  for (const { symbolicId } of worlds) {
    const session = await host.openWorld(await host.resolveWorld(symbolicId));
    try {
      await waitSurfaceAssets(session, [
        assets.font.source,
        assets.panel.source,
      ]);
    } finally {
      await session.close();
    }
  }
}

export async function loadPendingSurfaceAssetsAcrossContextLoss() {
  const [fontBytes, drawingBytes] = await Promise.all([
    fetch("/target/font-assets/shure-tech-mono.ippf").then((response) =>
      response.arrayBuffer(),
    ),
    fetch("/target/surface-assets/panel.ippd").then((response) =>
      response.arrayBuffer(),
    ),
  ]);
  const font = clientAssetSource(client.session, 17, 9001n);
  const drawing = clientAssetSource(client.session, 18, 9002n);
  await present(
    <Terminal assets={assets} onWorld={observeCanvas("surface-terminal")}>
      {terminalLayers(assets, { font, panel: drawing })}
    </Terminal>,
  );
  const contentClient = await canvasSession();
  const before = await presentation.capture({ afterOutputs: [cameraOutput] });
  presentationTesting(rendererDiagnostics()).loseContext();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );

  await Promise.all([
    client.registerAsset(font, fontBytes),
    client.registerAsset(drawing, drawingBytes),
  ]);
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  const whileLost = (await contentClient.inspect()).resources.filter(
    (resource) =>
      resource.source === font.source || resource.source === drawing.source,
  );

  presentationTesting(rendererDiagnostics()).restoreContext();
  await presentation.recoverPresentation();
  await waitSurfaceAssets(contentClient, [font.source, drawing.source]);
  let after = await presentation.capture({ afterOutputs: [cameraOutput] });
  while (after.view.surface.context === before.view.surface.context) {
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    after = await presentation.capture({ afterOutputs: [cameraOutput] });
  }
  const restored = (await contentClient.inspect()).resources.filter(
    (resource) =>
      resource.source === font.source || resource.source === drawing.source,
  );
  return {
    beforeGeneration: String(before.view.surface.context),
    afterGeneration: String(after.view.surface.context),
    whileLost,
    restored,
  };
}

export function equal(a: string, b: string) {
  const first = new Uint8Array(frames.get(a)!.pixels),
    second = new Uint8Array(frames.get(b)!.pixels);
  return (
    first.length === second.length &&
    first.every((value, index) => value === second[index])
  );
}

export function captureDataUrl(label: string) {
  const frame = frames.get(label)!;
  const { width, height } = frame.view.binding.viewport;
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(new Uint8ClampedArray(frame.pixels), width, height),
      0,
      0,
    );
  return canvas.toDataURL("image/png");
}

export async function close() {
  cachePolicies.clear();
  cacheScene = null;
  await presentation?.close();
  await Promise.all(
    [...observedCanvasClients].map(async (session) => {
      if (!session.closure) await session.close();
    }),
  );
  observedCanvasClients.clear();
  canvasSessions.clear();
  await host?.close();
  frames.clear();
  lastSequence = undefined;
}

/** Authored `SurfaceCache` fields; see `ipp_core::SurfaceCachePolicy`. */
export interface SurfaceCachePolicy {
  direct_distance: number;
  texels_per_metre: number;
  max_refresh_hz: number;
}

async function entityBySymbol(symbolicId: string) {
  const entity = (await client.inspect()).entities.find(
    (candidate) => candidate.metadata.symbolicId === symbolicId,
  );
  if (!entity) throw new Error(`No entity ${symbolicId}`);
  return entity.id;
}

/**
 * Opt a Surface of the current cache scenario into whole-Surface caching,
 * replace its policy, or with `null` return it to direct presentation, by
 * re-rendering the scene with its React SurfaceCache declaration.
 */
export async function setSurfaceCache(
  symbolicId: string,
  policy: SurfaceCachePolicy | null,
) {
  if (!cacheScene) throw new Error("No cache-aware scene is presented");
  if (policy) cachePolicies.set(symbolicId, { ...policy });
  else cachePolicies.delete(symbolicId);
  await root.render(cacheScene());
  return String(await entityBySymbol(symbolicId));
}

/** Generational identity of a named entity, as decimal text. */
export async function entityId(symbolicId: string) {
  return String(await entityBySymbol(symbolicId));
}

/** Move the orthographic fixture camera along +Z; the projected size is unchanged. */
export async function cameraDistance(distance: number) {
  successfulBatch(
    await client.batch(
      componentFields(client, "Transform", { z: distance }).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id: camera },
        component: client.components.Transform!.id,
        field,
      })),
    ),
  );
}

/**
 * The terminal application Surface in a controlled state: `cursor` recolours
 * the cursor item, `translucent` widens it over the text at half opacity, and
 * `font` selects the pending font source used for resource arrival. The view
 * is 320 x 240, or with `devicePixels` that CSS size times the page's
 * `devicePixelRatio`, as an application sizes its drawing buffer.
 */
export async function cacheTerminal(config: {
  cursor?: readonly [number, number, number, number];
  translucent?: boolean;
  font?: "ready" | "pending";
  angle?: number;
  devicePixels?: boolean;
}) {
  const scale = config.devicePixels ? window.devicePixelRatio : 1;
  await resizePresentation(
    Math.round(320 * scale),
    Math.round(240 * scale),
    window.devicePixelRatio,
  );
  const layers = terminalLayers(assets, {
    ...(config.cursor ? { cursorColor: config.cursor } : {}),
    ...(config.translucent
      ? {
          cursorOpacity: 0.5,
          cursorPosition: [1.2, 1.1] as const,
          cursorScale: [1.8, 0.5] as const,
        }
      : {}),
    ...(config.font === "pending"
      ? { font: clientAssetSource(client.session, 17, 9000n) }
      : {}),
  });
  await presentCached(() => (
    <Terminal
      assets={assets}
      angle={config.angle ?? 0}
      cache={cachePolicies.get("surface-terminal")}
      onWorld={observeCanvas("surface-terminal")}
    >
      {layers}
    </Terminal>
  ));
}

/** Bound resident cache image bytes on this graphics context. */
export function surfaceCacheBudget(bytes: number) {
  presentationTesting(rendererDiagnostics()).setSurfaceCacheBudget(bytes);
}

/** Run the device-level cache target and error-check oracles against a build's shipped WebGL bridge. */
export async function bridgeProbe(build: string, gui: boolean) {
  const bridge = `/target/browser-build/${build}/webgl.js`;
  return {
    ...(await probeSurfaceCacheBridge(bridge, gui)),
    errorChecks: await probeErrorCheckBridge(bridge),
  };
}

export const surfaceFixture = {
  get host() {
    return host;
  },
  get client() {
    return client;
  },
  get presentation() {
    return presentation;
  },
  get assets() {
    return assets;
  },
  cameraHeight: ORTHO_HEIGHT,
  resizePresentation,
  presentCached,
  cacheDeclaration,
  entityBySymbol,
  replacePresentedWorld,
};
