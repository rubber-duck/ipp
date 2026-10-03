import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import {
  outputProducer,
  sameOutputReference,
  type CameraWorldClient,
  type AnimationWorldClient,
  type ClientAssetSource,
  type ComponentFieldValue,
  type EntitySnapshot,
  type EntityTreeNode,
  type GuiEffectSubscription,
  type GuiTarget,
  type PresentationView,
  type OutputReference,
  type WorldReference,
  type GuiWorldClient,
  type Inspection,
} from "@ipp/client";
import {
  hostProfiling,
  nativePresentationTransport,
} from "../../packages/ipp-client/src/testing.js";
import { planCommandPages } from "../../packages/ipp-client/src/command-pages.js";
import {
  createRoot,
  type ReactWorldRoot,
  type CanvasWorldHandle,
} from "@ipp/react";
import type { GuiControlHandle } from "@ipp/react/gui";
import {
  GuiStressScene,
  GUI_STRESS_COLORS,
  type GuiStressAssets,
} from "../../examples/gui-stress/scene.js";
import { GuiDiagnosticPanel } from "../../examples/gui-stress/diagnostic-panel.js";
import { GUI_STRESS_WORKLOAD } from "../../examples/gui-stress/workload.js";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  CAMERA,
  SURFACE,
  GUI,
  selectSystems,
} from "../integration/system-selections.js";
import { exerciseHostProfileOwnership } from "../integration/profile-scenario.js";
import { guiAction } from "../integration/gui-actions.js";

type StressClient = GuiWorldClient & CameraWorldClient & AnimationWorldClient;
type StressContract = typeof import("@ipp/gui-stress-contract");
type Configuration = {
  readonly measure?: boolean;
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl?: string;
  readonly wasmUrl?: string;
  readonly nativeHost?: {
    readonly url: string;
    readonly presentationUrl: string;
  };
};
type Sweep = {
  readonly name: string;
  readonly panels: number;
  readonly treeRows: number;
};
type Action = (typeof GUI_STRESS_WORKLOAD.actions)[number];

let host: import("@ipp/gui-stress-contract").IppHostClient | undefined;
let client: StressClient | undefined;
let root: ReactWorldRoot | undefined;
let world: WorldReference | undefined;
let view: PresentationView | undefined;
let sequence = 0n;
let measure = false;
let activeProfileCapture: string | undefined;
let themes: Uint8Array<ArrayBuffer>[] = [];
const children = new Map<string, CanvasWorldHandle>();
const preparations: Promise<void>[] = [];
const subscriptions: GuiEffectSubscription[] = [];
let assets: GuiStressAssets | undefined;
let sweep: Sweep | undefined;
let revision = 0;
let layoutEpoch = 0;
let themeIndex = 0;
let churnEpoch = 0;
let callbackRevision = 0;
let pressCallbacks = 0;
let callbackRevisions: number[] = [];
/** GUI effects the panel subscriptions observed: presses publish one; value
 * changes are fields, not effects. */
let observedEffects = 0;
let rangeChanges = 0;
let failures: string[] = [];
const firstButton: { current: GuiControlHandle | null } = { current: null };
const firstList: { current: GuiControlHandle | null } = { current: null };
const ranges = new Map<number, readonly [number, number]>();
/** Logical batches, their pages on the wire and GUI commands they carry. */
const traffic = { batches: 0, pages: 0, guiEdits: 0 };
let pageContract:
  | Pick<StressContract, "encodeRequest" | "COMMAND_PAGE_LIMITS">
  | undefined;
const restoreTraffic: (() => void)[] = [];
let cameraEntity: bigint | undefined;
let animationController: bigint | undefined;
let animationAsset: ClientAssetSource | undefined;

function active(): {
  client: StressClient;
  root: ReactWorldRoot;
  assets: GuiStressAssets;
} {
  if (!client || !root || !assets)
    throw new Error("GUI stress fixture is not open");
  return { client, root, assets };
}

function instrument(next: StressClient, gui = false): void {
  const batch = next.batch.bind(next);
  next.batch = async (operations) => {
    traffic.batches += 1;
    traffic.pages += planCommandPages(
      operations,
      pageContract!.encodeRequest,
      pageContract!.COMMAND_PAGE_LIMITS,
    ).length;
    // React writes one SetField per changed field, so an edit counts one
    // operation per field it changes, not one per component.
    if (gui) traffic.guiEdits += operations.length;
    return batch(operations);
  };
  restoreTraffic.push(() => {
    next.batch = batch;
  });
}

function childClient(reference: WorldReference): StressClient {
  const found = [...host!.sessions.values()].find(
    (session) =>
      session.worldReference?.id === reference.id &&
      session.worldReference.incarnation === reference.incarnation,
  );
  if (!found) throw new Error("Attached stress World has no authoring session");
  return found;
}

function onWorld(panel: number, raw: boolean, handle: CanvasWorldHandle): void {
  const key = `${raw ? "raw" : "panel"}-${panel}`;
  if (children.get(key)?.world.id === handle.world.id) return;
  children.set(key, handle);
  const session = childClient(handle.world);
  instrument(session, !raw);
  if (!raw)
    preparations.push(
      session
        .subscribeGuiEffects(() => {
          observedEffects += 1;
        })
        .then((subscription) => {
          subscriptions.push(subscription);
        }),
    );
}

/** A report's output: its World, kind and, for a camera, the producer. */
function reportedOutput(output: OutputReference) {
  const producer = outputProducer(output);
  return {
    kind: output.kind,
    world: {
      id: String(output.world.id),
      incarnation: String(output.world.incarnation),
    },
    ...(producer
      ? {
          entity: String(producer.entity),
          incarnation: String(producer.incarnation),
        }
      : {}),
  };
}

/** The producer entity a report names, or null for a World canvas. */
function outputEntity(output: OutputReference): string | null {
  const producer = outputProducer(output);
  return producer ? String(producer.entity) : null;
}

function outputs(): OutputReference[] {
  if (!view) throw new Error("Stress presentation is unavailable");
  return [
    view.binding.output,
    ...[...children.values()].map((child) => {
      if (!child.output) throw new Error("Stress child output is unavailable");
      return child.output;
    }),
  ];
}

async function frame(afterOutputs?: readonly OutputReference[]) {
  if (!host || !view) throw new Error("Stress presentation is unavailable");
  const result = await host.presentation.frame(view, {
    afterSequence: sequence,
    ...(afterOutputs ? { afterOutputs } : {}),
  });
  sequence = result.sequence;
  if (result.failedDrawCalls)
    throw new Error("Stress presentation failed draws");
  return result;
}

/** The control components the stress panel declares, by component name. */
const CONTROL_KINDS = {
  GuiButton: "button",
  GuiCheckbox: "checkbox",
  GuiSlider: "slider",
  GuiTextInput: "text",
  GuiScrollView: "scrollView",
  GuiVirtualList: "virtualList",
} as const;

/** One entity of the first panel's Canvas tree and its control, if any. */
interface PanelRow {
  readonly entity: bigint;
  readonly parent: bigint | null;
  readonly control: {
    readonly kind: (typeof CONTROL_KINDS)[keyof typeof CONTROL_KINDS];
    readonly component: number;
    /** The control component's fields by name. */
    readonly fields: Readonly<Record<string, ComponentFieldValue>>;
  } | null;
}

function panelControl(
  session: StressClient,
  entity: EntitySnapshot | undefined,
): PanelRow["control"] {
  for (const [name, kind] of Object.entries(CONTROL_KINDS)) {
    const component = session.components[name]?.id;
    const fields = entity?.components.find(
      (candidate) => candidate.component === component,
    )?.fields;
    if (component !== undefined && fields) return { kind, component, fields };
  }
  return null;
}

/** The first panel's Canvas tree, read through entity tree pages and one
 * inspection of its World. */
async function firstPanel() {
  const handle = children.get("panel-0");
  if (!handle) throw new Error("First panel World is unavailable");
  const session = childClient(handle.world);
  // The panel's layout root is the top-level entity the scene names "canvas".
  const root = (await session.inspect()).entities.find(
    (entity) =>
      entity.link.parent === null && entity.metadata.symbolicId === "canvas",
  );
  if (!root) throw new Error("First panel layout root is unavailable");
  const nodes: EntityTreeNode[] = [];
  let after: bigint | undefined;
  for (;;) {
    const page = await session.inspectTreePage({
      root: root.id,
      limit: 256,
      maxDepth: 64,
      ...(after === undefined ? {} : { after }),
    });
    nodes.push(...page.nodes);
    if (page.next === 0n) break;
    after = page.next;
  }
  const inspection = await session.inspect();
  const entities = new Map(
    inspection.entities.map((entity) => [entity.id, entity]),
  );
  const rows: PanelRow[] = nodes.map(({ id, parent }) => ({
    entity: id,
    parent,
    control: panelControl(session, entities.get(id)),
  }));
  return { session, handle, rows, inspection };
}

/** The exact target of `component` on `entity`, from a lifecycle baseline. */
async function controlTarget(
  session: StressClient,
  entity: bigint,
  component: number,
): Promise<GuiTarget> {
  const world = session.worldReference;
  if (!world) throw new Error("Stress panel session has no exact World");
  const watch = await session.watchLifecycle(
    [{ target: { kind: "component", entity, component }, kinds: 8 }],
    () => {},
  );
  try {
    const lifetime = watch.baselines[0]?.lifetime;
    if (lifetime?.kind !== "component" || lifetime.incarnation === null)
      throw new Error("The control component is absent");
    return { world, entity, component, incarnation: lifetime.incarnation };
  } finally {
    await watch.remove();
  }
}

function declaration() {
  const current = active();
  if (!sweep) throw new Error("No GUI stress sweep mounted");
  return (
    <GuiStressScene
      assets={current.assets}
      panels={sweep.panels}
      treeRows={sweep.treeRows}
      revision={revision}
      layoutEpoch={layoutEpoch}
      themeIndex={themeIndex}
      churnEpoch={churnEpoch}
      callbackRevision={callbackRevision}
      themes={themes}
      onWorld={onWorld}
      firstButton={firstButton}
      firstList={firstList}
      onPress={(nextRevision) => {
        pressCallbacks += 1;
        callbackRevisions.push(nextRevision);
      }}
      onRange={(panel, first, last) => {
        rangeChanges += 1;
        ranges.set(panel, [first, last]);
      }}
    />
  );
}

async function render(): Promise<void> {
  await active().root.render(declaration());
}

async function panelEntity(): Promise<bigint> {
  const entity = (await active().client.inspect()).entities.find(
    (candidate) => candidate.metadata.symbolicId === "stress-panel-0",
  )?.id;
  if (entity === undefined)
    throw new Error("The first stress panel is missing");
  return entity;
}

async function until(predicate: () => boolean, message: string): Promise<void> {
  const deadline = performance.now() + 15_000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await frame();
  }
}

async function waitAssets(sources: readonly string[]): Promise<void> {
  const current = active().client;
  const deadline = performance.now() + 15_000;
  for (;;) {
    const resources = (await current.inspect()).resources.filter((item) =>
      sources.includes(item.source),
    );
    if (resources.some((item) => item.status === "failed"))
      throw new Error("A GUI stress asset failed to load");
    if (
      resources.length === sources.length &&
      resources.every((item) => item.status === "loaded")
    )
      return;
    if (performance.now() >= deadline)
      throw new Error("GUI stress assets did not become ready");
    await frame();
  }
}

export async function initialize(config: Configuration) {
  await close();
  measure = config.measure === true;
  const contract: StressContract = await import(config.generatedModuleUrl);
  pageContract = contract;
  if (config.nativeHost) {
    host = await contract.IppHostClient.connectTransport(
      nativePresentationTransport(
        config.nativeHost.url,
        config.nativeHost.presentationUrl,
      ),
      { timeoutMs: 30_000 },
    );
  } else {
    if (!config.workerScriptUrl || !config.wasmUrl)
      throw new Error("Worker script and WASM URL are required");
    const canvas = document.createElement("canvas");
    canvas.width = GUI_STRESS_WORKLOAD.viewport.width;
    canvas.height = GUI_STRESS_WORKLOAD.viewport.height;
    document.body.replaceChildren(canvas);
    host = await contract.IppHostClient.connectWorker(
      config.workerScriptUrl,
      config.wasmUrl,
      { canvas: canvas.transferControlToOffscreen(), timeoutMs: 30_000 },
    );
  }
  world = (
    await host!.createWorld({
      selectedSystems: selectSystems(ATTACHMENTS, CAMERA, SURFACE, LIFECYCLE),
      symbolicId: "gui-stress",
    })
  ).reference;
  client = await host!.openWorld(world);
  client.onRuntimeFailure((failure) => {
    failures.push(failure.message);
  });
  cameraEntity = aliasId(
    await client.batch([
      createEntity(1, "gui-stress-camera"),
      insertComponent(
        client,
        "Transform",
        { kind: "alias", alias: 1 },
        { z: 6 },
      ),
      insertComponent(
        client,
        "Camera",
        { kind: "alias", alias: 1 },
        {
          projection: 1,
          ortho_height: GUI_STRESS_WORKLOAD.camera.height,
        },
      ),
    ]),
    1,
  );
  const output = await host!.bindOutput(world, cameraEntity, "camera");
  const binding = await host!.setRootOutput(output, {
    width: GUI_STRESS_WORKLOAD.viewport.width,
    height: GUI_STRESS_WORKLOAD.viewport.height,
    devicePixelRatio: GUI_STRESS_WORKLOAD.viewport.dpr,
  });
  view = await host!.presentation.select(
    await host!.presentation.surface(),
    binding,
  );
  const load = async (kind: number, path: string) => {
    const response = await fetch(path);
    if (!response.ok) throw new Error(`Missing GUI stress asset ${path}`);
    return client!.createAsset(kind, await response.arrayBuffer());
  };
  const [font, drawing, panel, bitmap] = await Promise.all([
    load(17, "/target/font-assets/shure-tech-mono.ippf"),
    load(18, "/target/surface-assets/icon.ippd"),
    load(18, "/target/surface-assets/panel.ippd"),
    load(2, "/target/surface-assets/badge.ippt"),
  ]);
  assets = { font, drawing, panel, bitmap };
  themes = GUI_STRESS_COLORS.map((color) =>
    contract.GuiTheme.encodeParts({
      nextSlot: 7,
      rows: new Map([
        [
          0,
          {
            part: contract.guiPaintPartIndex({ part: "background" }),
            color: color.base,
            corner_radius: [0.035, 0.035],
            border_width: 0.008,
            border_color: color.accent,
          },
        ],
        [
          1,
          {
            part: contract.guiPaintPartIndex({
              part: "background",
              state: "hovered",
            }),
            color: color.accent,
          },
        ],
        [
          2,
          {
            part: contract.guiPaintPartIndex({ part: "fill" }),
            color: color.accent,
          },
        ],
        [
          3,
          {
            part: contract.guiPaintPartIndex({ part: "label" }),
            color: [0.94, 0.97, 1, 1],
          },
        ],
        [
          4,
          {
            part: contract.guiPaintPartIndex({ part: "focusRing" }),
            color: color.accent,
          },
        ],
        [
          5,
          {
            part: contract.guiPaintPartIndex({ part: "scrollTrackY" }),
            color: [0.12, 0.16, 0.2, 1],
          },
        ],
        [
          6,
          {
            part: contract.guiPaintPartIndex({ part: "scrollThumbY" }),
            color: color.accent,
          },
        ],
      ]),
    }),
  );
  const transform = client.components.Transform!;
  animationAsset = await client.createAsset(
    10,
    client.encodeAnimationClip({
      duration: 1,
      tracks: [
        {
          property: {
            component: transform.id,
            offsets: [transform.fields.z!.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: 0 },
              interpolation: { kind: "linear" },
            },
            { time: 1, value: { kind: "f32", value: 0.15 } },
          ],
        },
      ],
    }).buffer,
  );
  root = createRoot(client, {
    host: host!,
    onError: (error) => failures.push(error.message),
  });
  instrument(client);
  return {
    schemaHash: String(contract.SCHEMA_HASH),
    assetSources: [font.source, drawing.source, panel.source, bitmap.source],
    session: String(client.session),
  };
}

export async function mount(next: Sweep) {
  if (
    next.panels < 1 ||
    next.panels > 16 ||
    next.treeRows < 1 ||
    next.treeRows > 128
  )
    throw new Error("GUI stress sweep exceeds the frozen bounds");
  const current = active();
  for (const restore of restoreTraffic.splice(1)) restore();
  if (animationController !== undefined) {
    await current.client.deleteAnimationController(animationController);
    animationController = undefined;
  }
  // Unmount deletes nothing: remove the previous sweep's declarations, which
  // destroys the child Worlds its boundaries created, before unmounting.
  await current.root.render(null);
  await current.root.unmount();
  subscriptions.length = 0;
  preparations.length = 0;
  children.clear();
  if (cameraEntity === undefined) throw new Error("The camera is missing");
  successfulBatch(
    await current.client.batch(
      componentFields(current.client, "Transform", { x: 0 }).map((field) => ({
        kind: "setField" as const,
        entity: { kind: "handle" as const, id: cameraEntity! },
        component: current.client.components.Transform!.id,
        field,
      })),
    ),
  );
  root = createRoot(current.client, {
    host: host!,
    onError: (error) => failures.push(error.message),
  });
  sweep = next;
  revision = 0;
  layoutEpoch = 0;
  themeIndex = 0;
  churnEpoch = 0;
  callbackRevision = 0;
  firstButton.current = null;
  firstList.current = null;
  ranges.clear();
  const buildStarted = measure ? performance.now() : null;
  await render();
  await until(
    () => children.size === next.panels * 2,
    "Stress child Worlds did not attach",
  );
  await Promise.all(preparations);
  await waitAssets([
    ...Object.values(current.assets).map((asset) => asset.source),
    animationAsset!.source,
  ]);
  await until(
    () =>
      firstButton.current !== null &&
      firstList.current !== null &&
      ranges.has(0),
    "GUI controls or VirtualList did not become ready",
  );
  const raw = (await current.client.inspect()).entities.find(
    (candidate) => candidate.metadata.symbolicId === "stress-raw-0",
  )?.id;
  if (raw === undefined) throw new Error("Animated raw Surface is missing");
  animationController = await current.client.createAnimationController({
    speed: 0,
    drivers: [
      {
        source: animationAsset!.source,
        track: 0,
        target: raw,
        property: {
          component: current.client.components.Transform!.id,
          offsets: [current.client.components.Transform!.fields.z!.offset],
        },
      },
    ],
  });
  await current.client.controlAnimationController(animationController, {
    action: "play",
  });
  await current.client.controlAnimationController(animationController, {
    action: "seek",
    time: 0,
  });
  const completed = await frame(outputs());
  if (completed.failedDrawCalls)
    throw new Error("GUI stress scene did not present a valid frame");
  const buildToFrameMs =
    buildStarted === null ? null : performance.now() - buildStarted;
  const observedPanelEntities = (await firstPanel()).inspection.entities.length;
  const firstEntity = await panelEntity();
  const list = await firstList.current!.read();
  const camera = (await current.client.inspect()).entities.find(
    (candidate) => candidate.id === cameraEntity,
  );
  const cameraX = camera?.components.find(
    (component) =>
      component.component === current.client.components.Transform!.id,
  )?.fields.x;
  return {
    buildToFrameMs,
    observedPanelEntities,
    frame: {
      sequence: String(completed.sequence),
      publication: {
        host: String(completed.publication.host),
        revision: String(completed.publication.revision),
      },
      drawCalls: completed.drawCalls,
    },
    range: ranges.get(0),
    entity: String(firstEntity),
    rootIncarnation: String(children.get("panel-0")!.world.incarnation),
    initialAnchor:
      typeof list.anchor_index === "number" ? list.anchor_index : undefined,
    cameraX,
    rangeChanges,
  };
}

export async function warmup(frames: number) {
  for (let index = 0; index < frames; index += 1) await frame();
}

export async function step(action: Action, cycle = 0) {
  const current = active();
  const before = { ...traffic, pressCallbacks, observedEffects, rangeChanges };
  const started = measure ? performance.now() : null;
  let expectedRange: number | undefined;
  let toggleBefore: boolean | undefined;
  let toggleApplied: boolean | undefined;
  let controlValueChanged: boolean | undefined;
  if (action === "local-text") {
    revision += 1;
    await render();
  } else if (action === "layout") {
    layoutEpoch += 1;
    await render();
  } else if (action === "theme") {
    themeIndex += 1;
    await render();
  } else if (action === "callback-only") {
    callbackRevision += 1;
    await render();
  } else if (action === "churn") {
    churnEpoch += 1;
    await render();
  } else if (action === "control-press") {
    const button = firstButton.current;
    if (!button) throw new Error("The semantic ARM control is missing");
    const result = await button.action({ kind: "press" });
    if (!result.ok) throw new Error("Semantic press was rejected");
    await until(
      () => pressCallbacks > before.pressCallbacks,
      "The control press published no callback",
    );
    if (callbackRevisions.at(-1) !== callbackRevision)
      throw new Error(
        "The replaced callback did not observe its latest revision",
      );
  } else if (action === "control-toggle") {
    const panel = await firstPanel();
    const checkbox = panel.rows.find((row) => row.control?.kind === "checkbox");
    if (!checkbox?.control) throw new Error("The semantic checkbox is missing");
    toggleBefore = checkbox.control.fields.checked === true;
    const result = await guiAction(
      panel.session,
      await controlTarget(
        panel.session,
        checkbox.entity,
        checkbox.control.component,
      ),
      { kind: "toggle" },
    );
    // The toggle's batch applied; the new value is the checked field, read
    // after the next frame.
    toggleApplied = result.ok;
    if (!toggleApplied)
      throw new Error("Semantic checkbox toggle was rejected");
  } else if (action === "control-drag") {
    const panel = await firstPanel();
    const slider = panel.rows.find((row) => row.control?.kind === "slider");
    if (!slider?.control || !host || !view || !sweep)
      throw new Error("Slider drag fixture is unavailable");
    const entity = panel.inspection.entities.find(
      (item) => item.id === slider.entity,
    )!;
    const bounds = entity.components.find(
      (item) => item.component === panel.session.components.CanvasBounds!.id,
    )?.fields;
    if (!bounds) throw new Error("Slider has no evaluated bounds");
    const rootEntities = (await current.client.inspect()).entities;
    const surface = rootEntities.find(
      (item) => item.metadata.symbolicId === "stress-panel-0",
    )!;
    const transform = surface.components.find(
      (item) => item.component === current.client.components.Transform!.id,
    )!.fields;
    const camera = rootEntities.find((item) => item.id === cameraEntity)!;
    const cameraX = Number(
      camera.components.find(
        (item) => item.component === current.client.components.Transform!.id,
      )!.fields.x,
    );
    const scale =
      GUI_STRESS_WORKLOAD.viewport.height / GUI_STRESS_WORKLOAD.camera.height;
    // Host input is normalized viewport space; slider thumb centres stay
    // inside the rail rather than reaching the control's outer edge.
    const thumb = 0.75 * Math.min(Number(bounds.width), Number(bounds.height));
    const point = (fraction: number): readonly [number, number] => [
      0.5 +
        ((Number(transform.x) -
          cameraX -
          GUI_STRESS_WORKLOAD.layout.panelWidth / 2 +
          Number(bounds.x) +
          thumb / 2 +
          (Number(bounds.width) - thumb) * fraction) *
          scale) /
          GUI_STRESS_WORKLOAD.viewport.width,
      0.5 -
        ((Number(transform.y) +
          GUI_STRESS_WORKLOAD.layout.panelHeight / 2 -
          Number(bounds.y) -
          Number(bounds.height) / 2) *
          scale) /
          GUI_STRESS_WORKLOAD.viewport.height,
    ];
    const previous = Number(slider.control.fields.value);
    const target = previous < 0.5 ? 0.85 : 0.15;
    const input = await host.input.open(view);
    try {
      for (const event of [
        { kind: "pointerMove" as const, pointer: 1n, point: point(previous) },
        { kind: "pointerDown" as const, pointer: 1n, point: point(previous) },
        ...Array.from({ length: 4 }, (_, index) => ({
          kind: "pointerMove" as const,
          pointer: 1n,
          point: point(previous + ((target - previous) * (index + 1)) / 4),
        })),
        { kind: "pointerUp" as const, pointer: 1n, point: point(target) },
      ]) {
        const outcome = await input.send(event);
        if (event.kind === "pointerDown" && outcome.disposition !== "routed")
          throw new Error(
            `Slider pointer missed its thumb at ${JSON.stringify(event.point)}`,
          );
        if (outcome.rejected || outcome.error)
          throw new Error(`Slider physical input rejected: ${outcome.error}`);
        await frame(outputs());
      }
    } finally {
      await input.close();
    }
    const changed = (await firstPanel()).rows.find(
      (row) => row.entity === slider.entity,
    )?.control?.fields.value;
    if (typeof changed !== "number" || Math.abs(changed - previous) < 0.05)
      throw new Error("Routed slider drag did not change its value");
    controlValueChanged = true;
  } else if (action === "virtual-scroll") {
    const list = firstList.current;
    if (!list) throw new Error("VirtualList handle is unavailable");
    expectedRange =
      GUI_STRESS_WORKLOAD.virtualScrollStart +
      cycle * GUI_STRESS_WORKLOAD.virtualScrollStride;
    const oldRange = ranges.get(0);
    const oldChanges = rangeChanges;
    const result = await list.action({
      kind: "scrollToIndex",
      index: expectedRange,
      offset: 0,
    });
    if (!result.ok) throw new Error("Virtual scroll was rejected");
    await until(
      () =>
        rangeChanges > oldChanges &&
        ranges.get(0) !== oldRange &&
        Math.abs((ranges.get(0)?.[0] ?? 0) - expectedRange!) < 8,
      "VirtualList did not publish a fresh wanted range",
    );
  } else if (action === "camera-only") {
    if (cameraEntity === undefined) throw new Error("The camera is missing");
    const fields = componentFields(current.client, "Transform", {
      x: cycle % 2 === 0 ? 0.08 : -0.08,
    });
    successfulBatch(
      await current.client.batch(
        fields.map((field) => ({
          kind: "setField" as const,
          entity: { kind: "handle" as const, id: cameraEntity! },
          component: current.client.components.Transform!.id,
          field,
        })),
      ),
    );
  } else if (action === "animation") {
    if (animationController === undefined)
      throw new Error("The animation controller is missing");
    await current.client.controlAnimationController(animationController, {
      action: "seek",
      time: cycle % 2 === 0 ? 0.25 : 0.75,
    });
  } else if (action !== "idle" && action !== "cache-idle") {
    throw new Error(`Unknown GUI stress action: ${action}`);
  }
  const acknowledged = measure ? performance.now() : null;
  const completed = await frame(outputs());
  const presented = measure ? performance.now() : null;
  if (failures.length || completed.failedDrawCalls)
    throw new Error(`GUI stress frame failed: ${failures.join("; ")}`);
  if (toggleBefore !== undefined) {
    const panel = await firstPanel();
    controlValueChanged =
      (panel.rows.find((row) => row.control?.kind === "checkbox")?.control
        ?.fields.checked ===
        true) !==
      toggleBefore;
    if (!controlValueChanged)
      throw new Error("The checkbox value did not change");
  }
  let sampledAnimationTime: number | undefined;
  if (action === "animation") {
    sampledAnimationTime = (await current.client.inspect()).controllers?.find(
      (controller) => controller.id === animationController,
    )?.time;
    if (
      Math.abs((sampledAnimationTime ?? -1) - (cycle % 2 === 0 ? 0.25 : 0.75)) >
      0.01
    )
      throw new Error(
        "The runtime did not sample the requested animation time",
      );
  }
  return {
    action,
    updateMs:
      started === null || acknowledged === null ? null : acknowledged - started,
    updateToFrameMs:
      started === null || presented === null ? null : presented - started,
    frame: {
      sequence: String(completed.sequence),
      publication: {
        host: String(completed.publication.host),
        revision: String(completed.publication.revision),
      },
      drawCalls: completed.drawCalls,
      triangles: completed.triangles,
      sources: completed.sources.map((source) => ({
        output: reportedOutput(source.output),
        minimumTick: String(source.minimumTick),
        tick: String(source.tick),
        publication: {
          host: String(source.publication.host),
          revision: String(source.publication.revision),
        },
      })),
    },
    traffic: {
      batches: traffic.batches - before.batches,
      pages: traffic.pages - before.pages,
      guiEdits: traffic.guiEdits - before.guiEdits,
    },
    effects: observedEffects - before.observedEffects,
    toggleApplied,
    pressCallbacks: pressCallbacks - before.pressCallbacks,
    rangeChanges: rangeChanges - before.rangeChanges,
    firstRange: ranges.get(0),
    expectedRange,
    controlValueChanged,
    callbackRevision: callbackRevisions.at(-1),
    sampledAnimationTime,
    revision,
    layoutEpoch,
    themeIndex,
    churnEpoch,
  };
}

export async function observeState() {
  const current = active();
  const panel = await firstPanel();
  const controls = panel.rows.flatMap((row) =>
    row.control ? [row.control] : [],
  );
  const listRow = panel.rows.find((row) => row.control?.kind === "virtualList");
  if (!listRow?.control) throw new Error("Virtual list state missing");
  const listFields = listRow.control.fields;
  const list = {
    entity: listRow.entity,
    itemCount: Number(listFields.item_count),
    anchorIndex: Number(listFields.anchor_index),
    anchorOffset: Number(listFields.anchor_offset),
  };
  const listTarget = await controlTarget(
    panel.session,
    list.entity,
    listRow.control.component,
  );
  const inspection = panel.inspection;
  const virtualItems = (sample: Inspection) => {
    const items = sample.entities
      .filter((entity) => entity.link.parent === list.entity)
      .map((entity) => {
        const index = entity.components.find(
          (component) =>
            component.component === panel.session.components.GuiVirtualItem!.id,
        )?.fields.index;
        if (
          typeof index !== "number" ||
          !Number.isInteger(index) ||
          index < 0 ||
          index >= list.itemCount ||
          !entity.components.some(
            (component) =>
              component.component === panel.session.components.GuiLayout!.id,
          )
        )
          throw new Error("Virtual item declaration is missing or invalid");
        return { entity: String(entity.id), index };
      })
      .sort((left, right) => left.index - right.index);
    if (
      !items.length ||
      items.some(
        (item, index) =>
          index > 0 && item.index !== items[index - 1]!.index + 1,
      )
    )
      throw new Error(
        "Evaluated virtual items are empty, duplicated or discontinuous",
      );
    return items;
  };
  const items = virtualItems(inspection);
  const completed = await frame(outputs());
  const included = completed.sources.find((source) =>
    sameOutputReference(source.output, panel.handle.output),
  );
  if (
    !included ||
    included.minimumTick <= inspection.tick ||
    included.tick < included.minimumTick
  )
    throw new Error(
      "Virtual item declarations were not included in a later evaluated output",
    );
  const verified = await panel.session.inspect();
  if (
    verified.tick < included.tick ||
    JSON.stringify(virtualItems(verified)) !== JSON.stringify(items)
  )
    throw new Error(
      "Virtual item declarations changed across their evaluation fence",
    );
  const verifiedFields = verified.entities
    .find((entity) => entity.id === list.entity)
    ?.components.find(
      (component) => component.component === listRow.control!.component,
    )?.fields;
  const verifiedTarget = await controlTarget(
    panel.session,
    list.entity,
    listRow.control.component,
  );
  if (
    !verifiedFields ||
    verifiedTarget.incarnation !== listTarget.incarnation ||
    Number(verifiedFields.anchor_index) !== list.anchorIndex ||
    Number(verifiedFields.anchor_offset) !== list.anchorOffset
  )
    throw new Error("Virtual list anchor changed across the observation fence");
  const rows = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === "explicit-rows",
  );
  if (!rows) throw new Error("Explicit stress row container missing");
  const world = await current.client.inspect();
  return {
    panelCount: world.entities.filter((item) =>
      item.metadata.symbolicId?.startsWith("stress-panel-"),
    ).length,
    rawSurfaceCount: world.entities.filter((item) =>
      item.metadata.symbolicId?.startsWith("stress-raw-"),
    ).length,
    semanticRoles: controls.reduce<Record<string, number>>(
      (counts, control) => {
        const role = control.kind === "text" ? "textInput" : control.kind;
        counts[role] = (counts[role] ?? 0) + 1;
        return counts;
      },
      {},
    ),
    virtualList: {
      itemCount: list.itemCount,
      anchorIndex: list.anchorIndex,
      anchorOffset: list.anchorOffset,
      loadedFirst: items[0]!.index,
      loadedLast: items.at(-1)!.index + 1,
      realization: {
        world: {
          id: String(panel.handle.world.id),
          incarnation: String(panel.handle.world.incarnation),
        },
        list: String(list.entity),
        inspectionTick: String(inspection.tick),
        minimumTick: String(included.minimumTick),
        includedTick: String(included.tick),
        verificationTick: String(verified.tick),
        sequence: String(completed.sequence),
        items,
      },
    },
    declaredRowChildren: panel.rows.filter((row) => row.parent === rows.id)
      .length,
    range: ranges.get(0),
    pressCallbacks,
    observedEffects,
    rangeChanges,
    traffic: { ...traffic },
    failures: [...failures],
  };
}

export async function capture() {
  if (!host || !view || !renderDiagnostics(host))
    throw new Error("Stress capture/diagnostics unavailable");
  const captured = await host.presentation.capture(view, {
    afterSequence: sequence,
    afterOutputs: outputs(),
  });
  sequence = captured.sequence;
  const statistics = await renderDiagnostics(host)!.statistics();
  const bytes = new Uint8Array(captured.pixels);
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  return {
    width: captured.view.binding.viewport.width,
    height: captured.view.binding.viewport.height,
    tick: null,
    output: {
      kind: captured.view.binding.output.kind,
      world: String(captured.view.binding.output.world.id),
      entity: outputEntity(captured.view.binding.output),
    },
    publication: {
      host: String(captured.publication.host),
      revision: String(captured.publication.revision),
    },
    sequence: String(captured.sequence),
    worlds: [world!, ...[...children.values()].map((child) => child.world)].map(
      (reference) => ({
        id: String(reference.id),
        incarnation: String(reference.incarnation),
      }),
    ),
    drawCalls: captured.drawCalls,
    triangles: captured.triangles,
    failedDrawCalls: captured.failedDrawCalls,
    pixels: btoa(binary),
    statistics: JSON.parse(
      JSON.stringify(statistics, (_, value) =>
        typeof value === "bigint" ? String(value) : value,
      ),
    ),
  };
}

export async function layoutDiagnosticsProbe() {
  if (!host || !renderDiagnostics(host) || !client)
    throw new Error("Layout diagnostics require the live Host");
  const diagnostics = renderDiagnostics(host)!;
  const sample = async () => {
    await client!.waitForFrame();
    return (await diagnostics.statistics()).guiLayout;
  };
  const reference = (
    await host.createWorld({
      selectedSystems: selectSystems(GUI),
      symbolicId: "unpresented-layout-probe",
    })
  ).reference;
  const session = await host.openWorld(reference);
  const identity = {
    id: String(reference.id),
    incarnation: String(reference.incarnation),
  };
  let destroyed = false;
  try {
    successfulBatch(
      await session.batch([
        createEntity(1, "unpresented-canvas"),
        insertComponent(
          session,
          "GuiLayout",
          { kind: "alias", alias: 1 },
          { width: 20, height: 10 },
        ),
      ]),
    );
    await session.waitForFrame();
    const evaluated = await sample();
    await session.waitForFrame();
    const resumed = await sample();
    await session.close();
    await host.destroyWorld(reference);
    destroyed = true;
    const retired = await sample();
    const unsupported = (
      await host.createWorld({
        symbolicId: "layout-not-selected",
        selectedSystems: [],
      })
    ).reference;
    try {
      const missing = await sample();
      return {
        identity,
        evaluated,
        resumed,
        retired,
        missing,
        unsupported: {
          id: String(unsupported.id),
          incarnation: String(unsupported.incarnation),
        },
      };
    } finally {
      await host.destroyWorld(unsupported);
    }
  } finally {
    if (!destroyed) {
      await session.close();
      await host.destroyWorld(reference);
    }
  }
}

/** React declaration, transport inspection and local edits on counted panels. */
export async function panelDiagnostic(count: number, samples: number) {
  if (
    !GUI_STRESS_WORKLOAD.diagnosticPanelEntities.some(
      (expected) => expected === count,
    )
  )
    throw new Error("Unsupported diagnostic panel size");
  const current = active();
  if (animationController !== undefined) {
    await current.client.deleteAnimationController(animationController);
    animationController = undefined;
  }
  await current.root.render(null);
  // Destroyed child sessions already closed their effect subscriptions.
  subscriptions.length = 0;
  preparations.length = 0;
  for (const restore of restoreTraffic.splice(1)) restore();
  children.clear();
  let handle: CanvasWorldHandle | undefined;
  let revision = 0;
  let position = 0;
  const declaration = () => (
    <GuiDiagnosticPanel
      count={count}
      revision={revision}
      position={position}
      onReady={(next) => {
        handle = next;
        children.set("panel-0", next);
      }}
    />
  );
  const started = measure ? performance.now() : null;
  await current.root.render(declaration());
  await until(() => handle !== undefined, "Diagnostic panel did not attach");
  await frame(outputs());
  const buildMs = started === null ? null : performance.now() - started;
  const session = childClient(handle!.world);
  const inspectStarted = measure ? performance.now() : null;
  const inspection = await session.inspect();
  const inspectMs =
    inspectStarted === null ? null : performance.now() - inspectStarted;
  if (inspection.entities.length !== count)
    throw new Error(
      `Diagnostic panel expected ${count} entities, observed ${inspection.entities.length}`,
    );
  const inspectionJsonUtf8Bytes = new TextEncoder().encode(
    JSON.stringify(inspection, (_, item: unknown) =>
      typeof item === "bigint" ? item.toString() : item,
    ),
  ).byteLength;
  await warmup(GUI_STRESS_WORKLOAD.warmupFrames);
  const before = await capture();
  const edits = [];
  for (let sample = 0; sample < samples; sample += 1) {
    for (const action of ["local-colour", "local-position"] as const) {
      const started = measure ? performance.now() : null;
      if (action === "local-colour") revision += 1;
      else position = sample % 2 === 0 ? 0.02 : 0;
      await current.root.render(declaration());
      const acknowledged = measure ? performance.now() : null;
      await frame(outputs());
      edits.push({
        action,
        updateMs:
          started === null || acknowledged === null
            ? null
            : acknowledged - started,
        updateToFrameMs: started === null ? null : performance.now() - started,
      });
    }
  }
  // End with a colour guaranteed to differ from the before capture.
  revision = 1;
  await current.root.render(declaration());
  await frame(outputs());
  return {
    requestedEntities: count,
    observedEntities: inspection.entities.length,
    buildMs,
    inspectMs,
    inspectionJsonUtf8Bytes,
    inspectionScope:
      "encoded inspection JSON after transport decoding; not wire bytes",
    edits,
    before,
    after: await capture(),
  };
}

export async function hostProfileOwnershipScenario() {
  if (!host) throw new Error("Host is unavailable");
  return exerciseHostProfileOwnership(host, selectSystems(GUI));
}

export async function hostProfileStatus() {
  if (!host) throw new Error("Host is unavailable");
  return hostProfiling(host).status();
}

export async function hostProfileStart() {
  if (!host) throw new Error("Host is unavailable");
  activeProfileCapture = await hostProfiling(host).start({ counters: true });
  return activeProfileCapture;
}

export async function hostProfileStop() {
  if (!host || activeProfileCapture === undefined)
    throw new Error("Capture is unavailable");
  const reader = hostProfiling(host);
  try {
    return await reader.stop();
  } finally {
    await reader.release(activeProfileCapture);
    activeProfileCapture = undefined;
  }
}

export async function close() {
  for (const restore of restoreTraffic.splice(0)) restore();
  try {
    if (host && activeProfileCapture !== undefined) {
      await hostProfiling(host).release(activeProfileCapture);
      activeProfileCapture = undefined;
    }
    for (const subscription of subscriptions.splice(0))
      await subscription.unsubscribe();
    if (animationController !== undefined && client)
      await client.deleteAnimationController(animationController);
    animationController = undefined;
    await root?.unmount();
    if (view) await host?.presentation.clear(view);
    await client?.close();
    if (world) await host?.destroyWorld(world);
  } finally {
    await host?.close();
  }
  host = undefined;
  client = undefined;
  root = undefined;
  assets = undefined;
  sweep = undefined;
  cameraEntity = undefined;
  animationAsset = undefined;
  world = undefined;
  view = undefined;
  sequence = 0n;
  children.clear();
  preparations.length = 0;
  callbackRevisions = [];
  failures = [];
}
