/** Shared setup for the mounted browser GUI fixtures: a parent World camera
 * output presenting ordinary GUI panels in attached child Worlds. */
import type {
  Client,
  ClientAssetSource,
  GuiWorldClient,
  HostClientBase,
  OutputReference,
  RowsInput,
  WorldReference,
} from "@ipp/client";
import type { IppCanvasHandle } from "../../../packages/ipp-react/src/web.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";

/** CSS and Canvas size. The orthographic camera maps the 4x3 m panel onto
 * it, and panel Canvases use one logical unit per CSS pixel. */
export const WIDTH = 240;
export const HEIGHT = 180;
export const UNITS_PER_METRE = 60;

/** Systems of each attached panel World. */
export const PANEL_SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

/** Generated-contract members the fixtures use. */
export interface GuiPanelContract {
  readonly GUI_PAINT_PART_KEYS: readonly {
    readonly index: number;
    readonly part: string;
  }[];
  guiPaintPartIndex(key: {
    readonly part: string;
    readonly state?: string;
  }): number;
  readonly GuiTheme: { encodeParts(input: RowsInput): Uint8Array<ArrayBuffer> };
}

/** Encode theme part rows keyed by part name and optional state. */
export function encodeTheme(
  contract: GuiPanelContract,
  parts: readonly (readonly [
    part: string,
    state: string | undefined,
    row: Readonly<Record<string, unknown>>,
  ])[],
): Uint8Array<ArrayBuffer> {
  const rows = new Map(
    parts.map(([part, state, row], slot) => [
      slot,
      {
        ...row,
        part: contract.guiPaintPartIndex(state ? { part, state } : { part }),
      },
    ]),
  );
  return contract.GuiTheme.encodeParts({
    nextSlot: rows.size,
    rows: rows as RowsInput["rows"],
  });
}

/** Create the parent World's orthographic camera and bind its output. */
export async function presentCamera(
  client: Client,
  host: HostClientBase<Client>,
  symbolicId: string,
): Promise<OutputReference> {
  const camera = aliasId(
    successfulBatch(
      await client.batch([
        createEntity(1, symbolicId),
        insertComponent(
          client,
          "Transform",
          { kind: "alias", alias: 1 },
          {
            z: 6,
          },
        ),
        insertComponent(
          client,
          "Camera",
          { kind: "alias", alias: 1 },
          {
            projection: 1,
            ortho_height: HEIGHT / UNITS_PER_METRE,
          },
        ),
      ]),
    ),
    1,
  );
  const world = client.worldReference;
  if (!world) throw new Error("Presented World has no exact reference");
  return host.bindOutput(world, camera, "camera");
}

/** Register fetched immutable bytes through the parent World session. */
export async function loadAsset(
  client: Client,
  kind: number,
  path: string,
): Promise<ClientAssetSource> {
  if (!("createAsset" in client))
    throw new Error("GUI fixture requires client-authored assets");
  const response = await fetch(new URL(path, globalThis.location.href));
  if (!response.ok) throw new Error(`Missing GUI fixture asset ${path}`);
  return (
    client as Client & {
      createAsset(kind: number, bytes: ArrayBuffer): Promise<ClientAssetSource>;
    }
  ).createAsset(kind, await response.arrayBuffer());
}

/** An attached World's authoring session on the IppCanvas Host. */
export function attachedSession(
  handle: IppCanvasHandle,
  world: WorldReference,
): Client & GuiWorldClient {
  const session = [...handle.host.sessions.values()].find(
    (candidate) =>
      candidate.worldReference?.id === world.id &&
      candidate.worldReference.incarnation === world.incarnation,
  );
  if (!session) throw new Error("Attached World has no authoring session");
  return session as Client & GuiWorldClient;
}

/** One completed frame of the presented view. Without `flush` the frame is
 * captured without waiting for React acknowledgements. */
export async function presentedFrame(handle: IppCanvasHandle, flush = true) {
  const view = handle.view;
  const viewport = handle.viewport;
  if (!view || !viewport) throw new Error("GUI fixture has no presented view");
  const frame = flush
    ? await handle.capture()
    : await handle.host.presentation.capture(view);
  const width = Math.round(viewport.width * viewport.devicePixelRatio);
  const height = Math.round(viewport.height * viewport.devicePixelRatio);
  const pixels = new Uint8Array(frame.pixels);
  if (pixels.length !== width * height * 4)
    throw new Error(
      `Capture size ${pixels.length} does not match ${width}x${height}`,
    );
  return {
    width,
    height,
    pixels,
    drawCalls: frame.drawCalls,
    failedDrawCalls: frame.failedDrawCalls,
  };
}

/** Average display RGB over a 7x7 block around one pixel. */
export function averageRgb(
  pixels: Uint8Array,
  width: number,
  height: number,
  x: number,
  y: number,
): readonly [number, number, number] {
  const sum: [number, number, number] = [0, 0, 0];
  let count = 0;
  for (let py = Math.max(0, y - 3); py <= Math.min(height - 1, y + 3); py++)
    for (let px = Math.max(0, x - 3); px <= Math.min(width - 1, x + 3); px++) {
      const offset = (py * width + px) * 4;
      sum[0] += pixels[offset]!;
      sum[1] += pixels[offset + 1]!;
      sum[2] += pixels[offset + 2]!;
      count += 1;
    }
  return [
    Math.round(sum[0] / count),
    Math.round(sum[1] / count),
    Math.round(sum[2] / count),
  ];
}

/** Wait on animation frames for a fixture condition. */
export async function until(
  predicate: () => boolean,
  message: () => string,
): Promise<void> {
  const deadline = performance.now() + 10_000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message());
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
}

/** Wait until the parent World reports its client-authored assets loaded. */
export async function assetsLoaded(
  client: Client,
  sources: readonly string[],
): Promise<void> {
  const expected = new Set(sources);
  const deadline = performance.now() + 10_000;
  for (;;) {
    const loaded = (await client.inspect()).resources.filter((resource) =>
      expected.has(resource.source),
    );
    const failed = loaded.find((asset) => asset.status === "failed");
    if (failed !== undefined)
      throw new Error(
        `GUI fixture asset failed (${failed.source}): ${failed.error ?? "unknown"}`,
      );
    if (
      loaded.length === expected.size &&
      loaded.every((asset) => asset.status === "loaded")
    )
      return;
    if (performance.now() >= deadline)
      throw new Error("GUI fixture assets did not load");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
}
