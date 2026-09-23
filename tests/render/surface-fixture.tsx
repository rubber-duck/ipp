import {
  createRoot,
  Entity,
  Surface,
  SurfaceCache,
  Transform,
  type ReactWorldRoot,
  type SurfaceCacheProps,
} from "@ipp/react";
import {
  Checkbox,
  Drawing,
  GuiRoot,
  Padding,
  Row,
  Stack,
  Text,
  type GuiControlTheme,
} from "@ipp/react/gui";
import {
  terminalWorkloadItems,
  type TerminalWorkload,
} from "../../examples/surface-terminal/workload.js";
import {
  clientAssetSource,
  entityLocalToSurfaceContent,
  surfaceProperty,
} from "@ipp/client";
import { compareGlyph } from "./surface-glyph-oracle.js";
import { probeSurfaceCacheBridge } from "./surface-cache-bridge.js";
import type {
  FrameCapture,
  GlyphAtlasLimits,
  PickingWorldClient,
  RenderWorldClient,
  WorldPersistenceHostClient,
} from "@ipp/client";
import type { SurfaceItemProps } from "@ipp/react";
import type { ReactNode } from "react";
import {
  Terminal,
  terminalItems,
  TERMINAL_TEXT,
  type TerminalAssets,
} from "../../examples/surface-terminal/scene.js";
import {
  activateFixtureCamera,
  componentFields,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import {
  exerciseSurfaceLifecycle,
  surfaceCachePolicy,
  surfaceSnapshot,
  waitSurfaceAssets,
  type SurfaceTestClient,
} from "../integration/surface-scenario.js";

type Client = SurfaceTestClient & RenderWorldClient & PickingWorldClient;
let host: WorldPersistenceHostClient<Client>;
let client: Client;
let root: ReactWorldRoot;
let assets: TerminalAssets;
let terminal: bigint;
let camera: bigint;
let metrics: { units: number; ascender: number; line: number };
let workloadGlyphs: number[];
let unseenGlyphs: number[];
const frames = new Map<string, FrameCapture>();
const ORTHO_HEIGHT = 3;
const references = new Map<string, HTMLCanvasElement>();
const failures: unknown[] = [];

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
  return policy ? <SurfaceCache bound={false} {...policy} /> : null;
}

export async function initialize(config: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 240;
  document.body.replaceChildren(canvas);
  const contract = await import(config.generatedModuleUrl);
  host = await contract.IppHostClient.connectWorker(
    config.workerScriptUrl,
    config.wasmUrl,
    { canvas: canvas.transferControlToOffscreen(), timeoutMs: 20000 },
  );
  client = await host.createWorld({ symbolicId: "surface-rendering" });
  client.onRuntimeFailure((failure) => {
    failures.push(failure);
    console.error("Surface runtime failure", failure.message);
  });
  camera = await activateFixtureCamera(client);
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
  const lifecycle = await exerciseSurfaceLifecycle(client, assets, glyphs.A);
  successfulBatch(
    await client.batch([
      { kind: "delete", entity: { kind: "handle", id: lifecycle.entity } },
    ]),
  );
  root = createRoot(client);
  await present(<Terminal assets={assets} />);
  await waitSurfaceAssets(
    client,
    Object.values(assets).map((asset) => asset.source),
  );
  terminal = (await client.inspect()).entities.find(
    (entity) => entity.metadata.symbolicId === "surface-terminal",
  )!.id;
  const fontFace = new FontFace(
    "SurfaceOracle",
    await (
      await fetch("/target/font-sources/shure-tech-mono.ttf")
    ).arrayBuffer(),
  );
  document.fonts.add(await fontFace.load());
  return {
    nextId: lifecycle.nextId,
    ids: (await surfaceSnapshot(client, terminal)).collection.items.map(
      (item) => item.id,
    ),
  };
}

/**
 * Capture the frame presenting the latest inspected state, or with `next` the
 * next completed frame, which observes work a following frame would finish.
 */
export async function capture(label: string, options: { next?: boolean } = {}) {
  const tick = options.next ? undefined : (await client.inspect()).tick;
  if (failures.length)
    throw new Error(
      `Surface runtime failures: ${JSON.stringify(failures, (_, value) => (typeof value === "bigint" ? String(value) : value))}`,
    );
  const frame = await client.presentation!.capture(tick);
  frames.set(label, frame);
  const pixels = new Uint8Array(frame.pixels);
  let textPixels = 0;
  for (let y = Math.round(frame.height * 0.12); y < frame.height * 0.65; y++) {
    for (let x = 12; x < frame.width * 0.88; x++) {
      const offset = (y * frame.width + x) * 4;
      if (
        pixels[offset + 1]! > 160 &&
        pixels[offset]! > 120 &&
        pixels[offset + 2]! > 120
      )
        textPixels++;
    }
  }
  const sample = (x: number, y: number) => [
    ...pixels.subarray(
      (y * frame.width + x) * 4,
      (y * frame.width + x) * 4 + 4,
    ),
  ];
  return {
    width: frame.width,
    height: frame.height,
    devicePixelRatio: window.devicePixelRatio,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    // Cache records carry bigint entity identities; report them as decimal text.
    backend: JSON.parse(
      JSON.stringify(frame.backend, (_, value) =>
        typeof value === "bigint" ? value.toString() : value,
      ),
    ) as Record<string, unknown>,
    textPixels,
    background: sample(frame.width >> 1, Math.round(frame.height * 0.875)),
    corner: sample(0, 0),
    bitmap: sample(
      Math.round(frame.width * 0.8625),
      Math.round(frame.height * 0.35),
    ),
  };
}

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
  client.presentation!.resize(config.width ?? 640, config.height ?? 480);
  for (const id of [...cachePolicies.keys()])
    if (id.startsWith("workload-")) cachePolicies.delete(id);
  if (config.cache)
    for (let index = 0; index < (config.panels ?? 1); index++)
      cachePolicies.set(`workload-${index}`, config.cache);
  await presentCached(() =>
    Array.from({ length: config.panels ?? 1 }, (_, index) => (
      <Entity key={index} id={`workload-${index}`}>
        <Transform
          bound={false}
          x={index * 0.1}
          z={-index * 0.02}
          ry={config.angle ?? 0}
        />
        <Surface
          bound={false}
          width={3.8}
          height={2.4}
          items={terminalWorkloadItems(assets, {
            ...config,
            glyphs: workloadGlyphs,
            unseenGlyphs,
          })}
        />
        {cacheDeclaration(`workload-${index}`)}
      </Entity>
    )),
  );
}

/** Sizes of the application's printable and unseen glyph sets. */
export function glyphSets() {
  return { printable: workloadGlyphs.length, unseen: unseenGlyphs.length };
}

/** Bound the renderer's shared glyph atlas through the presentation channel. */
export function glyphAtlasLimits(limits: GlyphAtlasLimits) {
  client.presentation!.setGlyphAtlasLimits(limits);
}

export async function clearWorkload(viewport?: {
  width: number;
  height: number;
}) {
  if (viewport) client.presentation!.resize(viewport.width, viewport.height);
  await present(null);
}

type Color = readonly [number, number, number, number];
const PANEL_COLOR: Color = [0.02, 0.03, 0.05, 1];
/**
 * A GUI panel on the same camera, viewport and DPR as the terminal workload.
 * `mixed` combines a gradient shape with glow, atlas glyphs and a curve drawing;
 * `filled` and `sparse` differ only in the shape's interior fill. `control`
 * places a checkbox before the shape, under {@link CONTROL_POINT}, for pointer
 * interaction.
 */
export async function guiPanel(config: {
  variant: "mixed" | "mixed-without-glow" | "filled" | "sparse" | "empty";
  control?: boolean;
  /** Shape size, border and corner radius in Surface metres. */
  shape: {
    width: number;
    height: number;
    borderWidth: number;
    cornerRadius: number;
  };
  angle?: number;
}) {
  const [width, height] = [640, 480];
  client.presentation!.resize(width, height);
  const { variant, shape } = config;
  const edge = {
    cornerRadius: [shape.cornerRadius, shape.cornerRadius] as const,
    borderWidth: shape.borderWidth,
    borderColor: [1, 1, 1, 1] as Color,
  };
  const theme: GuiControlTheme = {
    parts: {
      background: {
        base: variant.startsWith("mixed")
          ? {
              ...edge,
              gradient: {
                kind: "linear",
                start: [0, 0],
                end: [0, shape.height],
                color0: [1, 0.08, 0.04, 1],
                color1: [1, 0.8, 0.08, 1],
              },
              ...(variant === "mixed"
                ? {
                    glow: {
                      color: [1, 0.35, 0.05, 1],
                      intensity: 0.8,
                      radius: 0.18,
                      falloff: 2,
                    },
                  }
                : {}),
            }
          : edge,
      },
    },
  };
  const fill: Color =
    variant === "sparse" ? [0, 0, 0, 0] : [0.85, 0.25, 0.08, 1];
  await presentCached(() => (
    <Entity key="gui" id="retained-gui-panel">
      <Transform bound={false} ry={config.angle ?? 0} />
      <Surface bound={false} width={3.8} height={2.4} />
      {cacheDeclaration("retained-gui-panel")}
      <GuiRoot>
        <Stack width={3.8} height={2.4} backgroundColor={PANEL_COLOR}>
          <Padding padding={[0.6, 0, 0, 0.3]}>
            <Row>
              {config.control ? <Checkbox width={0.5} height={1.2} /> : null}
              {variant === "empty" ? (
                <Stack width={shape.width} height={shape.height} />
              ) : (
                <Stack
                  width={shape.width}
                  height={shape.height}
                  backgroundColor={fill}
                  theme={theme}
                />
              )}
              {variant.startsWith("mixed") ? (
                <>
                  {/* Scale the 32 x 24 unit icon to 0.8 x 0.6 metres. */}
                  <Drawing
                    asset={assets.icon}
                    width={0.8}
                    height={0.6}
                    color={[1, 1, 1, 1]}
                    margin={[0.3, 0, 0, 0.15]}
                    theme={{
                      parts: { icon: { base: { scale: [0.025, 0.025] } } },
                    }}
                  />
                  <Text
                    text="Gui"
                    asset={assets.font}
                    fontSize={0.36}
                    color={[0.2, 1, 0.35, 1]}
                    margin={[0.3, 0, 0, 0.15]}
                  />
                </>
              ) : null}
            </Row>
          </Padding>
        </Stack>
      </GuiRoot>
    </Entity>
  ));
  // The orthographic fixture camera spans ORTHO_HEIGHT metres vertically.
  return { pixelsPerMetre: height / ORTHO_HEIGHT };
}

/**
 * Present a second World on this Host's graphics context. Detaching the
 * current World ends its presentation, so the shared renderer releases that
 * World's retained batches and glyph demand while it stays resident on the
 * Host. The second World copies the current camera and renders `panel`;
 * later fixture calls address it.
 */
export async function presentSecondWorld(
  panel: Parameters<typeof guiPanel>[0],
) {
  const inspection = await client.inspect();
  const view = inspection.entities.find(({ id }) => id === camera)!;
  const fields = (component: "Transform" | "Camera") =>
    Object.fromEntries(
      Object.entries(
        view.effective.find(
          (value) => value.component === client.components[component]!.id,
        )!.fields,
      ).filter(([, value]) => typeof value === "number"),
    ) as Record<string, number>;
  const placement = {
    Transform: fields("Transform"),
    Camera: fields("Camera"),
  };
  await host.detachWorld();
  client = await host.createWorld({
    symbolicId: "surface-second",
    temporary: true,
  });
  client.onRuntimeFailure((failure) => {
    failures.push(failure);
    console.error("Second World runtime failure", failure.message);
  });
  camera = await activateFixtureCamera(client);
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
  root = createRoot(client);
  cachePolicies.clear();
  return guiPanel(panel);
}

/** Raw RGBA pixels of a completed capture, base64 encoded for Node-side comparison. */
export async function capturePixels(label: string) {
  const frame = frames.get(label)!;
  const encoded = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(new Blob([frame.pixels]));
  });
  return {
    width: frame.width,
    height: frame.height,
    pixels: encoded.slice(encoded.indexOf(",") + 1),
  };
}

export async function referenceText(label: string) {
  const frame = frames.get(label)!;
  const canvas = document.createElement("canvas");
  const supersampling = 8;
  canvas.width = frame.width * supersampling;
  canvas.height = frame.height * supersampling;
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
  for (let y = Math.round(frame.height * 0.12); y < frame.height * 0.65; y++) {
    // Sample left of the text, then invert linear compositing with its authored
    // 0.92 green tint so both rasterizers use the same coverage threshold.
    const backgroundGreen = linear(actual[(y * frame.width + 12) * 4 + 1]!);
    for (let x = 12; x < frame.width * 0.88; x++) {
      const offset = (y * frame.width + x) * 4;
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
  client.presentation!.resize(320, 240);
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
  await present(
    <Terminal
      assets={assets}
      angle={angle}
      items={[
        { ...terminalItems(assets)[0]!, color: [0, 0, 0, 1] },
        {
          key: "text",
          content: { kind: "label", text: glyph },
          asset: assets.font,
          position: [1.9 - 0.3 * fontSize, 1.2 + 0.25 * fontSize],
          fontSize,
          color: [1, 1, 1, 1],
        },
      ]}
    />,
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
  client.presentation!.resize(width, height);
  await present(<Terminal assets={assets} angle={angle} />);
  return (await surfaceSnapshot(client, terminal)).collection.items.map(
    (item) => item.id,
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
  const items = terminalItems(assets);
  // Adjacent foreground items do not overlap; changing their order must retain identities.
  [items[4], items[5]] = [items[5]!, items[4]!];
  await present(<Terminal assets={assets} items={items} />);
  const reordered = (
    await surfaceSnapshot(client, terminal)
  ).collection.items.map((item) => item.id);
  const cursor = items.splice(3, 1)[0]!;
  await present(<Terminal assets={assets} items={items} />);
  const removed = await surfaceSnapshot(client, terminal);
  items.splice(3, 0, cursor);
  await present(<Terminal assets={assets} items={items} />);
  const restored = await surfaceSnapshot(client, terminal);
  return {
    reordered,
    removedIds: removed.collection.items.map((item) => item.id),
    removedProperty: removed.properties[surfaceProperty(4, "color")] ?? null,
    restoredIds: restored.collection.items.map((item) => item.id),
    nextId: restored.collection.nextId,
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
      <Surface bound={false} width={1} height={1} />
      {cache ? <SurfaceCache bound={false} {...cache} /> : null}
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
  await policyRoot.unmount();
  return {
    observed,
    released: !(await client.inspect()).entities.some(
      (item) => item.id === entity,
    ),
  };
}

export async function paintOrder(reverse: boolean, angle = 0) {
  const items = terminalItems(assets);
  const probes = [
    {
      key: "red",
      content: { kind: "drawing" as const },
      asset: assets.panel,
      position: [1.9, 1.85] as const,
      scale: [0.4, 0.4] as const,
      color: [1, 0, 0, 1] as const,
    },
    {
      key: "green",
      content: { kind: "drawing" as const },
      asset: assets.panel,
      position: [1.9, 1.85] as const,
      scale: [0.4, 0.4] as const,
      color: [0, 1, 0, 1] as const,
    },
  ];
  if (reverse) probes.reverse();
  items.push(...probes, {
    key: "clip",
    content: { kind: "drawing" },
    asset: assets.panel,
    position: [3.8, 1.2],
    scale: [0.5, 0.5],
    color: [1, 0, 0, 1],
  });
  await present(<Terminal assets={assets} items={items} angle={angle} />);
}

/** Compare a rear capture with the independently expected horizontal reflection. */
export function mirrorComparison(frontLabel: string, rearLabel: string) {
  const front = frames.get(frontLabel)!;
  const rear = frames.get(rearLabel)!;
  if (front.width !== rear.width || front.height !== rear.height)
    throw new Error("Surface mirror captures have different dimensions");
  const a = new Uint8Array(front.pixels);
  const b = new Uint8Array(rear.pixels);
  let contentPixels = 0;
  let mismatchedPixels = 0;
  let totalError = 0;
  const background = [...a.subarray(0, 4)];
  for (let y = 0; y < front.height; y++) {
    for (let x = 0; x < front.width; x++) {
      const source = (y * front.width + x) * 4;
      const reflected = (y * front.width + (front.width - x - 1)) * 4;
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
    meanError: totalError / (front.width * front.height),
  };
}

/**
 * Place differently scaled markers at opposite content corners, then move one
 * downward. Returns rendered centroids/extents and the content point that the
 * production camera projection and Surface inverse assign to a rendered pixel.
 */
export async function orientationProbe(redY: number, label: string) {
  client.presentation!.resize(320, 240);
  const marker = (
    key: string,
    position: readonly [number, number],
    scale: readonly [number, number],
    color: readonly [number, number, number, number],
  ): SurfaceItemProps => ({
    key,
    content: { kind: "drawing" },
    asset: assets.panel,
    position,
    scale,
    color,
  });
  await present(
    <Terminal
      assets={assets}
      items={[
        { ...terminalItems(assets)[0]!, color: [0, 0, 0, 1] },
        marker("red", [0.6, redY], [0.4, 0.2], [1, 0, 0, 1]),
        marker("green", [3.2, 1.9], [0.2, 0.4], [0, 1, 0, 1]),
      ]}
    />,
  );
  await capture(label);
  const frame = frames.get(label)!;
  const pixels = new Uint8Array(frame.pixels);
  const region = (select: (r: number, g: number, b: number) => boolean) => {
    let count = 0,
      x = 0,
      y = 0,
      left = Infinity,
      right = -Infinity,
      top = Infinity,
      bottom = -Infinity;
    for (let row = 0; row < frame.height; row++)
      for (let column = 0; column < frame.width; column++) {
        const offset = (row * frame.width + column) * 4;
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
    x: (red.centroid[0] + 0.5) / frame.width,
    y: (red.centroid[1] + 0.5) / frame.height,
    width: frame.width,
    height: frame.height,
    plane: { point: [0, 0, 0], normal: [0, 0, 1] },
  });
  if (!projection.ok || !projection.position)
    throw new Error(
      `Surface plane projection failed: ${JSON.stringify(projection)}`,
    );
  const hit = entityLocalToSurfaceContent(
    [projection.position[0], projection.position[1]],
    [3.8, 2.4],
  );
  return { red, green, hit };
}

export function sample(label: string, x: number, y: number) {
  const frame = frames.get(label)!;
  const offset = (y * frame.width + x) * 4;
  return [...new Uint8Array(frame.pixels).subarray(offset, offset + 4)];
}

export async function pendingFont() {
  const pending = clientAssetSource(client.session, 17, 9000n);
  const items = terminalItems(assets);
  items[2] = { ...items[2]!, asset: pending };
  await present(<Terminal assets={assets} items={items} />);
}

export async function provideFont() {
  const pending = clientAssetSource(client.session, 17, 9000n);
  await client.registerAsset(
    pending,
    await (
      await fetch("/target/font-assets/shure-tech-mono.ippf")
    ).arrayBuffer(),
  );
  await waitSurfaceAssets(client, [pending.source]);
}

export async function recover() {
  const previous = await client.presentation!.capture();
  client.presentation!.loseContext();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  client.presentation!.restoreContext();
  while (
    (await client.presentation!.capture()).contextGeneration <=
    previous.contextGeneration
  ) {
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
  await waitSurfaceAssets(
    client,
    Object.values(assets).map((asset) => asset.source),
  );
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
  const items = terminalItems(assets);
  items[0] = { ...items[0]!, asset: drawing };
  items[2] = { ...items[2]!, asset: font };
  await present(<Terminal assets={assets} items={items} />);
  const before = await client.presentation!.capture();
  client.presentation!.loseContext();
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
  const whileLost = (await client.inspect()).resources.filter(
    (resource) =>
      resource.source === font.source || resource.source === drawing.source,
  );

  client.presentation!.restoreContext();
  await waitSurfaceAssets(client, [font.source, drawing.source]);
  let after = await client.presentation!.capture();
  while (after.contextGeneration <= before.contextGeneration) {
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    after = await client.presentation!.capture();
  }
  const restored = (await client.inspect()).resources.filter(
    (resource) =>
      resource.source === font.source || resource.source === drawing.source,
  );
  return {
    beforeGeneration: before.contextGeneration,
    afterGeneration: after.contextGeneration,
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
  return canvas.toDataURL("image/png");
}

export async function close() {
  cachePolicies.clear();
  cacheScene = null;
  await root?.unmount();
  await host?.close();
  frames.clear();
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
 * `font` selects the pending font source used for resource arrival.
 */
export async function cacheTerminal(config: {
  cursor?: readonly [number, number, number, number];
  translucent?: boolean;
  font?: "ready" | "pending";
  angle?: number;
}) {
  client.presentation!.resize(320, 240);
  const items = terminalItems(assets);
  const cursor = items[3]!;
  const color = config.cursor ?? cursor.color ?? [1, 1, 1, 1];
  items[3] = {
    ...cursor,
    color: config.translucent ? [color[0], color[1], color[2], 0.5] : color,
    ...(config.translucent
      ? { position: [1.2, 1.1] as const, scale: [1.8, 0.5] as const }
      : {}),
  };
  if (config.font === "pending")
    items[2] = {
      ...items[2]!,
      asset: clientAssetSource(client.session, 17, 9000n),
    };
  await presentCached(() => (
    <Terminal
      assets={assets}
      items={items}
      angle={config.angle ?? 0}
      cache={cachePolicies.get("surface-terminal")}
    />
  ));
}

/**
 * Normalized viewport point over the `control` checkbox of an unrotated
 * {@link guiPanel}: the 3.8 x 2.4 m panel spans 16..624 x 48..432 pixels of the
 * 640 x 480 view at 160 px/m, and the checkbox follows the padding over panel
 * metres x 0.3..0.8 and y 0.6..1.8.
 */
const CONTROL_POINT = [(16 + 0.55 * 160) / 640, (48 + 1.2 * 160) / 480];

/**
 * Hover the `control` checkbox of the presented GUI panel through production
 * GUI input, or move the pointer off every panel.
 */
export async function hoverPanel(active: boolean) {
  const gui = client as unknown as {
    submitGuiInput(input: Record<string, unknown>): Promise<unknown>;
  };
  return gui.submitGuiInput({
    kind: "pointerMove",
    pointer: 1,
    position: active ? CONTROL_POINT : [0.01, 0.01],
  });
}

/** Bound resident cache image bytes on this graphics context. */
export function surfaceCacheBudget(bytes: number) {
  client.presentation!.setSurfaceCacheBudget(bytes);
}

/** Run the device-level cache target oracle against a build's shipped WebGL bridge. */
export function bridgeProbe(build: string, gui: boolean) {
  return probeSurfaceCacheBridge(
    `/target/browser-build/${build}/webgl.js`,
    gui,
  );
}
