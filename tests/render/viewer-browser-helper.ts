import type {
  FrameCapture,
  AnimationWorldClient,
  GuiSemanticAction,
  GuiSemanticNode,
  GuiSemanticRole,
  GuiSemanticTree,
  GuiPartProperty,
  GuiWorldClient,
  PickingWorldClient,
  SurfaceWorldClient,
  SystemQuery,
} from "@ipp/client";
import { guiPartProperty } from "@ipp/client";
import type { IppCanvasHandle } from "@ipp/react/web";
import {
  compareImages,
  type ImageDifference,
  type ImageSummary,
  summarizeImage,
} from "./image-assertions.js";
import {
  captureViewer as captureCanvas,
  observeViewer as observeCanvas,
  type ViewerObservation,
} from "./viewer-observation.js";

interface ViewerWindow extends Window {
  ippWorldCanvas?: IppCanvasHandle;
}

export interface ViewerBrowserCapture extends ViewerObservation {
  readonly label: string;
  readonly frame: Omit<FrameCapture, "pixels">;
  readonly summary: ImageSummary;
  readonly dataUrl: string;
}

export interface PlaneUvProbe {
  readonly u: number;
  readonly v: number;
}

export interface PlaneUvProbeEvidence extends PlaneUvProbe {
  readonly coordinate: readonly [number, number];
  readonly rgba: readonly [number, number, number, number];
  readonly neighborhood: readonly (readonly [number, number, number, number])[];
}

const captures = new Map<string, FrameCapture>();

export interface GalleryGuiSelector {
  readonly role: GuiSemanticRole;
  readonly name?: string;
}

function selectGuiNode(
  tree: GuiSemanticTree,
  selector: GalleryGuiSelector,
): GuiSemanticNode {
  const matches = tree.nodes.filter(
    (node) =>
      node.role === selector.role &&
      (selector.name === undefined || node.name === selector.name),
  );
  if (matches.length !== 1)
    throw new Error(
      `Expected one ${selector.role} '${selector.name ?? ""}', found ${matches.length}`,
    );
  return matches[0]!;
}

async function galleryGuiContext(flush = true) {
  const handle = requireCanvas();
  if (flush) await handle.flush();
  const client = handle.client as GuiWorldClient & SurfaceWorldClient;
  const inspection = await client.inspect();
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === "gui-demo",
  );
  if (!entity) throw new Error("Missing gallery GUI demo");
  const surface = entity.effective.find(
    ({ component }) => component === client.components.Surface!.id,
  );
  if (!surface) throw new Error("Missing effective GUI demo Surface");
  const semantic = await client.semanticSnapshot({
    entity: entity.id,
    maxDepth: 32,
    limit: 256,
  });
  const detailed = await client.inspectGui({
    entity: entity.id,
    maxDepth: 32,
    limit: 256,
  });
  return { handle, client, inspection, entity, surface, semantic, detailed };
}

/** Public GUI observations plus proof that GuiRoot is the sole Surface producer. */
export async function galleryGuiState(flush = true) {
  const { client, entity, surface, semantic, detailed } =
    await galleryGuiContext(flush);
  const bytes = surface.fields.items;
  if (!(bytes instanceof Uint8Array))
    throw new Error("GUI demo Surface items are not encoded bytes");
  return {
    semantic,
    detailed,
    surfaceItems: client.decodeSurfaceItems(bytes).items.length,
    entityComponents: entity.effective.map(
      ({ component }) =>
        Object.entries(client.components).find(
          ([, descriptor]) => descriptor.id === component,
        )?.[0],
    ),
  };
}

async function pointsForNode(
  node: GuiSemanticNode,
  fractions: readonly (readonly [number, number])[],
) {
  const { surface } = await galleryGuiContext();
  const width = Number(surface.fields.width);
  const height = Number(surface.fields.height);
  const [x, y, nodeWidth, nodeHeight] = node.bounds;
  const points = await projectGalleryPoints(
    "gui-demo",
    fractions.map(([fractionX, fractionY]) => {
      const logicalX = x + nodeWidth * fractionX;
      const logicalY = y + nodeHeight * fractionY;
      return [logicalX - width / 2, height / 2 - logicalY, 0];
    }),
  );
  return points.map((point) => ({ ...point, node }));
}

async function pointForNode(
  node: GuiSemanticNode,
  fractionX: number,
  fractionY: number,
) {
  const [point] = await pointsForNode(node, [[fractionX, fractionY]]);
  if (!point) throw new Error("GUI demo GUI point did not project");
  return point;
}

/** Project an acknowledged semantic control bound through the actual camera. */
export async function galleryGuiPoint(
  selector: GalleryGuiSelector,
  fractionX = 0.5,
  fractionY = 0.5,
) {
  const { semantic } = await galleryGuiContext();
  return pointForNode(selectGuiNode(semantic, selector), fractionX, fractionY);
}

/** Read one evaluated named skin-part property for a semantic GUI node. */
export async function galleryGuiPartValue(
  selector: GalleryGuiSelector,
  part: string,
  property: GuiPartProperty,
) {
  const { client, entity, semantic } = await galleryGuiContext();
  const node = selectGuiNode(semantic, selector);
  const root = entity.effective.find(
    ({ component }) => component === client.components.GuiRoot!.id,
  );
  if (!root) throw new Error("Missing effective GUI demo GuiRoot");
  const value = root.properties?.[guiPartProperty(node.id, part, property)];
  if (!value)
    throw new Error(`Missing ${node.id}:${part}:${property} GUI part value`);
  return value;
}

/** Project a semantic control rectangle into normalized completed-frame bounds. */
export async function galleryGuiRegion(
  selector: GalleryGuiSelector,
  insetX = 0.05,
  insetY = 0.1,
): Promise<readonly [number, number, number, number]> {
  const { semantic } = await galleryGuiContext();
  const node = selectGuiNode(semantic, selector);
  const corners = await pointsForNode(node, [
    [insetX, insetY],
    [1 - insetX, insetY],
    [insetX, 1 - insetY],
    [1 - insetX, 1 - insetY],
  ]);
  return [
    Math.min(...corners.map(({ x }) => x)),
    Math.min(...corners.map(({ y }) => y)),
    Math.max(...corners.map(({ x }) => x)),
    Math.max(...corners.map(({ y }) => y)),
  ];
}

/** Sample completed GUI paint through the actual Surface and camera transforms. */
export async function sampleGalleryGuiCapture(
  label: string,
  logicalPoints: readonly (readonly [number, number])[],
) {
  const { surface } = await galleryGuiContext();
  const width = Number(surface.fields.width);
  const height = Number(surface.fields.height);
  const projected = await projectGalleryPoints(
    "gui-demo",
    logicalPoints.map(([x, y]) => [x - width / 2, height / 2 - y, 0]),
  );
  const frame = requireCapture(label);
  return projected.map(({ x, y }) =>
    sample(
      frame,
      Math.min(frame.width - 1, Math.max(0, Math.floor(x * frame.width))),
      Math.min(frame.height - 1, Math.max(0, Math.floor(y * frame.height))),
    ),
  );
}

/** Sample completed frame pixels independently of scene geometry. */
export function sampleViewerCapture(
  label: string,
  points: readonly (readonly [number, number])[],
) {
  const frame = requireCapture(label);
  return points.map(([x, y]) =>
    sample(
      frame,
      Math.min(frame.width - 1, Math.max(0, Math.floor(x * frame.width))),
      Math.min(frame.height - 1, Math.max(0, Math.floor(y * frame.height))),
    ),
  );
}

/** Move the same panel temporarily to expose the real backdrop under its corners. */
export async function setGalleryGuiX(x: number): Promise<number> {
  const { client, entity } = await galleryGuiContext();
  const component = client.components.Transform!;
  const previous = Number(
    entity.effective.find(({ component: id }) => id === component.id)!.fields.x,
  );
  const result = await client.batch([
    {
      kind: "setField",
      entity: { kind: "handle", id: entity.id },
      component: component.id,
      field: {
        offset: component.fields.x!.offset,
        value: { kind: "f32", value: x },
      },
    },
  ]);
  if (!result.ok)
    throw new Error(`GUI capture placement failed: ${result.error.reason}`);
  return previous;
}

/** Drive an acknowledged controller through the production animation protocol. */
export async function controlGalleryAnimation(
  id: bigint | readonly bigint[],
  control: import("@ipp/client").AnimationPlaybackControl,
) {
  const handle = requireCanvas();
  const client = handle.client as import("@ipp/client").AnimationWorldClient;
  await Promise.all(
    (typeof id === "bigint" ? [id] : id).map((controller) =>
      client.controlAnimationController(controller, control),
    ),
  );
  await handle.flush();
}

/** Project the maintained ScrollView without application-specific geometry. */
export async function galleryGuiScrollPoint() {
  const { semantic, detailed } = await galleryGuiContext();
  const scrollViews = detailed.nodes.filter(
    ({ content, style }) =>
      content.kind === "container" &&
      content.containerKind === "scrollView" &&
      style.enabled !== false,
  );
  if (scrollViews.length !== 1)
    throw new Error(
      `Expected one enabled ScrollView, found ${scrollViews.length}`,
    );
  const semanticNode = semantic.nodes.find(
    ({ id }) => id === scrollViews[0]!.id,
  );
  if (!semanticNode) throw new Error("ScrollView is absent from semantics");
  return pointForNode(semanticNode, 0.5, 0.72);
}

/** Dispatch one revision-fenced semantic action for machine-access testing. */
export async function galleryGuiAction(
  selector: GalleryGuiSelector,
  action: GuiSemanticAction,
) {
  const { handle, client, semantic } = await galleryGuiContext();
  const node = selectGuiNode(semantic, selector);
  await client.semanticAction({
    entity: semantic.entity,
    rootIncarnation: semantic.rootIncarnation,
    node: node.id,
    lifetime: node.lifetime,
    expectedRevision: node.revision,
    action,
  });
  await handle.flush();
  return client.semanticSnapshot({
    entity: semantic.entity,
    maxDepth: 32,
    limit: 256,
  });
}

/** Execute the gallery's actual generated camera query path for geometry assertions. */
export function cameraQuery(input: SystemQuery) {
  return (requireCanvas().client as PickingWorldClient).query(input);
}

/** Independent projection oracle, then a real query to find an unobscured target. */
export async function locateGalleryObject(symbol: string) {
  const offsets =
    symbol === "lighting-skinning"
      ? [
          [0, 0.85, 0],
          [0, 0, 0],
        ]
      : symbol === "lighting-spot"
        ? [[0, 0, -0.35]]
        : [
            [0, 0, 0],
            [0, 0.45, 0],
            [-0.4, 0, 0],
            [0.4, 0, 0],
            [0, -0.4, 0],
          ];
  for (const point of await projectGalleryPoints(symbol, offsets)) {
    const { x, y, viewport } = point;
    if (x < 0 || x > 1 || y < 0 || y > 1) continue;
    const result = await (requireCanvas().client as PickingWorldClient).query({
      type: "GeometryPickQuery",
      x,
      y,
      ...viewport,
      includeViewPlane: true,
    });
    if (result.ok && result.hit?.entity === point.entity)
      return { ...point, hit: result.hit, camera: result.camera };
  }
  throw new Error(`No visible pick point for ${symbol}`);
}

/** Project explicit object-space probes independently of the runtime's picking shape. */
export async function projectGalleryPoints(
  symbol: string,
  offsets: number[][],
) {
  const handle = requireCanvas();
  await handle.flush();
  const inspection = await handle.client.inspect();
  const entity = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === symbol,
  );
  const camera = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === "gallery-camera",
  );
  if (!entity || !camera) throw new Error(`Missing gallery object ${symbol}`);
  const fields = (entity: typeof camera, name: string) =>
    entity.effective.find(
      (entry) => entry.component === handle.client.components[name]!.id,
    )!.fields;
  const object = fields(entity, "Transform");
  const view = fields(camera, "Transform");
  const projection = fields(camera, "Camera");
  const rotate = (v: number[], q: number[]) => {
    const [x, y, z] = v as [number, number, number];
    const [qx, qy, qz, qw] = q as [number, number, number, number];
    const t = [
      2 * (qy * z - qz * y),
      2 * (qz * x - qx * z),
      2 * (qx * y - qy * x),
    ];
    return [
      x + qw * t[0]! + qy * t[2]! - qz * t[1]!,
      y + qw * t[1]! + qz * t[0]! - qx * t[2]!,
      z + qw * t[2]! + qx * t[1]! - qy * t[0]!,
    ];
  };
  const bounds = document
    .querySelector<HTMLCanvasElement>("#ipp-world-canvas")!
    .getBoundingClientRect();
  return offsets.map((local) => {
    const point = rotate(
      local.map((v, axis) => v * Number(object[["sx", "sy", "sz"][axis]!])),
      ["qx", "qy", "qz", "qw"].map((key) => Number(object[key])),
    );
    const relative = point.map(
      (v, axis) =>
        v +
        Number(object[["x", "y", "z"][axis]!]) -
        Number(view[["x", "y", "z"][axis]!]),
    );
    const cameraPoint = rotate(relative, [
      -Number(view.qx),
      -Number(view.qy),
      -Number(view.qz),
      Number(view.qw),
    ]);
    const halfHeight =
      Number(projection.projection) === 1
        ? Number(projection.ortho_height) / 2
        : -cameraPoint[2]! * Math.tan(Number(projection.fov_y) / 2);
    const x =
      0.5 +
      cameraPoint[0]! /
        ((2 * halfHeight * handle.viewport.width) / handle.viewport.height);
    const y = 0.5 - cameraPoint[1]! / (2 * halfHeight);
    return {
      entity: entity.id,
      x,
      y,
      clientX: bounds.left + x * bounds.width,
      clientY: bounds.top + y * bounds.height,
      viewport: handle.viewport,
    };
  });
}

/** Wait for browser input batching, then use real inspection as an ingress barrier. */
export async function settleGalleryInput() {
  await nextFrame(10_000);
  await nextFrame(10_000);
  return requireCanvas().client.inspect();
}

let heldReply:
  | { release(): void; arrived: boolean; outcome?: unknown }
  | undefined;
let heldPresentationCapture:
  | { release(): void; arrived: boolean; outcome?: unknown }
  | undefined;

/** Delay delivery after a real worker query completes, exercising late input races. */
export function delayNextCameraQuery(
  type: SystemQuery["type"] = "GeometryPickQuery",
) {
  if (heldReply) throw new Error("A pick reply is already delayed");
  const client = requireCanvas().client as PickingWorldClient;
  const query = client.query;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      client.query = query;
      release();
      heldReply = undefined;
    },
    arrived: false,
  };
  heldReply = state;
  client.query = (async (input: SystemQuery) => {
    if (input.type !== type) return query.call(client, input);
    client.query = query;
    const result = await query.call(client, input);
    state.arrived = true;
    await gate;
    return result;
  }) as PickingWorldClient["query"];
}

/** Hold a real committed batch acknowledgement to exercise page startup cancellation. */
export function delayNextComponentBatch(name: string) {
  if (heldReply) throw new Error("A reply is already delayed");
  const client = requireCanvas().client;
  const batch = client.batch;
  const component = client.components[name]!.id;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      client.batch = batch;
      release();
      heldReply = undefined;
    },
    arrived: false,
    outcome: undefined as unknown,
  };
  heldReply = state;
  client.batch = async (operations, batchId) => {
    if (
      !operations.some(
        (operation) =>
          operation.kind === "insertComponent" &&
          operation.component === component,
      )
    )
      return batch.call(client, operations, batchId);
    client.batch = batch;
    const outcome = await batch.call(client, operations, batchId);
    state.outcome = outcome;
    state.arrived = true;
    await gate;
    return outcome;
  };
}

/** Hold the next real component batch after the Host commits it. */
export function delayNextBatch() {
  if (heldReply) throw new Error("A reply is already delayed");
  const client = requireCanvas().client;
  const batch = client.batch;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      client.batch = batch;
      release();
      heldReply = undefined;
    },
    arrived: false,
    outcome: undefined as unknown,
  };
  heldReply = state;
  client.batch = async (operations, batchId) => {
    client.batch = batch;
    const outcome = await batch.call(client, operations, batchId);
    state.outcome = outcome;
    state.arrived = true;
    await gate;
    return outcome;
  };
}

/** Hold the next real GUI edit acknowledgement after the Host commits it. */
export function delayNextGuiBatch() {
  if (heldReply) throw new Error("A reply is already delayed");
  const client = requireCanvas().client as GuiWorldClient;
  const editGuiBatch = client.editGuiBatch;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      client.editGuiBatch = editGuiBatch;
      release();
      heldReply = undefined;
    },
    arrived: false,
    outcome: undefined as unknown,
  };
  heldReply = state;
  client.editGuiBatch = async (edits) => {
    client.editGuiBatch = editGuiBatch;
    const outcome = await editGuiBatch.call(client, edits);
    state.outcome = outcome;
    state.arrived = true;
    await gate;
    return outcome;
  };
}

/** Delay a completed renderer capture so startup readiness cannot precede it. */
export function delayNextPresentationCapture() {
  if (heldPresentationCapture)
    throw new Error("A presentation capture is already delayed");
  const presentation = requireCanvas().client.presentation;
  if (!presentation) throw new Error("The gallery renderer is unavailable");
  const capture = presentation.capture;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      presentation.capture = capture;
      release();
      heldPresentationCapture = undefined;
    },
    arrived: false,
    outcome: undefined as unknown,
  };
  heldPresentationCapture = state;
  presentation.capture = async (afterTick) => {
    presentation.capture = capture;
    const frame = await capture.call(presentation, afterTick);
    state.outcome = {
      session: frame.session,
      tick: frame.tick,
      width: frame.width,
      height: frame.height,
    };
    state.arrived = true;
    await gate;
    return frame;
  };
}

/** Delay the reply from real controller creation to exercise cancellation cleanup. */
export function delayNextControllerCreation() {
  if (heldReply) throw new Error("A reply is already delayed");
  const client = requireCanvas().client as AnimationWorldClient;
  const create = client.createAnimationController;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      client.createAnimationController = create;
      release();
      heldReply = undefined;
    },
    arrived: false,
    outcome: undefined as unknown,
  };
  heldReply = state;
  client.createAnimationController = async (description) => {
    client.createAnimationController = create;
    const id = await create.call(client, description);
    state.outcome = { id };
    state.arrived = true;
    await gate;
    return id;
  };
}

/** Delay a real animation-control reply after the Host has committed it. */
export function delayNextAnimationControl(
  action: "play" | "pause" | "stop" | "restart" | "seek" | "playAtSpeed",
) {
  if (heldReply) throw new Error("A reply is already delayed");
  const client = requireCanvas().client as AnimationWorldClient;
  const control = client.controlAnimationController;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      client.controlAnimationController = control;
      release();
      heldReply = undefined;
    },
    arrived: false,
    outcome: undefined as unknown,
  };
  heldReply = state;
  client.controlAnimationController = async (id, input) => {
    if (input.action !== action) return control.call(client, id, input);
    client.controlAnimationController = control;
    await control.call(client, id, input);
    state.outcome = { id, action };
    state.arrived = true;
    await gate;
  };
}

export function heldReplyOutcome() {
  return heldReply?.outcome;
}

export function queryReplyHeld() {
  return heldReply?.arrived === true;
}

export function releaseQuery() {
  heldReply?.release();
}

export function presentationCaptureHeld() {
  return heldPresentationCapture?.arrived === true;
}

export function releasePresentationCapture() {
  heldPresentationCapture?.release();
}

export function countViewerColors(
  label: string,
  colors: readonly (readonly [number, number, number])[],
): number[] {
  const pixels = new Uint8Array(requireCapture(label).pixels);
  return colors.map((color) => {
    let count = 0;
    for (let offset = 0; offset < pixels.length; offset += 4) {
      if (
        color.every(
          (channel, i) => Math.abs(channel - pixels[offset + i]!) <= 4,
        )
      )
        count++;
    }
    return count;
  });
}

export async function waitForViewer(): Promise<ViewerObservation> {
  const deadline = performance.now() + 10_000;
  let handle = (window as ViewerWindow).ippWorldCanvas;
  while (handle === undefined) {
    const status = document.querySelector<HTMLOutputElement>("#status");
    if (status?.dataset.state === "error") {
      throw new Error(
        status.textContent?.trim() || "World gallery failed to start",
      );
    }
    const remaining = deadline - performance.now();
    if (remaining <= 0) throw new Error("World gallery did not become ready");
    await nextFrame(remaining);
    handle = (window as ViewerWindow).ippWorldCanvas;
  }
  const ready = await captureCanvas(handle);
  return {
    session: ready.session,
    inspection: ready.inspection,
    componentIds: ready.componentIds,
  };
}

export async function observeViewer(): Promise<ViewerObservation> {
  return await observeCanvas(requireCanvas());
}

export async function captureViewer(
  label: string,
  waitForResources = true,
): Promise<ViewerBrowserCapture> {
  if (!label) throw new Error("Capture label must be nonempty");
  const { frame, ...observation } = await captureCanvas(requireCanvas(), {
    waitForResources,
  });
  const stored = { ...frame, pixels: frame.pixels.slice(0) };
  captures.set(label, stored);
  const { pixels: _pixels, ...metadata } = frame;
  return {
    ...observation,
    label,
    frame: metadata,
    summary: summarizeImage(frame),
    dataUrl: await frameDataUrl(frame),
  };
}

/** Capture committed presentation state without waiting for React reconciliation. */
export async function captureUnflushedViewer(label: string) {
  if (!label) throw new Error("Capture label must be nonempty");
  const handle = requireCanvas();
  const presentation = handle.client.presentation;
  if (!presentation) throw new Error("The gallery renderer is unavailable");
  const inspection = await handle.client.inspect();
  const frame = await presentation.capture(inspection.tick);
  captures.set(label, { ...frame, pixels: frame.pixels.slice(0) });
  const { pixels: _pixels, ...metadata } = frame;
  return { label, frame: metadata, summary: summarizeImage(frame) };
}

export function analyzePlaneCapture(label: string) {
  const frame = requireCapture(label);
  const pixels = new Uint8Array(frame.pixels);
  let surfacePixels = 0;
  let arrowPixels = 0;
  let surfaceBottom = -1;
  let arrowBottom = -1;
  for (let y = 0; y < frame.height; y += 1) {
    for (let x = 0; x < frame.width; x += 1) {
      const offset = (y * frame.width + x) * 4;
      const r = pixels[offset] ?? 0;
      const g = pixels[offset + 1] ?? 0;
      const b = pixels[offset + 2] ?? 0;
      const minimum = Math.min(r, g, b);
      const maximum = Math.max(r, g, b);
      if (minimum >= 220) {
        arrowPixels += 1;
        arrowBottom = y;
      } else if (minimum >= 100 && maximum <= 200 && maximum - minimum <= 4) {
        surfacePixels += 1;
        surfaceBottom = y;
      }
    }
  }
  return {
    surfacePixels,
    arrowPixels,
    surfaceBottom,
    arrowBottom,
  };
}

export function samplePlaneUvCapture(
  label: string,
  probes: readonly PlaneUvProbe[],
): readonly PlaneUvProbeEvidence[] {
  const frame = requireCapture(label);
  return probes.map(({ u, v }) => {
    if (u < 0 || u > 1 || v < 0 || v > 1) {
      throw new RangeError(`Plane UV (${u}, ${v}) is outside 0..1`);
    }
    const coordinate = projectPlanePoint(frame, 2 * u - 1, 1 - 2 * v);
    const center = coordinate.map(Math.floor) as [number, number];
    const neighborhood: [number, number, number, number][] = [];
    for (let y = center[1] - 1; y <= center[1] + 1; y += 1) {
      for (let x = center[0] - 1; x <= center[0] + 1; x += 1) {
        if (x >= 0 && y >= 0 && x < frame.width && y < frame.height) {
          neighborhood.push(sample(frame, x, y));
        }
      }
    }
    return {
      u,
      v,
      coordinate: center,
      rgba: sample(frame, center[0], center[1]),
      neighborhood,
    };
  });
}

export function compareViewerCaptures(
  first: string,
  second: string,
): ImageDifference {
  return compareImages(requireCapture(first), requireCapture(second));
}

/** Compare a normalized canvas region while ignoring independently changing UI. */
export function compareViewerCaptureRegion(
  first: string,
  second: string,
  bounds: readonly [number, number, number, number],
): ImageDifference {
  const a = requireCapture(first);
  const b = requireCapture(second);
  if (a.width !== b.width || a.height !== b.height)
    throw new Error("image dimensions differ");
  const [u0, v0, u1, v1] = bounds;
  const left = Math.max(0, Math.floor(a.width * u0));
  const top = Math.max(0, Math.floor(a.height * v0));
  const right = Math.min(a.width, Math.ceil(a.width * u1));
  const bottom = Math.min(a.height, Math.ceil(a.height * v1));
  if (!(left < right && top < bottom)) throw new Error("empty image region");
  const ap = new Uint8Array(a.pixels);
  const bp = new Uint8Array(b.pixels);
  let changedPixels = 0;
  let absoluteDifference = 0;
  for (let y = top; y < bottom; y += 1)
    for (let x = left; x < right; x += 1) {
      const offset = (y * a.width + x) * 4;
      let changed = false;
      for (let channel = 0; channel < 3; channel += 1) {
        const difference = Math.abs(
          (ap[offset + channel] ?? 0) - (bp[offset + channel] ?? 0),
        );
        absoluteDifference += difference;
        changed ||= difference > 6;
      }
      if (changed) changedPixels += 1;
    }
  const totalPixels = (right - left) * (bottom - top);
  return {
    changedPixels,
    changedFraction: changedPixels / totalPixels,
    meanAbsoluteChannelDifference: absoluteDifference / (totalPixels * 3),
  };
}

/** Measure a known unlit marker color in a completed rendered frame. */
export function captureColorRegion(
  label: string,
  rgb: readonly number[],
  bounds = [0, 0, 1, 1],
) {
  const frame = requireCapture(label);
  const pixels = new Uint8Array(frame.pixels);
  let count = 0,
    sumX = 0,
    sumY = 0;
  let left = frame.width,
    top = frame.height,
    right = -1,
    bottom = -1;
  for (let offset = 0; offset < pixels.length; offset += 4) {
    const pixel = offset / 4;
    const x = pixel % frame.width;
    const y = Math.floor(pixel / frame.width);
    if (
      x < bounds[0]! * frame.width ||
      y < bounds[1]! * frame.height ||
      x >= bounds[2]! * frame.width ||
      y >= bounds[3]! * frame.height
    )
      continue;
    if (
      !rgb.every(
        (value, channel) => Math.abs(pixels[offset + channel]! - value) <= 1,
      )
    )
      continue;
    count++;
    sumX += x;
    sumY += y;
    left = Math.min(left, x);
    top = Math.min(top, y);
    right = Math.max(right, x);
    bottom = Math.max(bottom, y);
  }
  return {
    count,
    x: count ? sumX / count : 0,
    y: count ? sumY / count : 0,
    bounds: count ? { left, top, right, bottom } : null,
  };
}

export function countCaptureColors(label: string) {
  const pixels = new Uint8Array(requireCapture(label).pixels);
  const counts = { red: 0, green: 0, blue: 0, white: 0, yellow: 0, magenta: 0 };
  for (let i = 0; i < pixels.length; i += 4) {
    const r = pixels[i]!;
    const g = pixels[i + 1]!;
    const b = pixels[i + 2]!;
    if (r > 240 && g < 15 && b < 15) counts.red++;
    if (g > 240 && r < 15 && b < 15) counts.green++;
    if (b > 240 && r < 15 && g < 15) counts.blue++;
    if (r > 240 && g > 240 && b > 240) counts.white++;
    if (r > 240 && g > 200 && b < 15) counts.yellow++;
    if (r > 240 && b > 240 && g < 15) counts.magenta++;
  }
  return counts;
}

function requireCanvas(): IppCanvasHandle {
  const handle = (window as ViewerWindow).ippWorldCanvas;
  if (handle === undefined) throw new Error("World gallery is not ready");
  return handle;
}

function requireCapture(label: string): FrameCapture {
  const frame = captures.get(label);
  if (!frame) throw new Error(`Missing viewer capture '${label}'`);
  return frame;
}

function projectPlanePoint(
  frame: FrameCapture,
  x: number,
  y: number,
): readonly [number, number] {
  const rotation = Math.PI / 4;
  const world = [x, Math.cos(rotation) * y, Math.sin(rotation) * y] as const;
  const eye = [3, 2, 5] as const;
  const distance = Math.hypot(...eye);
  const back = eye.map((value) => value / distance);
  const rightLength = Math.hypot(back[2]!, back[0]!);
  const right = [back[2]! / rightLength, 0, -back[0]! / rightLength];
  const up = [
    back[1]! * right[2]!,
    back[2]! * right[0]! - back[0]! * right[2]!,
    -back[1]! * right[0]!,
  ];
  const relative = world.map((value, index) => value - eye[index]!);
  const viewX = dot(right, relative);
  const viewY = dot(up, relative);
  const viewZ = dot(back, relative);
  const projection = 1 / Math.tan(Math.PI / 8);
  const ndcX = (projection * viewX) / (-viewZ * (frame.width / frame.height));
  const ndcY = (projection * viewY) / -viewZ;
  return [((ndcX + 1) * frame.width) / 2, ((1 - ndcY) * frame.height) / 2];
}

function dot(a: readonly number[], b: readonly number[]): number {
  return a.reduce((total, value, index) => total + value * b[index]!, 0);
}

function sample(
  frame: FrameCapture,
  x: number,
  y: number,
): [number, number, number, number] {
  const pixels = new Uint8Array(frame.pixels);
  const offset = (y * frame.width + x) * 4;
  return [
    pixels[offset] ?? 0,
    pixels[offset + 1] ?? 0,
    pixels[offset + 2] ?? 0,
    pixels[offset + 3] ?? 0,
  ];
}

async function frameDataUrl(frame: FrameCapture): Promise<string> {
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Browser does not expose a 2D canvas context");
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

async function nextFrame(timeoutMs: number): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const frame = requestAnimationFrame(() => {
      clearTimeout(timeout);
      resolve();
    });
    const timeout = setTimeout(() => {
      cancelAnimationFrame(frame);
      reject(new Error("World gallery did not become ready"));
    }, timeoutMs);
  });
}
