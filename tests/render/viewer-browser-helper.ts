import { hostProfiling } from "../../packages/ipp-client/src/profiling.js";
import type { RenderStatisticsSnapshot } from "@ipp/client/diagnostics";
import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import type {
  AnimationControllerSnapshot,
  AnimationPlaybackControl,
  Command,
  AnimationWorldClient,
  ComponentSnapshot,
  EntitySnapshot,
  EntityTreeNode,
  GuiAction,
  GuiFocusRecord,
  GuiInputRoutingOutcome,
  GuiPhysicalInput,
  GuiPointerRecord,
  GuiTarget,
  GuiWorldClient,
  Inspection,
  PickingWorldClient,
  PresentedCapture,
  PresentedFrame,
  SystemQuery,
  WorldReference,
} from "@ipp/client";
import { sameOutputReference } from "../../packages/ipp-client/src/references.js";
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
import { guiAction } from "../integration/gui-actions.js";

interface ViewerWindow extends Window {
  ippWorldCanvas?: IppCanvasHandle;
}

/**
 * One completed draw of the canvas's selected output: top-left RGBA8 pixels
 * of the exact presented viewport, the evaluated tick of the root output in
 * that draw, and renderer diagnostics read after it. Diagnostics describe
 * the latest completed draw at observation; they are evidence, not a fence.
 */
export interface ViewerFrame {
  readonly width: number;
  readonly height: number;
  readonly devicePixelRatio: number;
  readonly sequence: bigint;
  readonly tick: bigint;
  readonly drawCalls: number;
  readonly triangles: number;
  readonly failedDrawCalls: number;
  readonly statistics: RenderStatisticsSnapshot | undefined;
  readonly pixels: ArrayBuffer;
}

/** Summarize a completed draw with its root output tick and diagnostics. */
function viewerFrame(
  capture: PresentedCapture,
  tick: bigint,
  statistics: RenderStatisticsSnapshot | undefined,
): ViewerFrame {
  const { width, height, devicePixelRatio } = capture.view.binding.viewport;
  if (capture.pixels.byteLength !== width * height * 4)
    throw new Error(
      `Captured ${capture.pixels.byteLength} bytes for a ${width}x${height} viewport`,
    );
  return {
    width,
    height,
    devicePixelRatio,
    sequence: capture.sequence,
    tick,
    drawCalls: capture.drawCalls,
    triangles: capture.triangles,
    failedDrawCalls: capture.failedDrawCalls,
    statistics,
    pixels: capture.pixels,
  };
}

export interface ViewerBrowserCapture extends ViewerObservation {
  readonly label: string;
  readonly frame: Omit<ViewerFrame, "pixels">;
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

const captures = new Map<string, ViewerFrame>();

/** Symbolic IDs the GUI demo names; the tests restate them independently. */
const GUI_PANEL_ENTITY = "gui-demo";
const GUI_EVENT_LOG_ENTITY = "gui-event-log";

/** Entity IDs of named gallery objects per client, verified on each targeted read. */
const galleryEntityIds = new WeakMap<object, Map<string, bigint>>();

function knownGalleryEntities(): Map<string, bigint> {
  const client = requireCanvas().client;
  let ids = galleryEntityIds.get(client);
  if (!ids) galleryEntityIds.set(client, (ids = new Map()));
  return ids;
}

/** Page through the entity collection alone, recording every symbolic ID. */
async function discoverGalleryEntities(): Promise<Map<string, EntitySnapshot>> {
  const client = requireCanvas().client;
  const ids = knownGalleryEntities();
  ids.clear();
  const found = new Map<string, EntitySnapshot>();
  let after = 0n;
  do {
    const page = await client.inspectPage({ collection: "entities", after });
    for (const entity of page.entities) {
      const symbol = entity.metadata.symbolicId;
      if (!symbol) continue;
      ids.set(symbol, entity.id);
      found.set(symbol, entity);
    }
    after = page.next;
  } while (after !== 0n);
  return found;
}

/**
 * Current snapshots of named gallery entities. Known IDs are read through
 * concurrent single-entity inspection pages instead of the whole World; an
 * unknown, replaced or renamed ID is rediscovered from the entity collection.
 * Absent entities read as undefined.
 */
async function galleryEntities(
  symbols: readonly string[],
): Promise<(EntitySnapshot | undefined)[]> {
  const client = requireCanvas().client;
  const ids = knownGalleryEntities();
  const targeted = await Promise.all(
    symbols.map(async (symbol) => {
      const id = ids.get(symbol);
      if (id === undefined) return undefined;
      const page = await client
        .inspectPage({ collection: "entities", target: id, limit: 1 })
        .catch(() => undefined);
      const entity = page?.entities[0];
      return entity?.id === id && entity.metadata.symbolicId === symbol
        ? entity
        : undefined;
    }),
  );
  if (targeted.every((entity) => entity !== undefined)) return targeted;
  const found = await discoverGalleryEntities();
  return symbols.map((symbol) => found.get(symbol));
}

/** The named entity's current ID through a targeted read; undefined when absent. */
export async function galleryEntityId(
  symbol: string,
): Promise<bigint | undefined> {
  return (await galleryEntities([symbol]))[0]?.id;
}

function componentFields(
  client: { readonly components: Readonly<Record<string, { id: number }>> },
  entity: EntitySnapshot,
  name: string,
): ComponentSnapshot["fields"] | undefined {
  const id = client.components[name]?.id;
  return entity.components.find(({ component }) => component === id)?.fields;
}

function worldReference(value: unknown): WorldReference {
  const world = (
    value && typeof value === "object" && "value" in value
      ? (value as { value: unknown }).value
      : value
  ) as WorldReference | null | undefined;
  if (
    !world ||
    typeof world.id !== "bigint" ||
    typeof world.incarnation !== "bigint"
  )
    throw new Error("The GUI demo Surface presents no attached World");
  return world;
}

type PanelClient = GuiWorldClient & AnimationWorldClient;

interface PanelSession {
  readonly world: WorldReference;
  readonly client: PanelClient;
  density?: number;
}

/** This helper's own observation session on the panel World. */
let panelSession: PanelSession | undefined;

/**
 * The World the GUI demo's Surface presents, read from the ordinary Surface
 * attachment of the parent panel entity, with an observation session of this
 * helper. A replaced panel World closes the previous session first.
 */
async function galleryPanel(): Promise<PanelSession> {
  const handle = requireCanvas();
  const [entity] = await galleryEntities([GUI_PANEL_ENTITY]);
  if (!entity) throw new Error("Missing gallery GUI demo");
  const attachment = componentFields(handle.client, entity, "WorldAttachment");
  if (!attachment) throw new Error("The GUI demo has no World attachment");
  const world = worldReference(attachment.child);
  const current = panelSession;
  if (
    current &&
    current.world.id === world.id &&
    current.world.incarnation === world.incarnation &&
    !current.client.closure
  )
    return current;
  panelSession = undefined;
  await current?.client.close().catch(() => {});
  const client = (await handle.host.openWorld(world)) as PanelClient;
  const session: PanelSession = { world, client };
  panelSession = session;
  return session;
}

/** Close this helper's panel observation session. */
export async function closeGalleryPanel(): Promise<void> {
  const session = panelSession;
  panelSession = undefined;
  await session?.client.close();
}

/** Canvas units per Surface metre of the presented panel. */
async function galleryGuiDensity(): Promise<number> {
  const panel = await galleryPanel();
  if (panel.density !== undefined) return panel.density;
  const canvas = (await panel.client.inspectPage({ collection: "canvas" }))
    .canvas;
  const density = canvas?.state.unitsPerMetre ?? Number.NaN;
  if (!(density > 0)) throw new Error("The GUI demo canvas has no density");
  panel.density = density;
  return density;
}

/** The panel World's inspection with plain rows tables. */
export async function inspectGalleryPanel(flush = true): Promise<Inspection> {
  if (flush) await requireCanvas().flush();
  const panel = await galleryPanel();
  return plainRowsTables(await panel.client.inspect());
}

/** The control components the gallery panel authors, by component name. */
const GALLERY_CONTROL_KINDS = {
  GuiButton: "button",
  GuiCheckbox: "checkbox",
  GuiSlider: "slider",
  GuiTextInput: "text",
  GuiScrollView: "scrollView",
  GuiVirtualList: "virtualList",
} as const;

export type GalleryGuiKind =
  (typeof GALLERY_CONTROL_KINDS)[keyof typeof GALLERY_CONTROL_KINDS];

export interface GalleryGuiSelector {
  readonly role: GalleryGuiKind;
  readonly name?: string;
  /** The control's symbolic id, for controls that share a label. */
  readonly symbol?: string;
}

/** A control's value as its value fields hold it. */
export type GalleryGuiValue =
  | { readonly kind: "none" }
  | { readonly kind: "bool"; readonly value: boolean }
  | { readonly kind: "scalar"; readonly value: number }
  | { readonly kind: "text"; readonly value: string }
  | {
      readonly kind: "scroll";
      readonly offset: readonly [number, number];
      readonly anchorIndex: number;
      readonly anchorOffset: number;
    };

/**
 * One control of the panel World, named by its entity's symbolic ID and read
 * through public surfaces: its control, GuiBehavior and CanvasBounds fields,
 * the GUI focus and pointer queries, its core ancestry and its component
 * incarnation from a lifecycle baseline.
 */
export interface GalleryGuiControl {
  readonly symbol: string | undefined;
  readonly target: GuiTarget;
  readonly kind: GalleryGuiKind;
  readonly value: GalleryGuiValue;
  readonly label: string;
  /** Core ancestors, parent first. */
  readonly ancestry: readonly bigint[];
  /** The symbolic id of the overlay the control lies in, if it does. */
  readonly overlay: string | undefined;
  /** GuiBehavior's evaluated eligibility. */
  readonly enabled: boolean;
  readonly visible: boolean;
  readonly available: boolean;
  readonly focused: boolean;
  /** Aggregated over the live pointers on this control. */
  readonly interaction: {
    readonly hovered: boolean;
    readonly pressed: boolean;
    readonly captured: boolean;
  };
  /** Scroll geometry and, for a VirtualList, the item range; null otherwise. */
  readonly scroll: {
    readonly viewport: readonly [number, number];
    readonly content: readonly [number, number];
    readonly capacity: readonly [number, number];
    readonly itemCount: number | null;
    readonly first: number;
    readonly last: number;
  } | null;
  /** CanvasBounds' evaluated `[x, y, width, height]`. */
  readonly bounds: readonly [number, number, number, number];
}

export interface GalleryGuiState {
  /** The panel World and the depth-first order of its entity tree. */
  readonly world: WorldReference;
  readonly rows: number;
  readonly controls: readonly GalleryGuiControl[];
  /** Canvas text leaves by symbolic ID, in tree order. */
  readonly texts: readonly {
    readonly symbol: string | undefined;
    readonly text: string;
  }[];
  readonly drawings: readonly string[];
  /** Canvas boxes with their style alpha and opacity. */
  readonly boxes: readonly {
    readonly symbol: string | undefined;
    readonly alpha: number;
    readonly opacity: number;
  }[];
  /** The event log's authored VirtualList fields and its declared items in
   * index order. */
  readonly eventLog: {
    readonly itemCount: number;
    readonly itemExtent: number;
    readonly overscan: number;
    readonly items: readonly {
      readonly index: number;
      readonly text: string;
    }[];
  };
  /** Components of the parent panel entity. */
  readonly panelComponents: readonly (string | undefined)[];
  /** Overlay entities and whether each is open and evaluated visible. */
  readonly overlays: readonly {
    readonly symbol: string | undefined;
    readonly open: boolean;
    readonly visible: boolean;
  }[];
}

function componentName(
  client: { readonly components: Readonly<Record<string, { id: number }>> },
  id: number,
) {
  return Object.entries(client.components).find(
    ([, descriptor]) => descriptor.id === id,
  )?.[0];
}

/** Every record of one paged GUI System query of the panel World. */
async function panelGuiQuery<Collection extends "guiFocus" | "guiPointers">(
  client: PanelClient,
  collection: Collection,
): Promise<
  Collection extends "guiFocus" ? GuiFocusRecord[] : GuiPointerRecord[]
> {
  const records: (GuiFocusRecord | GuiPointerRecord)[] = [];
  let after = 0n;
  do {
    const page = await client.inspectPage({ collection, after });
    records.push(...(page[collection] ?? []));
    after = page.next;
  } while (after !== 0n);
  return records as Collection extends "guiFocus"
    ? GuiFocusRecord[]
    : GuiPointerRecord[];
}

/** Exact component incarnations from one lifecycle add page's baselines. */
async function componentIncarnations(
  client: PanelClient,
  components: readonly {
    readonly entity: bigint;
    readonly component: number;
  }[],
): Promise<Map<string, bigint>> {
  const incarnations = new Map<string, bigint>();
  if (components.length === 0) return incarnations;
  const watch = await client.watchLifecycle(
    components.map(({ entity, component }) => ({
      target: { kind: "component", entity, component },
      kinds: 8,
    })),
    () => {},
  );
  try {
    for (const { target, lifetime } of watch.baselines)
      if (
        target.kind === "component" &&
        lifetime.kind === "component" &&
        lifetime.incarnation !== null
      )
        incarnations.set(
          `${target.entity}:${target.component}`,
          lifetime.incarnation,
        );
  } finally {
    await watch.remove();
  }
  return incarnations;
}

/** The controls among `nodes`, in tree order. */
async function panelControls(
  panel: PanelSession,
  entities: ReadonlyMap<bigint, EntitySnapshot>,
  nodes: readonly EntityTreeNode[],
): Promise<GalleryGuiControl[]> {
  const client = panel.client;
  const found = nodes.flatMap(({ id }) => {
    const entity = entities.get(id);
    if (!entity) return [];
    for (const [name, kind] of Object.entries(GALLERY_CONTROL_KINDS)) {
      const fields = componentFields(client, entity, name);
      if (fields)
        return [
          { entity, kind, component: client.components[name]!.id, fields },
        ];
    }
    return [];
  });
  const [incarnations, focus, pointers] = await Promise.all([
    componentIncarnations(
      client,
      found.map(({ entity, component }) => ({ entity: entity.id, component })),
    ),
    panelGuiQuery(client, "guiFocus"),
    panelGuiQuery(client, "guiPointers"),
  ]);
  return found.flatMap(({ entity, kind, component, fields }) => {
    const incarnation = incarnations.get(`${entity.id}:${component}`);
    if (incarnation === undefined) return [];
    const mine = (target: GuiTarget) =>
      target.entity === entity.id && target.component === component;
    const number = (field: string, from = fields) => Number(from[field] ?? 0);
    const pair = (field: string) =>
      [number(`${field}_x`), number(`${field}_y`)] as const;
    const value: GalleryGuiValue =
      kind === "checkbox"
        ? { kind: "bool", value: fields.checked === true }
        : kind === "slider"
          ? { kind: "scalar", value: number("value") }
          : kind === "text"
            ? // A numeric input holds its number rather than its text.
              fields.numeric === true
              ? { kind: "scalar", value: number("value") }
              : { kind: "text", value: String(fields.text ?? "") }
            : kind === "scrollView" || kind === "virtualList"
              ? {
                  kind: "scroll",
                  offset: pair("offset"),
                  anchorIndex: number("anchor_index"),
                  anchorOffset: number("anchor_offset"),
                }
              : { kind: "none" };
    const behavior = componentFields(client, entity, "GuiBehavior") ?? {};
    const bounds = componentFields(client, entity, "CanvasBounds") ?? {};
    const ancestry: bigint[] = [];
    for (
      let parent = entity.link.parent;
      parent !== null;
      parent = entities.get(parent)?.link.parent ?? null
    )
      ancestry.push(parent);
    const interaction = { hovered: false, pressed: false, captured: false };
    for (const record of pointers)
      if (mine(record.target)) {
        interaction.hovered ||= record.state.hovered;
        interaction.pressed ||= record.state.pressed;
        interaction.captured ||= record.state.captured;
      }
    const list = kind === "virtualList";
    return [
      {
        symbol: entity.metadata.symbolicId ?? undefined,
        target: {
          world: panel.world,
          entity: entity.id,
          component,
          incarnation,
        },
        kind,
        value,
        // A non-empty semantic label names the control; otherwise its own
        // visible label does.
        label:
          typeof behavior.semantic_label === "string" &&
          behavior.semantic_label !== ""
            ? behavior.semantic_label
            : typeof fields.label === "string"
              ? fields.label
              : "",
        ancestry,
        overlay: (() => {
          for (const id of [entity.id, ...ancestry]) {
            const holder = entities.get(id);
            if (holder && componentFields(client, holder, "GuiOverlay"))
              return holder.metadata.symbolicId ?? "";
          }
          return undefined;
        })(),
        enabled: behavior.effective_enabled !== false,
        visible: behavior.effective_visible !== false,
        available: behavior.available !== false,
        focused: focus.some((record) => mine(record.target)),
        interaction,
        scroll:
          value.kind === "scroll"
            ? {
                viewport: pair("viewport"),
                content: pair("content"),
                capacity: pair("capacity"),
                itemCount: list ? number("item_count") : null,
                first: list ? number("range_first") : 0,
                last: list ? number("range_last") : 0,
              }
            : null,
        bounds: [
          number("x", bounds),
          number("y", bounds),
          number("width", bounds),
          number("height", bounds),
        ],
      },
    ];
  });
}

/**
 * The panel World's controls and the ordinary Canvas entities around them,
 * plus proof that no legacy GUI producer remains. Every value comes from
 * public inspection, entity tree pages, the GUI focus and pointer queries
 * and lifecycle baselines.
 */
export async function galleryGuiState(flush = true): Promise<GalleryGuiState> {
  const handle = requireCanvas();
  if (flush) await handle.flush();
  const panel = await galleryPanel();
  const [inspection, parentEntity] = await Promise.all([
    panel.client.inspect(),
    galleryEntities([GUI_PANEL_ENTITY]).then(([entity]) => entity),
  ]);
  if (!parentEntity) throw new Error("Missing gallery GUI demo");
  const nodes: EntityTreeNode[] = [];
  let after: bigint | undefined;
  do {
    const page = await panel.client.inspectTreePage({
      ...(after === undefined ? {} : { after }),
      limit: 256,
      maxDepth: 64,
    });
    nodes.push(...page.nodes);
    after = page.next === 0n ? undefined : page.next;
  } while (after !== undefined);
  const entities = new Map(
    inspection.entities.map((entity) => [entity.id, entity]),
  );
  const symbol = (id: bigint) =>
    entities.get(id)?.metadata.symbolicId ?? undefined;
  const field = (entity: EntitySnapshot | undefined, name: string) =>
    entity ? componentFields(panel.client, entity, name) : undefined;
  const ordered = nodes.map(({ id }) => entities.get(id));
  const list = field(
    inspection.entities.find(
      ({ metadata }) => metadata.symbolicId === GUI_EVENT_LOG_ENTITY,
    ),
    "GuiVirtualList",
  );
  const eventLog = {
    itemCount: Number(list?.item_count ?? Number.NaN),
    itemExtent: Number(list?.item_extent ?? Number.NaN),
    overscan: Number(list?.overscan ?? Number.NaN),
    items: inspection.entities
      .filter(
        (entity) =>
          entity.link.parent !== null &&
          symbol(entity.link.parent) === GUI_EVENT_LOG_ENTITY &&
          field(entity, "GuiVirtualItem"),
      )
      .map((item) => {
        // Each declared item holds its entry Text as its one child.
        const entries = inspection.entities.filter(
          (entity) => entity.link.parent === item.id,
        );
        return {
          index: Number(field(item, "GuiVirtualItem")!.index),
          text: entries
            .map((entry) => field(entry, "CanvasText")?.text)
            .filter((text): text is string => typeof text === "string")
            .join("\n"),
        };
      })
      .sort((left, right) => left.index - right.index),
  };
  return {
    world: panel.world,
    rows: nodes.length,
    controls: await panelControls(panel, entities, nodes),
    texts: ordered.flatMap((entity) => {
      const text = field(entity, "CanvasText")?.text;
      return typeof text === "string"
        ? [{ symbol: entity!.metadata.symbolicId ?? undefined, text }]
        : [];
    }),
    drawings: ordered.flatMap((entity) => {
      const source = field(entity, "CanvasDrawing")?.source;
      return typeof source === "string" ? [source] : [];
    }),
    boxes: ordered.flatMap((entity) => {
      if (!field(entity, "CanvasBox")) return [];
      const style = field(entity, "CanvasStyle");
      return [
        {
          symbol: entity!.metadata.symbolicId ?? undefined,
          alpha: Number(style?.alpha ?? 1),
          opacity: Number(style?.opacity ?? 1),
        },
      ];
    }),
    eventLog,
    panelComponents: parentEntity.components.map(({ component }) =>
      componentName(handle.client, component),
    ),
    overlays: ordered.flatMap((entity) => {
      if (!field(entity, "GuiOverlay")) return [];
      const behavior = field(entity, "GuiBehavior");
      return [
        {
          symbol: entity!.metadata.symbolicId ?? undefined,
          open: behavior?.visible === true,
          visible: behavior?.effective_visible === true,
        },
      ];
    }),
  };
}

function selectControl(
  state: GalleryGuiState,
  selector: GalleryGuiSelector,
): GalleryGuiControl {
  const matches = state.controls.filter(
    (control) =>
      control.kind === selector.role &&
      (selector.name === undefined || control.label === selector.name) &&
      (selector.symbol === undefined || control.symbol === selector.symbol),
  );
  if (matches.length !== 1)
    throw new Error(
      `Expected one ${selector.role} '${selector.name ?? ""}', found ${matches.length}`,
    );
  return matches[0]!;
}

/** Paint part keys of the connected runtime contract, loaded like the
 * gallery loads its generated module. */
interface GalleryPaintKeys {
  guiPaintPartIndex(key: {
    readonly part: string;
    readonly state?: string;
    readonly variant?: string;
  }): number;
}

const GALLERY_GENERATED_MODULE = "/target/browser-build/render/generated.js";

/**
 * One authored row of the theme a control's skin references, selected by
 * its generated paint key, as the panel World inspection decodes it.
 */
export async function galleryGuiThemeRow(
  selector: GalleryGuiSelector,
  key: {
    readonly part: string;
    readonly state?: string;
    readonly variant?: string;
  },
): Promise<Readonly<Record<string, unknown>>> {
  const control = selectControl(await galleryGuiState(), selector);
  const panel = await galleryPanel();
  const module = GALLERY_GENERATED_MODULE;
  const contract = (await import(module)) as GalleryPaintKeys;
  const inspection = await panel.client.inspect();
  const entity = inspection.entities.find(
    ({ id }) => id === control.target.entity,
  );
  const reference: unknown =
    entity && componentFields(panel.client, entity, "GuiSkin")?.theme;
  const theme =
    reference && typeof reference === "object" && "value" in reference
      ? reference.value
      : reference;
  const themeEntity = inspection.entities.find(({ id }) => id === theme);
  const table = themeEntity
    ? (componentFields(panel.client, themeEntity, "GuiTheme")?.parts as
        | { rows?: ReadonlyMap<number, Readonly<Record<string, unknown>>> }
        | undefined)
    : undefined;
  const part = contract.guiPaintPartIndex(key);
  const row = [...(table?.rows?.values() ?? [])].find(
    (candidate) => candidate.part === part,
  );
  if (!row)
    throw new Error(
      `${selector.role} '${selector.name ?? ""}' theme has no ${JSON.stringify(key)} row`,
    );
  return row;
}

/** Observe optional layout-root bounds through raw generated-client writes and a completed draw. */
export async function galleryGuiLayoutBounds(symbols: readonly string[]) {
  const panel = await galleryPanel();
  const before = await panel.client.inspect();
  const component = panel.client.components.CanvasBounds!;
  const missing = symbols.filter((symbol) => {
    const entity = before.entities.find(
      (item) => item.metadata.symbolicId === symbol,
    );
    if (!entity) throw new Error(`Missing layout root ${symbol}`);
    return !componentFields(panel.client, entity, "CanvasBounds");
  });
  if (missing.length) {
    const outcome = await panel.client.batch(
      missing.map((symbol) => ({
        kind: "insertComponent" as const,
        entity: { kind: "symbol" as const, symbol },
        component: component.id,
        fields: [],
      })),
    );
    if (!outcome.ok)
      throw new Error("Layout observer components were not inserted");
  }
  await captureCanvas(requireCanvas(), { waitForResources: true });
  const after = await panel.client.inspect();
  return Object.fromEntries(
    symbols.map((symbol) => {
      const entity = after.entities.find(
        (item) => item.metadata.symbolicId === symbol,
      )!;
      const fields = componentFields(panel.client, entity, "CanvasBounds");
      if (!fields) throw new Error(`Missing evaluated bounds ${symbol}`);
      return [
        symbol,
        [
          Number(fields.x),
          Number(fields.y),
          Number(fields.width),
          Number(fields.height),
        ],
      ];
    }),
  );
}

/** Symbolic IDs of the Host's current Worlds. */
export async function galleryWorlds(): Promise<string[]> {
  const worlds = await requireCanvas().host.listWorlds();
  return worlds.map(({ symbolicId }) => symbolicId).sort();
}

/** Dispatch one identity-checked semantic action for machine-access testing. */
export async function galleryGuiAction(
  selector: GalleryGuiSelector,
  action: GuiAction,
): Promise<GalleryGuiState> {
  const handle = requireCanvas();
  const control = selectControl(await galleryGuiState(), selector);
  const panel = await galleryPanel();
  const outcome = await guiAction(panel.client, control.target, action);
  if (!outcome.ok)
    throw new Error(
      `Semantic ${action.kind} on ${selector.role} '${selector.name ?? ""}' was ${JSON.stringify(outcome, (_key, value) => (typeof value === "bigint" ? String(value) : value))}`,
    );
  await handle.flush();
  return galleryGuiState();
}

/** Scan and pulse trace state: the effective Canvas style translation and
 * opacity of each trace entity, and the controller that drives it. */
export interface GalleryWaveformTrace {
  readonly entity: bigint;
  readonly x: number;
  readonly opacity: number;
  readonly controller: AnimationControllerSnapshot | undefined;
}

export interface GalleryWaveform {
  readonly scan: GalleryWaveformTrace;
  readonly pulse: GalleryWaveformTrace;
  /** Every controller of the panel World. */
  readonly controllers: readonly AnimationControllerSnapshot[];
}

/** Read both waveform traces from one panel World inspection. */
export async function galleryWaveform(flush = true): Promise<GalleryWaveform> {
  if (flush) await requireCanvas().flush();
  const panel = await galleryPanel();
  const inspection = await panel.client.inspect();
  const trace = (symbol: string): GalleryWaveformTrace => {
    const entity = inspection.entities.find(
      ({ metadata }) => metadata.symbolicId === symbol,
    );
    if (!entity) throw new Error(`Missing waveform trace ${symbol}`);
    const style = componentFields(panel.client, entity, "CanvasStyle");
    return {
      entity: entity.id,
      x: Number(style?.x ?? Number.NaN),
      opacity: Number(style?.opacity ?? Number.NaN),
      controller: inspection.controllers?.find(({ description }) =>
        description.drivers.some(({ target }) => target === entity.id),
      ),
    };
  };
  return {
    scan: trace("gui-waveform-signal"),
    pulse: trace("gui-waveform-pulse"),
    controllers: inspection.controllers ?? [],
  };
}

/** Project Surface content points, in Canvas units from its top-left
 * corner, through the actual panel Surface and camera transforms: on the
 * Surface plane, or `depth` Surface metres in front of it along its normal,
 * where a layer plane of an exploded panel lies. */
export async function galleryGuiProjection() {
  const [placement, density] = await Promise.all([
    galleryGuiPlacement(),
    galleryGuiDensity(),
  ]);
  return { ...placement, density };
}

export async function projectGalleryGuiContent(
  points: readonly (readonly [number, number])[],
  depth = 0,
  projection?: Awaited<ReturnType<typeof galleryGuiProjection>>,
) {
  await requireCanvas().flush();
  const { entity, camera, surface, shape, density } =
    projection ?? (await galleryGuiProjection());
  return projectSnapshotPoints(
    entity,
    camera,
    points.map((point) =>
      guiSurfacePoint({ surface, shape, density }, point, depth),
    ),
  );
}

/** Test-owned chart oracle shared by frame projection and cover-ray checks. */
function guiSurfacePoint(
  {
    surface,
    shape,
    density,
  }: Pick<
    Awaited<ReturnType<typeof galleryGuiProjection>>,
    "surface" | "shape" | "density"
  >,
  [u, v]: readonly [number, number],
  depth: number,
): number[] {
  const x = u / density - Number(surface.width) / 2;
  const y = Number(surface.height) / 2 - v / density;
  const k = shape === "FlatSurface" ? 0 : Number(surface.curvature);
  if (k === 0) return [x, y, depth];
  const angle =
    k * (shape === "SphereSurface" ? Math.hypot(x, y) : Math.abs(x));
  const sinc = angle === 0 ? 1 : Math.sin(angle) / angle;
  const cosine = Math.cos(angle);
  const factor = 1 + k * depth;
  return [
    x * sinc * factor,
    shape === "SphereSurface" ? y * sinc * factor : y,
    (cosine - 1) / k + depth * cosine,
  ];
}

/** Independently intersect camera-to-content rays with the inspected cover's
 * unit picking box. The first entry axis distinguishes a side wall from its
 * front cap, and target coordinates prove protected-shell enclosure. */
export async function galleryShieldRays(
  points: readonly (readonly [number, number])[],
  depth: number,
) {
  const projection = await galleryGuiProjection();
  const client = requireCanvas().client;
  const entities = (await client.inspect()).entities;
  const shield = entities.find(
    (entity) => entity.metadata.symbolicId === "gui-input-shield",
  );
  if (!shield) throw new Error("Shield is not mounted");
  const parents: EntitySnapshot[] = [];
  for (let at = shield.link.parent; at !== projection.entity.id; ) {
    const parent = entities.find((entity) => entity.id === at);
    if (!parent) throw new Error("Shield must inherit its Surface parent");
    parents.push(parent);
    at = parent.link.parent;
  }
  const frames = [...parents.reverse(), shield];
  const inverse = (entity: EntitySnapshot, point: readonly number[]) => {
    const transform = componentFields(client, entity, "Transform")!;
    return rotateByQuaternion(
      point.map(
        (value, axis) => value - Number(transform[["x", "y", "z"][axis]!]),
      ),
      [
        -Number(transform.qx),
        -Number(transform.qy),
        -Number(transform.qz),
        Number(transform.qw),
      ],
    ).map(
      (value, axis) => value / Number(transform[["sx", "sy", "sz"][axis]!]),
    );
  };
  const camera = componentFields(client, projection.camera, "Transform")!;
  const shieldLocal = (point: readonly number[]) =>
    frames.reduce((at, frame) => inverse(frame, at), point);
  const origin = shieldLocal(
    inverse(projection.entity, [
      Number(camera.x),
      Number(camera.y),
      Number(camera.z),
    ]),
  );
  return points.map((point) => {
    const target = shieldLocal(guiSurfacePoint(projection, point, depth));
    const direction = target.map((value, axis) => value - origin[axis]!);
    let near = -Infinity;
    let far = Infinity;
    let axis = -1;
    for (let index = 0; index < 3; index++) {
      if (Math.abs(direction[index]!) < 1e-12) {
        if (Math.abs(origin[index]!) > 0.5) {
          near = Infinity;
          far = -Infinity;
          break;
        }
        continue;
      }
      const a = (-0.5 - origin[index]!) / direction[index]!;
      const b = (0.5 - origin[index]!) / direction[index]!;
      const entry = Math.min(a, b);
      if (entry > near) {
        near = entry;
        axis = index;
      }
      far = Math.min(far, Math.max(a, b));
    }
    const frontTime = (0.5 - origin[2]!) / direction[2]!;
    const front = origin.map(
      (value, index) => value + frontTime * direction[index]!,
    );
    return { point, origin, target, near, far, axis, front };
  });
}

/** Project a content rectangle `[minX, minY, maxX, maxY]` into normalized
 * completed-frame bounds. */
export async function galleryGuiContentRegion(
  rect: readonly [number, number, number, number],
): Promise<readonly [number, number, number, number]> {
  const [minX, minY, maxX, maxY] = rect;
  const corners = await projectGalleryGuiContent([
    [minX, minY],
    [maxX, minY],
    [minX, maxY],
    [maxX, maxY],
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
  const projected = await projectGalleryGuiContent(logicalPoints);
  const frame = requireCapture(label);
  return projected.map(({ x, y }) =>
    sample(
      frame,
      Math.min(frame.width - 1, Math.max(0, Math.floor(x * frame.width))),
      Math.min(frame.height - 1, Math.max(0, Math.floor(y * frame.height))),
    ),
  );
}

export interface GalleryGuiRegionStats {
  readonly pixels: number;
  readonly mean: readonly [number, number, number];
  readonly min: readonly [number, number, number];
  readonly max: readonly [number, number, number];
}

/**
 * Summarize completed-frame pixels whose centres fall inside each named
 * logical GUI rectangle `[minX, minY, maxX, maxY]`, projected through the
 * actual Surface and camera transforms.
 */
export async function galleryGuiRegionStats(
  label: string,
  rects: Readonly<Record<string, readonly [number, number, number, number]>>,
): Promise<Record<string, GalleryGuiRegionStats>> {
  const frame = requireCapture(label);
  const quads = await projectGalleryGuiRects(frame, rects);
  return Object.fromEntries(
    Object.entries(quads).map(([name, quad]) => [
      name,
      quadStats(frame, quad, name),
    ]),
  );
}

/**
 * Logical `[minX, minY, maxX, maxY]` extent of the painted ink inside each
 * named logical GUI rectangle: the pixels whose brightest channel exceeds
 * the midpoint of that channel's range in the region, mapped back through
 * the rectangle's projected corners. The detail view faces the panel to the
 * camera, so the corner mapping is affine to well under a pixel.
 */
export async function galleryGuiInkBounds(
  label: string,
  rects: Readonly<Record<string, readonly [number, number, number, number]>>,
  depth = 0,
): Promise<Record<string, readonly [number, number, number, number]>> {
  const frame = requireCapture(label);
  const quads = await projectGalleryGuiRects(frame, rects, depth);
  return Object.fromEntries(
    Object.entries(quads).map(([name, quad]) => {
      const [minX, minY, maxX, maxY] = rects[name]!;
      const [[ax, ay], [bx, by], , [dx, dy]] = quad as [
        readonly [number, number],
        readonly [number, number],
        readonly [number, number],
        readonly [number, number],
      ];
      // Solve pixel = a + u (b - a) + v (d - a) for the logical fractions.
      const det = (bx - ax) * (dy - ay) - (by - ay) * (dx - ax);
      const logical = (px: number, py: number) => {
        const u = ((px - ax) * (dy - ay) - (py - ay) * (dx - ax)) / det;
        const v = ((bx - ax) * (py - ay) - (by - ay) * (px - ax)) / det;
        return [minX + u * (maxX - minX), minY + v * (maxY - minY)] as const;
      };
      const pixels = quadPixels(frame, quad).map(
        ([x, y]) =>
          [x, y, Math.max(...sample(frame, x, y).slice(0, 3))] as const,
      );
      const values = pixels.map(([, , value]) => value);
      const threshold = (Math.min(...values) + Math.max(...values)) / 2;
      const ink = pixels.filter(([, , value]) => value > threshold);
      if (ink.length === 0) throw new Error(`GUI region ${name} has no ink`);
      const corners = ink.flatMap(([x, y]) => [
        logical(x, y),
        logical(x + 1, y + 1),
      ]);
      return [
        name,
        [
          Math.min(...corners.map(([x]) => x)),
          Math.min(...corners.map(([, y]) => y)),
          Math.max(...corners.map(([x]) => x)),
          Math.max(...corners.map(([, y]) => y)),
        ] as const,
      ];
    }),
  );
}

/** Project named logical GUI rectangles, on the Surface plane or `depth`
 * metres in front of it, into completed-frame pixel quads, corners ordered
 * min/min, max/min, max/max, min/max. */
async function projectGalleryGuiRects(
  frame: ViewerFrame,
  rects: Readonly<Record<string, readonly [number, number, number, number]>>,
  depth = 0,
): Promise<Record<string, readonly (readonly [number, number])[]>> {
  const names = Object.keys(rects);
  const projected = await projectGalleryGuiContent(
    names.flatMap((name) => {
      const [minX, minY, maxX, maxY] = rects[name]!;
      return [
        [minX, minY],
        [maxX, minY],
        [maxX, maxY],
        [minX, maxY],
      ] as const;
    }),
    depth,
  );
  return Object.fromEntries(
    names.map((name, index) => [
      name,
      projected
        .slice(index * 4, index * 4 + 4)
        .map(({ x, y }) => [x * frame.width, y * frame.height] as const),
    ]),
  );
}

/** Frame pixels whose centres fall inside a convex quad. */
function quadPixels(
  frame: ViewerFrame,
  quad: readonly (readonly [number, number])[],
): (readonly [number, number])[] {
  const inside = (px: number, py: number) => {
    let sign = 0;
    for (let index = 0; index < quad.length; index++) {
      const [ax, ay] = quad[index]!;
      const [bx, by] = quad[(index + 1) % quad.length]!;
      const cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
      if (cross === 0) continue;
      if (sign === 0) sign = Math.sign(cross);
      else if (Math.sign(cross) !== sign) return false;
    }
    return true;
  };
  const xs = quad.map(([x]) => x);
  const ys = quad.map(([, y]) => y);
  const left = Math.max(0, Math.floor(Math.min(...xs)));
  const right = Math.min(frame.width - 1, Math.ceil(Math.max(...xs)));
  const top = Math.max(0, Math.floor(Math.min(...ys)));
  const bottom = Math.min(frame.height - 1, Math.ceil(Math.max(...ys)));
  const pixels: (readonly [number, number])[] = [];
  for (let y = top; y <= bottom; y++)
    for (let x = left; x <= right; x++)
      if (inside(x + 0.5, y + 0.5)) pixels.push([x, y]);
  return pixels;
}

/** Channel statistics over pixels whose centres fall inside a convex quad. */
function quadStats(
  frame: ViewerFrame,
  quad: readonly (readonly [number, number])[],
  name: string,
): GalleryGuiRegionStats {
  const sum = [0, 0, 0];
  const min = [255, 255, 255];
  const max = [0, 0, 0];
  const pixels = quadPixels(frame, quad);
  for (const [x, y] of pixels) {
    const pixel = sample(frame, x, y);
    for (let channel = 0; channel < 3; channel++) {
      sum[channel]! += pixel[channel]!;
      min[channel] = Math.min(min[channel]!, pixel[channel]!);
      max[channel] = Math.max(max[channel]!, pixel[channel]!);
    }
  }
  if (pixels.length === 0)
    throw new Error(`GUI region ${name} covers no pixels`);
  return {
    pixels: pixels.length,
    mean: sum.map((value) => value / pixels.length) as [number, number, number],
    min: min as [number, number, number],
    max: max as [number, number, number],
  };
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

/**
 * The panel's Transform fields as they were before the first override. The
 * component store holds one value, so this helper keeps the authored
 * placement itself to write it back on release.
 */
let galleryGuiTransformOriginal: Readonly<Record<string, number>> | undefined;

/** Write the panel's Transform fields in one batch through its symbolic ID. */
async function writeGalleryGuiTransform(
  fields: Readonly<Record<string, number>>,
  label: string,
): Promise<void> {
  const handle = requireCanvas();
  const component = handle.client.components.Transform!;
  const result = await handle.client.batch(
    Object.entries(fields).map(([name, value]) => ({
      kind: "setField" as const,
      entity: { kind: "symbol" as const, symbol: GUI_PANEL_ENTITY },
      component: component.id,
      field: {
        offset: component.fields[name]!.offset,
        value: { kind: "f32" as const, value },
      },
    })),
  );
  if (!result.ok)
    throw new Error(`GUI placement ${label} failed: ${result.error.reason}`);
  await handle.flush();
}

/**
 * Temporarily override the panel's Transform fields with plain field writes,
 * keeping the authored placement to write back on release. `replace` writes
 * a new override over an applied one in the same batch, restoring the
 * authored value of every field the new override leaves out, so no frame
 * presents the authored placement between the two overrides.
 */
export async function overrideGalleryGuiTransform(
  fields: Readonly<Record<string, number>>,
  replace = false,
): Promise<void> {
  const original = galleryGuiTransformOriginal;
  if (original && !replace)
    throw new Error("A GUI placement override is already applied");
  const handle = requireCanvas();
  await handle.flush();
  const kept =
    original ??
    (await (async () => {
      const [entity] = await galleryEntities([GUI_PANEL_ENTITY]);
      const current = entity
        ? componentFields(handle.client, entity, "Transform")
        : undefined;
      if (!current) throw new Error("The GUI demo has no Transform");
      return Object.fromEntries(
        Object.entries(current).flatMap(([name, value]) =>
          typeof value === "number" ? [[name, value]] : [],
        ),
      );
    })());
  galleryGuiTransformOriginal = kept;
  await writeGalleryGuiTransform(
    original ? { ...original, ...fields } : fields,
    "override",
  );
}

/** Write the kept authored panel placement back. */
export async function releaseGalleryGuiTransform(): Promise<void> {
  const original = galleryGuiTransformOriginal;
  if (!original) throw new Error("No GUI placement override is applied");
  galleryGuiTransformOriginal = undefined;
  await writeGalleryGuiTransform(original, "release");
}

/**
 * Face the panel squarely toward the current camera, filling `fill` of the
 * limiting view axis, so thin skin features span several pixels. A given
 * `distance` places the panel centre that many metres along the view axis
 * instead. Placement changes only projection: layout, paint and semantics
 * are unchanged.
 */
export async function faceGalleryGuiToCamera(
  fill = 0.92,
  distance?: number,
  replace = false,
): Promise<void> {
  const handle = requireCanvas();
  await handle.flush();
  const client = handle.client;
  const { entity, camera, surface } = await galleryGuiPlacement();
  const fields = (owner: typeof camera, name: string) =>
    owner.components.find(
      ({ component }) => component === client.components[name]!.id,
    )!.fields;
  const view = fields(camera, "Transform");
  const projection = fields(camera, "Camera");
  if (Number(projection.projection) === 1)
    throw new Error("Detailed GUI placement expects a perspective camera");
  const panel = fields(entity, "Transform");
  const rotation = ["qx", "qy", "qz", "qw"].map((key) => Number(view[key]));
  const forward = rotateByQuaternion([0, 0, -1], rotation);
  const viewport = requireViewport();
  const tanY = Math.tan(Number(projection.fov_y) / 2);
  const tanX = (tanY * viewport.width) / viewport.height;
  const width = Number(surface.width) * Number(panel.sx);
  const height = Number(surface.height) * Number(panel.sy);
  const along =
    distance ?? Math.max(width / (2 * tanX * fill), height / (2 * tanY * fill));
  // The panel's +Z front faces the camera when it shares the camera rotation.
  await overrideGalleryGuiTransform(
    {
      x: Number(view.x) + forward[0]! * along,
      y: Number(view.y) + forward[1]! * along,
      z: Number(view.z) + forward[2]! * along,
      qx: rotation[0]!,
      qy: rotation[1]!,
      qz: rotation[2]!,
      qw: rotation[3]!,
    },
    replace,
  );
}

/** Drive an acknowledged controller through the production animation
 * protocol, in the gallery World or, with `panel`, in the GUI panel World. */
export async function controlGalleryAnimation(
  id: bigint | readonly bigint[],
  control: AnimationPlaybackControl,
  panel = false,
) {
  const handle = requireCanvas();
  const client = panel
    ? (await galleryPanel()).client
    : (handle.client as AnimationWorldClient);
  await Promise.all(
    (typeof id === "bigint" ? [id] : id).map((controller) =>
      client.controlAnimationController(controller, control),
    ),
  );
  await handle.flush();
}

interface ObservedPhysicalContext {
  send(input: GuiPhysicalInput): Promise<GuiInputRoutingOutcome>;
}

/**
 * The canvas's open physical input context. The Host input connection keeps
 * its contexts privately; this observation reads that map without changing
 * which context the canvas selected.
 */
function physicalContext(): ObservedPhysicalContext {
  const input = requireCanvas().host.input as unknown as {
    readonly contexts: ReadonlyMap<bigint, ObservedPhysicalContext>;
  };
  const contexts = [...input.contexts.values()];
  if (contexts.length !== 1)
    throw new Error(
      `Expected one physical input context, found ${contexts.length}`,
    );
  return contexts[0]!;
}

/** Observe the production input path during sustained DOM input, without gating it. */
export function observeGalleryGuiInput() {
  if (finishGuiInputObservation)
    throw new Error("GUI input observation is already installed");
  const context = physicalContext();
  const send = context.send;
  const pending = new Set<Promise<unknown>>();
  const observation = {
    sent: 0,
    completed: 0,
    peakPending: 0,
    errors: [] as string[],
    /** Routing outcomes of presses, releases and wheel samples, and of any
     * input that rejected or cancelled an action. */
    outcomes: [] as {
      input: string;
      disposition: string;
      applied: number;
      rejected: number;
      cancelled: number;
    }[],
  };
  context.send = function (input) {
    observation.sent++;
    const result = send.call(this, input);
    pending.add(result);
    observation.peakPending = Math.max(observation.peakPending, pending.size);
    void result.then(
      (outcome) => {
        observation.completed++;
        if (outcome.error !== undefined) observation.errors.push(outcome.error);
        if (
          (outcome.rejected ||
            outcome.cancelled ||
            input.kind !== "pointerMove") &&
          observation.outcomes.length < 64
        )
          observation.outcomes.push({
            input: input.kind,
            disposition: outcome.disposition,
            applied: outcome.applied,
            rejected: outcome.rejected,
            cancelled: outcome.cancelled,
          });
        pending.delete(result);
      },
      (error: unknown) => {
        observation.errors.push(
          error instanceof Error ? error.message : String(error),
        );
        pending.delete(result);
      },
    );
    return result;
  };
  finishGuiInputObservation = async (timeoutMs) => {
    // Restore the production path before waiting, so a stuck submission
    // cannot leave the spy installed.
    context.send = send;
    finishGuiInputObservation = undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const settled = await Promise.race([
      Promise.allSettled([...pending]).then(() => true),
      new Promise<false>((resolve) => {
        timer = setTimeout(() => resolve(false), timeoutMs);
      }),
    ]);
    clearTimeout(timer);
    if (!settled)
      throw new Error(
        `${pending.size} GUI input submissions did not settle within ${timeoutMs} ms`,
      );
    return observation;
  };
}

let finishGuiInputObservation:
  | ((timeoutMs: number) => Promise<{
      sent: number;
      completed: number;
      peakPending: number;
      errors: string[];
      outcomes: {
        input: string;
        disposition: string;
        applied: number;
        rejected: number;
        cancelled: number;
      }[];
    }>)
  | undefined;

export async function finishGalleryGuiInputObservation(timeoutMs = 10_000) {
  if (!finishGuiInputObservation)
    throw new Error("GUI input observation was not started");
  return finishGuiInputObservation(timeoutMs);
}

/** The canvas's selected root binding, the view every camera query names. */
function selectedView() {
  const binding = requireCanvas().view?.binding;
  if (!binding) throw new Error("The gallery presents no root output");
  return { kind: "bound" as const, binding };
}

/** Execute the gallery's actual generated camera query path for geometry
 * assertions, against the selected root binding. */
export function cameraQuery(
  input: { readonly type: SystemQuery["type"] } & Record<string, unknown>,
) {
  return (requireCanvas().client as PickingWorldClient).query({
    ...input,
    view: selectedView(),
  } as unknown as SystemQuery);
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
    const { x, y } = point;
    if (x < 0 || x > 1 || y < 0 || y > 1) continue;
    const result = await (requireCanvas().client as PickingWorldClient).query({
      type: "GeometryPickQuery",
      view: selectedView(),
      x,
      y,
      includeViewPlane: true,
    });
    if (result.ok && result.hit?.entity === point.entity)
      return { ...point, hit: result.hit, view: result.view };
  }
  throw new Error(`No visible pick point for ${symbol}`);
}

/** Project explicit object-space probes independently of the runtime's picking shape. */
export async function projectGalleryPoints(
  symbol: string,
  offsets: number[][],
) {
  await requireCanvas().flush();
  const [entity, camera] = await galleryEntities([symbol, "gallery-camera"]);
  if (!entity || !camera) throw new Error(`Missing gallery object ${symbol}`);
  const parents: EntitySnapshot[] = [];
  let parent = entity.link.parent;
  const seen = new Set<bigint>([entity.id]);
  while (parent !== null) {
    if (seen.has(parent)) throw new Error("Cyclic gallery entity placement");
    seen.add(parent);
    const page = await requireCanvas().client.inspectPage({
      collection: "entities",
      target: parent,
      limit: 1,
    });
    const ancestor = page.entities[0];
    if (!ancestor || ancestor.id !== parent)
      throw new Error("Missing gallery placement ancestor");
    parents.push(ancestor);
    parent = ancestor.link.parent;
  }
  return projectSnapshotPoints(entity, camera, offsets, parents);
}

/** The GUI demo, its Surface and the gallery camera, read together. */
async function galleryGuiPlacement() {
  const client = requireCanvas().client;
  const [entity, camera] = await galleryEntities([
    GUI_PANEL_ENTITY,
    "gallery-camera",
  ]);
  if (!entity) throw new Error("Missing gallery GUI demo");
  if (!camera) throw new Error("Missing gallery camera");
  const selected = (
    ["FlatSurface", "CylinderSurface", "SphereSurface"] as const
  )
    .map((shape) => ({
      shape,
      surface: componentFields(client, entity, shape),
    }))
    .find(({ surface }) => surface !== undefined);
  if (!selected?.surface) throw new Error("Missing effective GUI demo Surface");
  return { entity, camera, surface: selected.surface, shape: selected.shape };
}

function requireViewport() {
  const viewport = requireCanvas().view?.binding.viewport;
  if (!viewport) throw new Error("The gallery presents no root output");
  return viewport;
}

/** Project object-space offsets through inspected entity and camera fields. */
function projectSnapshotPoints(
  entity: EntitySnapshot,
  camera: EntitySnapshot,
  offsets: readonly (readonly number[])[],
  parents: readonly EntitySnapshot[] = [],
) {
  const handle = requireCanvas();
  const fields = (entity: typeof camera, name: string) =>
    entity.components.find(
      (entry) => entry.component === handle.client.components[name]!.id,
    )!.fields;
  const objects = [entity, ...parents].flatMap((owner) => {
    const transform = componentFields(handle.client, owner, "Transform");
    return transform ? [transform] : [];
  });
  const view = fields(camera, "Transform");
  const projection = fields(camera, "Camera");
  const viewport = requireViewport();
  const bounds = document
    .querySelector<HTMLCanvasElement>("#ipp-world-canvas")!
    .getBoundingClientRect();
  return offsets.map((local) => {
    let point = [...local];
    for (const object of objects) {
      point = rotateByQuaternion(
        point.map((v, axis) => v * Number(object[["sx", "sy", "sz"][axis]!])),
        ["qx", "qy", "qz", "qw"].map((key) => Number(object[key])),
      ).map((v, axis) => v + Number(object[["x", "y", "z"][axis]!]));
    }
    const relative = point.map(
      (v, axis) => v - Number(view[["x", "y", "z"][axis]!]),
    );
    const cameraPoint = rotateByQuaternion(relative, [
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
      cameraPoint[0]! / ((2 * halfHeight * viewport.width) / viewport.height);
    const y = 0.5 - cameraPoint[1]! / (2 * halfHeight);
    return {
      entity: entity.id,
      x,
      y,
      clientX: bounds.left + x * bounds.width,
      clientY: bounds.top + y * bounds.height,
      viewport: {
        width: viewport.width,
        height: viewport.height,
        devicePixelRatio: viewport.devicePixelRatio,
      },
    };
  });
}

/**
 * Replace decoded schema rows tables' `Map` rows with plain records keyed by
 * slot, so inspections cross the page boundary intact.
 */
function plainRowsTables<T>(inspection: T): T {
  const entities = (inspection as { entities?: readonly unknown[] }).entities;
  for (const entity of entities ?? []) {
    const record = entity as {
      components?: readonly { fields: Record<string, unknown> }[];
    };
    for (const component of record.components ?? [])
      for (const [name, value] of Object.entries(component.fields)) {
        const table = value as { nextSlot?: unknown; rows?: unknown };
        if (table?.rows instanceof Map)
          component.fields[name] = {
            nextSlot: table.nextSlot,
            rows: Object.fromEntries(table.rows),
          };
      }
  }
  return inspection;
}

/** Wait for browser input batching, then use real inspection as an ingress barrier. */
export async function settleGalleryInput() {
  await awaitGalleryIngress();
  return plainRowsTables(await requireCanvas().client.inspect());
}

/** The same ingress barrier, answered by the World summary instead of a full inspection. */
export async function awaitGalleryIngress(): Promise<void> {
  await nextFrame(10_000);
  await nextFrame(10_000);
  await requireCanvas().client.inspectPage();
}

let heldReply:
  | { release(): void; arrived: boolean; outcome?: unknown }
  | undefined;
let heldPresentedFrame:
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
  client.batch = async (operations) => {
    if (
      !operations.some(
        (operation) =>
          operation.kind === "insertComponent" &&
          operation.component === component,
      )
    )
      return batch.call(client, operations);
    client.batch = batch;
    const outcome = await batch.call(client, operations);
    state.outcome = outcome;
    state.arrived = true;
    await gate;
    return outcome;
  };
}

/**
 * Hold the next gallery World batch containing an operation of `kind`, on
 * the named component when given, before it reaches the Host, such as the
 * field writes a React render submits. `queryReplyHeld` reports the held
 * request; releasing it submits the unchanged batch in order.
 */
export function delayNextBatchSubmission(
  kind: Command["kind"],
  componentName?: string,
) {
  if (heldReply) throw new Error("A reply is already delayed");
  const client = requireCanvas().client;
  const batch = client.batch;
  const component =
    componentName === undefined
      ? undefined
      : client.components[componentName]!.id;
  const matches = (operation: Command) =>
    operation.kind === kind &&
    (component === undefined ||
      ("component" in operation && operation.component === component));
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
  client.batch = async (operations) => {
    if (!operations.some(matches)) return batch.call(client, operations);
    client.batch = batch;
    state.outcome = { operations: operations.length };
    state.arrived = true;
    await gate;
    return batch.call(client, operations);
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
  client.batch = async (operations) => {
    client.batch = batch;
    const outcome = await batch.call(client, operations);
    state.outcome = outcome;
    state.arrived = true;
    await gate;
    return outcome;
  };
}

/** Delay the canvas's next completed-frame observation after the Host
 * presents it, so startup readiness cannot precede it. */
export function delayNextPresentedFrame() {
  if (heldPresentedFrame)
    throw new Error("A presented frame is already delayed");
  const handle = requireCanvas();
  const frame = handle.frame;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const state = {
    release: () => {
      handle.frame = frame;
      release();
      heldPresentedFrame = undefined;
    },
    arrived: false,
    outcome: undefined as unknown,
  };
  heldPresentedFrame = state;
  handle.frame = async (options) => {
    handle.frame = frame;
    const presented: PresentedFrame = await frame.call(handle, options);
    state.outcome = {
      sequence: presented.sequence,
      drawCalls: presented.drawCalls,
      failedDrawCalls: presented.failedDrawCalls,
    };
    state.arrived = true;
    await gate;
    return presented;
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

export function presentedFrameHeld() {
  return heldPresentedFrame?.arrived === true;
}

export function releasePresentedFrame() {
  heldPresentedFrame?.release();
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

/** Scene completion is separate from the connected diagnostic canvas handle. */
export async function waitForGallerySceneReady(): Promise<void> {
  const deadline = performance.now() + 15_000;
  for (;;) {
    const status = document.querySelector<HTMLOutputElement>("#status");
    if (status?.dataset.state === "ready") return;
    if (status?.dataset.state === "error")
      throw new Error(status.textContent?.trim() || "Gallery scene failed");
    const remaining = deadline - performance.now();
    if (remaining <= 0) throw new Error("Gallery scene did not become ready");
    await nextFrame(remaining);
  }
}

export async function waitForViewer(): Promise<ViewerObservation> {
  const deadline = performance.now() + 10_000;
  let handle = (window as ViewerWindow).ippWorldCanvas;
  while (handle === undefined || !handle.view) {
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
  await waitForGallerySceneReady();
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
  const {
    frame: presented,
    tick,
    statistics,
    ...captured
  } = await captureCanvas(requireCanvas(), { waitForResources });
  const frame = viewerFrame(presented, tick, statistics);
  const observation = {
    ...captured,
    inspection: plainRowsTables(captured.inspection),
  };
  captures.set(label, { ...frame, pixels: frame.pixels.slice(0) });
  const { pixels: _pixels, ...metadata } = frame;
  return {
    ...observation,
    label,
    frame: metadata,
    summary: summarizeImage(frame),
    dataUrl: await frameDataUrl(frame),
  };
}

/** Capture presented state without waiting for React reconciliation: the
 * completed draw includes content already admitted when it is requested. */
export async function captureUnflushedViewer(label: string) {
  if (!label) throw new Error("Capture label must be nonempty");
  const handle = requireCanvas();
  const view = handle.view;
  if (!view) throw new Error("The gallery presents no root output");
  const captured = await handle.host.presentation.capture(view, {
    afterOutputs: [view.binding.output],
  });
  const source = captured.sources.find(({ output }) =>
    sameOutputReference(output, view.binding.output),
  );
  if (!source) throw new Error("Completed draw omitted the selected output");
  const frame = viewerFrame(
    captured,
    source.tick,
    await renderDiagnostics(handle.host)?.statistics(),
  );
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

/**
 * RGBA pixels of a normalized region of a stored capture, row zero at the
 * top, base64-encoded so node-side comparisons receive the exact bytes.
 */
export function viewerCaptureRegionPixels(
  label: string,
  bounds: readonly [number, number, number, number],
) {
  const frame = requireCapture(label);
  const [u0, v0, u1, v1] = bounds;
  const left = Math.max(0, Math.floor(frame.width * u0));
  const top = Math.max(0, Math.floor(frame.height * v0));
  const right = Math.min(frame.width, Math.ceil(frame.width * u1));
  const bottom = Math.min(frame.height, Math.ceil(frame.height * v1));
  if (!(left < right && top < bottom)) throw new Error("empty image region");
  const source = new Uint8Array(frame.pixels);
  const width = right - left;
  const height = bottom - top;
  let binary = "";
  for (let y = top; y < bottom; y += 1) {
    const row = source.subarray(
      (y * frame.width + left) * 4,
      (y * frame.width + right) * 4,
    );
    for (let index = 0; index < row.length; index += 0x8000)
      binary += String.fromCharCode(...row.subarray(index, index + 0x8000));
  }
  return { left, top, width, height, pixels: btoa(binary) };
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

function requireCapture(label: string): ViewerFrame {
  const frame = captures.get(label);
  if (!frame) throw new Error(`Missing viewer capture '${label}'`);
  return frame;
}

function projectPlanePoint(
  frame: ViewerFrame,
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

/** Rotate a vector by an xyzw quaternion. */
function rotateByQuaternion(v: readonly number[], q: readonly number[]) {
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
}

function dot(a: readonly number[], b: readonly number[]): number {
  return a.reduce((total, value, index) => total + value * b[index]!, 0);
}

function sample(
  frame: ViewerFrame,
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

async function frameDataUrl(frame: ViewerFrame): Promise<string> {
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

/** Diagnostic capture stays on the live gallery Host connection. */
let galleryTraceCapture: string | undefined;
export async function startGalleryTrace(
  maxEvents: number,
  maxArtifactBytes: number,
) {
  const reader = hostProfiling(requireCanvas().host);
  galleryTraceCapture = await reader.start({
    counters: false,
    trace: { maxEvents },
    maxArtifactBytes,
  });
  return galleryTraceCapture;
}
export async function stopGalleryTrace() {
  return hostProfiling(requireCanvas().host).stop();
}
export async function cancelGalleryTrace() {
  if (galleryTraceCapture)
    await hostProfiling(requireCanvas().host).release(galleryTraceCapture);
  galleryTraceCapture = undefined;
}

/** Scene diagnostics stay on the mounted production controller and its generated client. */
export async function gallerySceneState() {
  const scene = (
    window as Window & {
      ippGalleryScene?: { inspect(): Promise<unknown> };
    }
  ).ippGalleryScene;
  if (!scene) throw new Error("No mounted gallery scene");
  await requireCanvas().flush();
  const state = await scene.inspect();
  const world = (state as { world?: Inspection }).world;
  if (world) plainRowsTables(world);
  for (const chart of (
    state as { charts?: readonly { inspection: Inspection }[] }
  ).charts ?? [])
    plainRowsTables(chart.inspection);
  return state;
}

export async function gallerySceneAction(name: string, args?: unknown) {
  const scene = (
    window as Window & {
      ippGalleryScene?: {
        action(name: string, args?: unknown): Promise<unknown>;
      };
    }
  ).ippGalleryScene;
  if (!scene) throw new Error("No mounted gallery scene");
  return scene.action(name, args);
}

/** Observe source disposal through the same production Host dataset connection. */
export async function galleryDatasetExists(name: string): Promise<boolean> {
  try {
    await requireCanvas().host.datasets.read(name);
    return true;
  } catch (error) {
    if (error instanceof Error && error.message === "MissingSource")
      return false;
    throw error;
  }
}
