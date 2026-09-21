import {
  type CanvasRuntimeInput,
  type TransferObservation,
  fields,
  waitForResource,
  requireComponent,
  entity,
  numericField,
  installTransferObserver,
  releaseTransferObserver,
  transferObservation,
  waitUntil,
  animationBarrier,
} from "./canvas-fixture-helpers.js";
export type {
  CanvasRuntimeInput,
  TransferObservation,
} from "./canvas-fixture-helpers.js";
export { transferObservation } from "./canvas-fixture-helpers.js";

import { activateFixtureCamera } from "../integration/camera-fixtures.js";
import {
  createContext,
  StrictMode,
  useCallback,
  useContext,
  useMemo,
  type ReactNode,
} from "react";
import { createRoot, type Root as ReactDomRoot } from "react-dom/client";
import type { FrameCapture } from "@ipp/client";
import type { Command, EntitySnapshot } from "@ipp/client";
import { Entity, Transform, UnlitMaterial } from "@ipp/react";
import {
  IppCanvas,
  World,
  useIppCanvas,
  type CanvasRuntimeConfiguration,
  type IppCanvasHandle,
} from "@ipp/react/web";
import {
  compareImages,
  summarizeImage,
  type ImageDifference,
  type ImageSummary,
} from "./image-assertions.js";

const WIDTH = 320;
const HEIGHT = 240;
const OPERATION_TIMEOUT_MS = 10_000;
const CUBE_RECIPE = "ipp://mesh/cube?width=2&height=2&length=2";
const CANVAS_IDS = ["left", "right"] as const;

type CanvasId = (typeof CANVAS_IDS)[number];

interface SharedSceneValue {
  readonly x: number;
  readonly scale: number;
  readonly color: readonly [number, number, number];
}

interface CanvasModel {
  asyncCommitFailure: Record<CanvasId, boolean>;
  callbackRevision: number;
  dimensions: Record<
    CanvasId,
    { readonly width: number; readonly height: number }
  >;
  cssDimensions: Record<
    CanvasId,
    { readonly width: number; readonly height: number } | undefined
  >;
  mounted: Record<CanvasId, boolean>;
  sceneMounted: Record<CanvasId, boolean>;
  invalid: Record<CanvasId, boolean>;
  shared: SharedSceneValue;
}

interface FixtureController {
  readonly configurations: Record<CanvasId, CanvasRuntimeConfiguration>;
  readonly handles: Map<CanvasId, IppCanvasHandle>;
  readonly prepared: Map<CanvasId, IppCanvasHandle>;
  readonly preparing: Map<CanvasId, Promise<void>>;
  readonly captures: Map<string, FrameCapture>;
  readonly commits: Record<CanvasId, number>;
  readonly errors: Record<CanvasId, string[]>;
  readonly model: CanvasModel;
  root: ReactDomRoot;
  strict: boolean;
}

export interface CanvasObservation {
  readonly id: CanvasId;
  readonly session: bigint;
  readonly entityIds: readonly string[];
  readonly runtimeEntityIds: readonly {
    readonly symbolicId: string;
    readonly id: bigint;
  }[];
  readonly producer: {
    readonly baseX: number | null;
    readonly effectiveX: number | null;
    readonly baseColor: readonly number[] | null;
    readonly effectiveColor: readonly number[] | null;
  };
  readonly ownedExists: boolean;
  readonly commitCount: number;
  readonly errors: readonly string[];
  readonly width: number;
  readonly height: number;
}

export interface CanvasCaptureReport {
  readonly label: string;
  readonly id: CanvasId;
  readonly session: bigint;
  readonly tick: bigint;
  readonly drawCalls: number;
  readonly triangles: number;
  readonly contextGeneration: number;
  readonly summary: ImageSummary;
  readonly observation: CanvasObservation;
}

export interface CanvasLayoutObservation {
  readonly observation: CanvasObservation;
  readonly cssWidth: number;
  readonly cssHeight: number;
  readonly attributeWidth: number;
  readonly attributeHeight: number;
  readonly devicePixelRatio: number;
  readonly transfers: TransferObservation;
}

const ApplicationContext = createContext<SharedSceneValue | undefined>(
  undefined,
);
const SceneContext = createContext<SharedSceneValue | undefined>(undefined);

let active: FixtureController | undefined;
let pendingRoot: ReactDomRoot | undefined;
let pendingReadyCount = 0;
let pendingErrors: string[] = [];
export async function mountCanvasApplication(
  configuration: CanvasRuntimeInput,
  strict: boolean,
): Promise<void> {
  await closeCanvasApplication();
  installTransferObserver();
  const host = document.createElement("div");
  host.id = "canvas-application";
  document.body.replaceChildren(host);
  const controller: FixtureController = {
    configurations: {
      left: Object.freeze({ ...configuration }),
      right: Object.freeze({ ...configuration }),
    },
    handles: new Map(),
    prepared: new Map(),
    preparing: new Map(),
    captures: new Map(),
    commits: { left: 0, right: 0 },
    errors: { left: [], right: [] },
    model: {
      asyncCommitFailure: { left: false, right: false },
      callbackRevision: 0,
      dimensions: {
        left: { width: WIDTH, height: HEIGHT },
        right: { width: WIDTH, height: HEIGHT },
      },
      cssDimensions: { left: undefined, right: undefined },
      mounted: { left: true, right: true },
      sceneMounted: { left: true, right: true },
      invalid: { left: false, right: false },
      shared: Object.freeze({
        x: -0.55,
        scale: 0.72,
        color: Object.freeze([0.15, 0.82, 0.28] as const),
      }),
    },
    root: createRoot(host),
    strict,
  };
  active = controller;
  renderApplication(controller);
  await animationBarrier();
}

export async function waitForCanvasApplication(): Promise<
  readonly CanvasObservation[]
> {
  const controller = requireActive();
  try {
    await waitUntil(
      () =>
        CANVAS_IDS.every((id) => {
          const handle = controller.handles.get(id);
          return (
            handle !== undefined &&
            controller.prepared.get(id) === handle &&
            controller.commits[id] > 0
          );
        }),
      "both canvases to prepare and commit nested scenes",
    );
  } catch (error) {
    throw new Error(
      `${asError(error).message}: ${JSON.stringify(
        Object.fromEntries(
          CANVAS_IDS.map((id) => [
            id,
            {
              handle: controller.handles.has(id),
              prepared:
                controller.prepared.get(id) === controller.handles.get(id),
              preparing: controller.preparing.has(id),
              commits: controller.commits[id],
              errors: controller.errors[id],
            },
          ]),
        ),
      )}`,
    );
  }
  await Promise.all(CANVAS_IDS.map((id) => requireHandle(id).flush()));
  if (requireHandle("left").client === requireHandle("right").client)
    throw new Error("Independent canvases must own distinct runtime clients");
  return await Promise.all(CANVAS_IDS.map(observeCanvas));
}

export async function updateSharedScene(
  x: number,
  scale: number,
  color: readonly [number, number, number],
): Promise<readonly CanvasObservation[]> {
  const controller = requireActive();
  const prior = { ...controller.commits };
  controller.model.shared = Object.freeze({
    x,
    scale,
    color: Object.freeze([color[0], color[1], color[2]] as const),
  });
  renderApplication(controller);
  await waitUntil(
    () => CANVAS_IDS.every((id) => controller.commits[id] > prior[id]),
    "shared context update to commit in both scene roots",
  );
  await Promise.all(CANVAS_IDS.map((id) => requireHandle(id).flush()));
  return await Promise.all(CANVAS_IDS.map(observeCanvas));
}

export async function replaceCommitCallbacks(): Promise<{
  readonly before: Readonly<Record<CanvasId, number>>;
  readonly after: Readonly<Record<CanvasId, number>>;
}> {
  const controller = requireActive();
  const before = Object.freeze({ ...controller.commits });
  controller.model.callbackRevision += 1;
  renderApplication(controller);
  await animationBarrier();
  await Promise.all(CANVAS_IDS.map((id) => requireHandle(id).flush()));
  return { before, after: Object.freeze({ ...controller.commits }) };
}

export async function rejectSceneUpdate(id: CanvasId): Promise<{
  readonly error: string;
  readonly observation: CanvasObservation;
}> {
  const controller = requireActive();
  const errorsBefore = controller.errors[id].length;
  controller.model.invalid[id] = true;
  renderApplication(controller);
  await waitUntil(
    () => controller.errors[id].length > errorsBefore,
    `${id} scene rejection to reach onError`,
  );
  await requireHandle(id)
    .flush()
    .catch(() => undefined);
  const error = controller.errors[id].at(-1);
  if (error === undefined) throw new Error(`${id} rejection omitted its error`);
  return { error, observation: await inspectCanvas(id) };
}

export async function recoverSceneUpdate(
  id: CanvasId,
): Promise<CanvasObservation> {
  const controller = requireActive();
  const commitsBefore = controller.commits[id];
  controller.model.invalid[id] = false;
  renderApplication(controller);
  await waitUntil(
    () => controller.commits[id] > commitsBefore,
    `${id} scene correction to commit`,
  );
  await requireHandle(id).flush();
  return await observeCanvas(id);
}

export async function rejectAsyncCommit(id: CanvasId): Promise<{
  readonly error: string;
  readonly duringFailure: CanvasObservation;
  readonly recovered: CanvasObservation;
}> {
  const controller = requireActive();
  const errorsBefore = controller.errors[id].length;
  const commitsBefore = controller.commits[id];
  controller.model.asyncCommitFailure[id] = true;
  controller.model.shared = Object.freeze({
    ...controller.model.shared,
    x: controller.model.shared.x + 0.1,
  });
  renderApplication(controller);
  await waitUntil(
    () => controller.errors[id].length > errorsBefore,
    `${id} asynchronous onCommit rejection to reach onError`,
  );
  const error = controller.errors[id].at(-1);
  if (error === undefined)
    throw new Error(`${id} callback rejection omitted its error`);
  const duringFailure = await inspectCanvas(id);

  controller.model.asyncCommitFailure[id] = false;
  controller.model.shared = Object.freeze({
    ...controller.model.shared,
    x: controller.model.shared.x + 0.1,
  });
  renderApplication(controller);
  await waitUntil(
    () => controller.commits[id] > commitsBefore,
    `${id} scene commit after callback rejection`,
  );
  await requireHandle(id).flush();
  return { error, duringFailure, recovered: await inspectCanvas(id) };
}

export async function removeScene(id: CanvasId): Promise<{
  readonly removed: CanvasObservation;
  readonly other: CanvasObservation;
}> {
  const controller = requireActive();
  controller.model.sceneMounted[id] = false;
  renderApplication(controller);
  await animationBarrier();
  await requireHandle(id).flush();
  const other = id === "left" ? "right" : "left";
  await requireHandle(other).flush();
  return {
    removed: await observeCanvas(id),
    other: await observeCanvas(other),
  };
}

export async function resizeCanvas(
  id: CanvasId,
  width: number,
  height: number,
): Promise<{
  readonly session: bigint;
  readonly frameWidth: number;
  readonly frameHeight: number;
  readonly domWidth: number;
  readonly domHeight: number;
}> {
  const controller = requireActive();
  const handle = requireHandle(id);
  controller.model.dimensions[id] = { width, height };
  renderApplication(controller);
  await animationBarrier();
  await handle.flush();
  const frame = await captureAtSize(handle, width, height);
  const canvas = document.querySelector<HTMLCanvasElement>(`#canvas-${id}`);
  if (!canvas) throw new Error(`${id} canvas is missing after resize`);
  return {
    session: handle.client.session,
    frameWidth: frame.width,
    frameHeight: frame.height,
    domWidth: canvas.clientWidth,
    domHeight: canvas.clientHeight,
  };
}

/** Real presentation fault controls; inspect remains a runtime barrier during loss. */
export async function setCanvasContextLost(
  id: CanvasId,
  lost: boolean,
): Promise<void> {
  const client = requireHandle(id).client;
  if (lost) client.presentation!.loseContext();
  else client.presentation!.restoreContext();
  await client.inspect();
}

export async function setCanvasCssSize(
  id: CanvasId,
  width: number,
  height: number,
): Promise<void> {
  const controller = requireActive();
  controller.model.cssDimensions[id] = { width, height };
  renderApplication(controller);
  await animationBarrier();
}

export async function observeCanvasLayout(
  id: CanvasId,
): Promise<CanvasLayoutObservation> {
  const canvas = document.querySelector<HTMLCanvasElement>(`#canvas-${id}`);
  if (!canvas) throw new Error(`${id} canvas is missing from the DOM`);
  const bounds = canvas.getBoundingClientRect();
  return {
    observation: await observeCanvas(id),
    cssWidth: bounds.width,
    cssHeight: bounds.height,
    attributeWidth: canvas.width,
    attributeHeight: canvas.height,
    devicePixelRatio: window.devicePixelRatio,
    transfers: transferObservation(),
  };
}

export async function replaceCanvasRuntime(id: CanvasId): Promise<{
  readonly oldSession: bigint;
  readonly replacement: CanvasObservation;
  readonly other: CanvasObservation;
  readonly transfers: TransferObservation;
}> {
  const controller = requireActive();
  const oldHandle = requireHandle(id);
  const oldSession = oldHandle.client.session;
  const otherId = id === "left" ? "right" : "left";
  const otherHandle = requireHandle(otherId);
  const otherSession = otherHandle.client.session;
  const commitsBefore = controller.commits[id];
  const configuration = controller.configurations[id];
  const wasmUrl = new URL(configuration.wasmUrl);
  wasmUrl.searchParams.set("canvas-replacement", oldSession.toString());
  controller.configurations[id] = Object.freeze({
    ...configuration,
    wasmUrl: wasmUrl.href,
  });
  renderApplication(controller);

  await oldHandle.closed;
  await waitUntil(() => {
    const replacement = controller.handles.get(id);
    return (
      replacement !== undefined &&
      replacement !== oldHandle &&
      controller.prepared.get(id) === replacement &&
      controller.commits[id] > commitsBefore
    );
  }, `${id} replacement canvas to prepare and commit its World`);
  const replacement = requireHandle(id);
  await replacement.flush();
  if (replacement.client === oldHandle.client) {
    throw new Error(`${id} runtime replacement reused its old client`);
  }
  if (requireHandle(otherId) !== otherHandle) {
    throw new Error(`${otherId} handle changed during ${id} replacement`);
  }
  if (otherHandle.client.session !== otherSession) {
    throw new Error(`${otherId} session changed during ${id} replacement`);
  }
  return {
    oldSession,
    replacement: await inspectCanvas(id),
    other: await inspectCanvas(otherId),
    transfers: transferObservation(),
  };
}

export async function removeCanvas(id: CanvasId): Promise<{
  readonly closedSession: bigint;
  readonly remaining: CanvasObservation;
}> {
  const controller = requireActive();
  const handle = requireHandle(id);
  const closedSession = handle.client.session;
  controller.model.mounted[id] = false;
  renderApplication(controller);
  await handle.closed;
  controller.handles.delete(id);
  controller.prepared.delete(id);
  const other = id === "left" ? "right" : "left";
  await requireHandle(other).flush();
  return { closedSession, remaining: await observeCanvas(other) };
}

export async function observeCanvas(id: CanvasId): Promise<CanvasObservation> {
  const handle = requireHandle(id);
  await handle.flush();
  return await inspectCanvas(id);
}

async function inspectCanvas(id: CanvasId): Promise<CanvasObservation> {
  const controller = requireActive();
  const handle = requireHandle(id);
  const inspection = await handle.client.inspect();
  const producer = entity(inspection, producerId(id));
  const owned = entity(inspection, ownedId(id));
  const transform = requireComponent(handle.client, "Transform");
  const material = requireComponent(handle.client, "UnlitMaterial");
  const canvas = document.querySelector<HTMLCanvasElement>(`#canvas-${id}`);
  if (!canvas) throw new Error(`${id} canvas is missing from the DOM`);
  return {
    id,
    session: handle.client.session,
    entityIds: inspection.entities
      .map(({ metadata }) => metadata.symbolicId)
      .filter((value): value is string => value !== null)
      .sort(),
    runtimeEntityIds: inspection.entities
      .flatMap(({ id: runtimeId, metadata }) =>
        metadata.symbolicId === null
          ? []
          : [{ symbolicId: metadata.symbolicId, id: runtimeId }],
      )
      .sort((left, right) => left.symbolicId.localeCompare(right.symbolicId)),
    producer: {
      baseX: numericField(producer, "base", transform.id, "x"),
      effectiveX: numericField(producer, "effective", transform.id, "x"),
      baseColor: colorFields(producer, "base", material.id),
      effectiveColor: colorFields(producer, "effective", material.id),
    },
    ownedExists: owned !== undefined,
    commitCount: controller.commits[id],
    errors: [...controller.errors[id]],
    width: canvas.width,
    height: canvas.height,
  };
}

export async function captureCanvas(
  id: CanvasId,
  label: string,
  expectedWidth = WIDTH,
  expectedHeight = HEIGHT,
): Promise<CanvasCaptureReport> {
  const controller = requireActive();
  const handle = requireHandle(id);
  const observation = await observeCanvas(id);
  const frame = await captureAtSize(handle, expectedWidth, expectedHeight);
  if (frame.session !== handle.client.session) {
    throw new Error(`${id} capture belongs to another session`);
  }
  controller.captures.set(label, {
    ...frame,
    pixels: frame.pixels.slice(0),
  });
  return {
    label,
    id,
    session: frame.session,
    tick: frame.tick,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    contextGeneration: frame.contextGeneration,
    summary: summarizeImage(frame),
    observation,
  };
}

export function compareCanvasCaptures(
  first: string,
  second: string,
): ImageDifference {
  return compareImages(requireCapture(first), requireCapture(second));
}

export async function canvasCaptureDataUrl(label: string): Promise<string> {
  const frame = requireCapture(label);
  const image = new ImageData(
    new Uint8ClampedArray(frame.pixels.slice(0)),
    frame.width,
    frame.height,
  );
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d");
  if (!context)
    throw new Error("2D canvas is unavailable for capture evidence");
  context.putImageData(image, 0, 0);
  return canvas.toDataURL("image/png");
}

export async function closeCanvasApplication(): Promise<void> {
  const controller = active;
  active = undefined;
  if (!controller) return;
  const handles = [...controller.handles.values()];
  controller.root.unmount();
  await Promise.all(handles.map(({ closed }) => closed));
  controller.captures.clear();
  document.querySelector("#canvas-application")?.remove();
  releaseTransferObserver();
}

export async function probeAssetCacheEviction(): Promise<{
  before: bigint;
  after: bigint;
}> {
  const handle = requireHandle("left");
  const client = handle.client;
  const mesh = requireComponent(client, "MeshInstance");
  const source = "ipp://mesh/cube?width=0.437&height=0.283&length=0.619";
  const live = new Set<bigint>();
  const create = async (alias: number, symbolicId: string) => {
    const outcome = await client.batch([
      {
        kind: "create",
        alias,
        metadata: { symbolicId, classes: ["cache-budget-probe"] },
      },
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias },
        component: mesh.id,
        fields: fields(mesh, { source, variant: 0 }),
      },
    ]);
    if (!outcome.ok)
      throw new Error(`cache probe creation failed: ${outcome.error.reason}`);
    const created = outcome.aliases.find((entry) => entry.alias === alias);
    if (!created) throw new Error("cache probe creation returned no entity");
    live.add(created.id);
    return created.id;
  };
  const remove = async (id: bigint) => {
    const outcome = await client.batch([
      { kind: "delete", entity: { kind: "handle", id } },
    ]);
    if (!outcome.ok)
      throw new Error(`cache probe cleanup failed: ${outcome.error.reason}`);
    live.delete(id);
  };
  try {
    const firstEntity = await create(7001, "cache-budget-probe-first");
    const first = await waitForResource(client, 1, source);
    const before = first.resources.find(
      (resource) => resource.kind === 1 && resource.source === source,
    )?.id;
    if (before === undefined)
      throw new Error("cache probe first resource omitted its identity");
    await remove(firstEntity);
    const deleted = await client.inspect();
    await client.waitForFrame(deleted.tick);

    const secondEntity = await create(7002, "cache-budget-probe-second");
    const second = await waitForResource(client, 1, source);
    const after = second.resources.find(
      (resource) => resource.kind === 1 && resource.source === source,
    )?.id;
    if (after === undefined)
      throw new Error("cache probe second resource omitted its identity");
    await remove(secondEntity);
    return { before, after };
  } finally {
    await Promise.all(
      [...live].map(async (id) => {
        const outcome = await client.batch([
          { kind: "delete", entity: { kind: "handle", id } },
        ]);
        if (!outcome.ok)
          throw new Error(
            `cache probe cleanup failed: ${outcome.error.reason}`,
          );
      }),
    );
  }
}

export async function mountPendingCanvas(
  configuration: CanvasRuntimeInput,
): Promise<void> {
  await closeCanvasApplication();
  await closePendingCanvas();
  installTransferObserver();
  pendingReadyCount = 0;
  pendingErrors = [];
  const host = document.createElement("div");
  host.id = "pending-canvas-application";
  document.body.replaceChildren(host);
  pendingRoot = createRoot(host);
  pendingRoot.render(
    <StrictMode>
      <IppCanvas
        runtime={{ ...configuration }}
        width={WIDTH}
        height={HEIGHT}
        canvasProps={{ id: "pending-canvas" }}
        onReady={() => {
          pendingReadyCount += 1;
        }}
        onError={(error) => pendingErrors.push(error.message)}
      >
        <div id="pending-dom-child">pending</div>
      </IppCanvas>
    </StrictMode>,
  );
  await animationBarrier();
}

export async function closePendingCanvas(): Promise<void> {
  const root = pendingRoot;
  pendingRoot = undefined;
  root?.unmount();
  await animationBarrier();
  releaseTransferObserver();
}

export function pendingCanvasObservation(): {
  readonly readyCount: number;
  readonly errors: readonly string[];
  readonly canvasPresent: boolean;
  readonly transfers: TransferObservation;
} {
  return {
    readyCount: pendingReadyCount,
    errors: [...pendingErrors],
    canvasPresent: document.querySelector("#pending-canvas") !== null,
    transfers: transferObservation(),
  };
}

function CanvasApplication({ controller }: { controller: FixtureController }) {
  const contents = (
    <ApplicationContext.Provider value={controller.model.shared}>
      <main id="ordinary-dom-wrapper">
        {CANVAS_IDS.map((id) =>
          controller.model.mounted[id] ? (
            <CanvasSlot controller={controller} id={id} key={id} />
          ) : null,
        )}
      </main>
    </ApplicationContext.Provider>
  );
  return controller.strict ? <StrictMode>{contents}</StrictMode> : contents;
}

function CanvasSlot({
  controller,
  id,
}: {
  readonly controller: FixtureController;
  readonly id: CanvasId;
}) {
  const handle = controller.handles.get(id);
  const ready = handle !== undefined && controller.prepared.get(id) === handle;
  const dimensions = controller.model.dimensions[id];
  const cssDimensions = controller.model.cssDimensions[id];
  const onReady = useCallback(
    (handle: IppCanvasHandle) => registerHandle(controller, id, handle),
    [controller, id],
  );
  const onCanvasError = useCallback(
    (error: Error) => controller.errors[id].push(error.message),
    [controller, id],
  );
  return (
    <IppCanvas
      data-canvas-owner={id}
      runtime={{ ...controller.configurations[id] }}
      width={dimensions.width}
      height={dimensions.height}
      canvasProps={{
        id: `canvas-${id}`,
        style: {
          display: "block",
          ...(cssDimensions
            ? {
                width: `${cssDimensions.width}px`,
                height: `${cssDimensions.height}px`,
              }
            : {}),
        },
      }}
      onReady={onReady}
      onError={onCanvasError}
    >
      <div className="ordinary-component-wrapper" data-wrapper={id}>
        <DemoScene controller={controller} id={id} ready={ready} />
      </div>
    </IppCanvas>
  );
}

function DemoScene({
  controller,
  id,
  ready,
}: {
  readonly controller: FixtureController;
  readonly id: CanvasId;
  readonly ready: boolean;
}) {
  const handle = useIppCanvas();
  const shared = useContext(ApplicationContext);
  if (!shared) throw new Error("DemoScene requires application context");
  const sceneValue = useMemo(
    () => ({
      ...shared,
      x: controller.model.invalid[id] ? Number.NaN : shared.x,
    }),
    [controller.model.invalid[id], shared],
  );
  const sceneChildren = useMemo(
    () => (
      <SceneContext.Provider value={sceneValue}>
        <DemoGeometry id={id} />
      </SceneContext.Provider>
    ),
    [id, sceneValue],
  );
  const callbackRevision = controller.model.callbackRevision;
  const onCommit = useCallback(async () => {
    void callbackRevision;
    if (controller.model.asyncCommitFailure[id]) {
      throw new Error(`async onCommit failed for ${id}`);
    }
    controller.commits[id] += 1;
  }, [callbackRevision, controller, id]);
  const onError = useCallback(
    (error: Error) => controller.errors[id].push(error.message),
    [controller, id],
  );
  return (
    <section data-demo-scene={id}>
      <span data-session={id}>
        {handle ? handle.client.session.toString() : "starting"}
      </span>
      {ready &&
      handle !== undefined &&
      handle === controller.prepared.get(id) &&
      controller.model.sceneMounted[id] ? (
        <World onCommit={onCommit} onError={onError}>
          {sceneChildren}
        </World>
      ) : null}
    </section>
  );
}

function DemoGeometry({ id }: { readonly id: CanvasId }): ReactNode {
  const shared = useContext(SceneContext);
  if (!shared) throw new Error("DemoGeometry requires explicit scene context");
  return (
    <>
      <Entity bindTo={producerId(id)}>
        <Transform
          x={shared.x}
          sx={shared.scale}
          sy={shared.scale}
          sz={shared.scale}
        />
        <UnlitMaterial
          r={shared.color[0]}
          g={shared.color[1]}
          b={shared.color[2]}
        />
      </Entity>
      <Entity id={ownedId(id)}>
        <Transform bound={false} x={100} />
      </Entity>
    </>
  );
}

function registerHandle(
  controller: FixtureController,
  id: CanvasId,
  handle: IppCanvasHandle,
): void {
  if (active !== controller) return;
  if (controller.handles.get(id) === handle) return;
  controller.handles.set(id, handle);
  controller.prepared.delete(id);
  const preparation = prepareProducer(controller, id, handle)
    .catch((error: unknown) => {
      if (controller.handles.get(id) !== handle) return;
      controller.errors[id].push(asError(error).message);
    })
    .finally(() => {
      if (controller.preparing.get(id) === preparation) {
        controller.preparing.delete(id);
      }
    });
  controller.preparing.set(id, preparation);
}

async function prepareProducer(
  controller: FixtureController,
  id: CanvasId,
  handle: IppCanvasHandle,
): Promise<void> {
  const client = handle.client;
  if (
    !client.capabilities.spatial ||
    !client.capabilities.stateOverlays ||
    !client.capabilities.builtinAssets
  ) {
    throw new Error(
      "canvas fixture requires scene, overlays, and built-in assets",
    );
  }
  await activateFixtureCamera(client);
  const transform = requireComponent(client, "Transform");
  const material = requireComponent(client, "UnlitMaterial");
  const mesh = requireComponent(client, "MeshInstance");
  const operations: Command[] = [
    {
      kind: "create",
      alias: 1,
      metadata: { symbolicId: producerId(id), classes: ["canvas-producer"] },
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: transform.id,
      fields: fields(transform, {
        x: 0,
        y: 0,
        z: 0,
        qx: 0,
        qy: 0,
        qz: 0,
        qw: 1,
        sx: 1,
        sy: 1,
        sz: 1,
      }),
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: material.id,
      fields: fields(material, { r: 0.82, g: 0.18, b: 0.12 }),
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: mesh.id,
      fields: fields(mesh, {
        source: CUBE_RECIPE,
        variant: 0,
      }),
    },
  ];
  const outcome = await client.batch(operations);
  if (!outcome.ok) {
    throw new Error(`producer setup rejected: ${outcome.error.reason}`);
  }
  const inspection = await waitForResource(client, 1, CUBE_RECIPE);
  if (!entity(inspection, producerId(id))) {
    throw new Error(
      `producer setup acknowledgement omitted ${producerId(id)}: ${inspection.entities
        .map(({ metadata }) => metadata.symbolicId)
        .join(",")}`,
    );
  }
  if (active !== controller || controller.handles.get(id) !== handle) return;
  controller.prepared.set(id, handle);
  renderApplication(controller);
}

function renderApplication(controller: FixtureController): void {
  controller.root.render(<CanvasApplication controller={controller} />);
}

function requireActive(): FixtureController {
  if (!active) throw new Error("canvas fixture is not mounted");
  return active;
}

function requireHandle(id: CanvasId): IppCanvasHandle {
  const handle = requireActive().handles.get(id);
  if (!handle) throw new Error(`${id} canvas is not ready`);
  return handle;
}

function colorFields(
  entitySnapshot: EntitySnapshot | undefined,
  layer: "base" | "effective",
  component: number,
): readonly number[] | null {
  const snapshot = entitySnapshot?.[layer].find(
    (candidate) => candidate.component === component,
  );
  if (!snapshot) return null;
  return ["r", "g", "b"].map((field) => Number(snapshot.fields[field]));
}

function producerId(id: CanvasId): string {
  return `canvas-producer-${id}`;
}

function ownedId(id: CanvasId): string {
  return `canvas-owned-${id}`;
}

function requireCapture(label: string): FrameCapture {
  const frame = requireActive().captures.get(label);
  if (!frame) throw new Error(`missing canvas capture '${label}'`);
  return frame;
}

async function captureAtSize(
  handle: IppCanvasHandle,
  width: number,
  height: number,
): Promise<FrameCapture> {
  const deadline = performance.now() + OPERATION_TIMEOUT_MS;
  let frame = await handle.capture();
  while (frame.width !== width || frame.height !== height) {
    if (performance.now() >= deadline) {
      throw new Error(
        `Timed out waiting for ${width}x${height} canvas capture; latest was ${frame.width}x${frame.height}`,
      );
    }
    await animationBarrier();
    frame = await handle.capture();
  }
  return frame;
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

export * from "./saved-canvas-fixture.js";
