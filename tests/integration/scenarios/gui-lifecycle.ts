/** Ordinary GUI entity lifecycle through a generated client, independent of
 * process launch and wire layout.
 *
 * GUI is authored as ordinary entities and components in Worlds that select
 * the Canvas System: top-level layout roots, GuiLayout containers and control
 * components placed through core links. Observations read component fields, the GUI focus and pointer
 * queries and lifecycle baselines, and effects; presented
 * scenarios attach the GUI World to a parent root output and drive it with
 * the physical input context of that presentation.
 */
import type {
  AssetWorldClient,
  Client,
  Command,
  ComponentFieldValue,
  EntitySnapshot,
  GuiInputCancellation,
  GuiInputRoutingOutcome,
  GuiPhysicalContext,
  GuiPhysicalInput,
  GuiTarget,
  BatchOutcome,
  GuiWorldClient,
  PresentationView,
  RowsInput,
  WorldPersistenceHostClient,
  WorldReference,
} from "@ipp/client";
import { canvasOutput } from "../../../packages/ipp-client/src/references.js";
import { guiAction } from "../gui-actions.js";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  SURFACE,
  CANVAS,
  GUI,
  selectSystems,
} from "../system-selections.js";

export type GuiHost = WorldPersistenceHostClient<Client>;
export type GuiTestClient = GuiWorldClient & AssetWorldClient;

/** Row encoders exported by the generated contract under test. */
export interface GuiContract {
  GuiTheme: { encodeParts(input: RowsInput): Uint8Array<ArrayBuffer> };
  GuiSkin: { encodeParts(input: RowsInput): Uint8Array<ArrayBuffer> };
  guiPaintPartIndex(input: {
    part: "background";
    state?: "hovered" | "pressed";
  }): number;
}

export function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/** JSON with bigint values, for failure messages and evidence. */
export function encoded(value: unknown): string {
  return JSON.stringify(value, (_key, entry: unknown) =>
    typeof entry === "bigint" ? `${entry}n` : entry,
  );
}

export const alias = (value: number) =>
  ({ kind: "alias", alias: value }) as const;
export const handle = (id: bigint) => ({ kind: "handle", id }) as const;

/** Append `entity` to `parent`'s ordered children. */
export function place(
  entity: ReturnType<typeof alias> | ReturnType<typeof handle>,
  parent: ReturnType<typeof alias> | ReturnType<typeof handle>,
  before: ReturnType<typeof alias> | ReturnType<typeof handle> | null = null,
): Command {
  return { kind: "placeEntity", entity, placement: { parent, before } };
}

/** GuiLayout operations (see the component documentation). */
export const LAYOUT = {
  leaf: 0,
  row: 1,
  column: 2,
  stack: 3,
  padding: 4,
  align: 5,
  sizedBox: 6,
} as const;

export async function openGui(
  host: GuiHost,
  world: WorldReference,
): Promise<GuiTestClient> {
  const client = await host.openWorld(world);
  check(
    "subscribeGuiEffects" in client && "createAsset" in client,
    "Target lacks ordinary GUI or asset authoring",
  );
  return client as GuiTestClient;
}

/** An action batch that applied, or a descriptive failure. */
export function applied(outcome: BatchOutcome) {
  check(outcome.ok, `GUI action did not apply: ${encoded(outcome)}`);
  return outcome;
}

/** Whether the action batch stopped at its operation for exactly `reason`. */
export function rejectedFor(outcome: BatchOutcome, reason: string): boolean {
  return (
    !outcome.ok &&
    outcome.error.scope === "operation" &&
    outcome.error.reason === reason
  );
}

/** The control components, by component name. */
const CONTROL_KINDS = {
  GuiButton: "button",
  GuiCheckbox: "checkbox",
  GuiSlider: "slider",
  GuiTextInput: "text",
  GuiScrollView: "scrollView",
  GuiVirtualList: "virtualList",
} as const;

/** A control's value as its value fields hold it. */
export type GuiControlValue =
  | { kind: "none" }
  | { kind: "bool"; value: boolean }
  | { kind: "scalar"; value: number }
  | { kind: "text"; value: string }
  | {
      kind: "scroll";
      offset: readonly [number, number];
      anchorIndex: number;
      anchorOffset: number;
    };

/**
 * One control's state read through the public surfaces: its component and
 * GuiBehavior/CanvasBounds fields, the GUI focus and pointer queries, and its
 * incarnation from a lifecycle baseline.
 */
export interface GuiControlState {
  readonly target: GuiTarget;
  readonly kind: (typeof CONTROL_KINDS)[keyof typeof CONTROL_KINDS];
  /** The control component's fields by name. */
  readonly fields: Readonly<Record<string, ComponentFieldValue>>;
  readonly value: GuiControlValue;
  readonly label: string;
  /** GuiBehavior's evaluated eligibility. */
  readonly enabled: boolean;
  readonly visible: boolean;
  readonly available: boolean;
  readonly focused: boolean;
  /** Aggregated over the live pointers on this control. */
  readonly interaction: {
    hovered: boolean;
    pressed: boolean;
    captured: boolean;
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

/** The exact target of `component` on `entity`, from a lifecycle baseline. */
export async function controlTarget(
  client: GuiWorldClient,
  entity: bigint,
  component: number,
): Promise<GuiTarget | undefined> {
  const watch = await client.watchLifecycle(
    [{ target: { kind: "component", entity, component }, kinds: 8 }],
    () => {},
  );
  try {
    const lifetime = watch.baselines[0]?.lifetime;
    if (lifetime?.kind !== "component" || lifetime.incarnation === null)
      return undefined;
    check(client.worldReference, "GUI client has no exact World");
    return {
      world: client.worldReference,
      entity,
      component,
      incarnation: lifetime.incarnation,
    };
  } finally {
    await watch.remove();
  }
}

/** The control on `entity`, or undefined when it holds none. */
export async function controlState(
  client: GuiWorldClient,
  entity: bigint,
): Promise<GuiControlState | undefined> {
  const page = await client.inspectPage({
    collection: "entities",
    target: entity,
    limit: 1,
  });
  const snapshot = page.entities.find((item) => item.id === entity);
  if (!snapshot) return undefined;
  const fieldsNamed = (name: string) =>
    snapshot.components.find(
      (component) => component.component === client.components[name]?.id,
    )?.fields;
  const name = (
    Object.keys(CONTROL_KINDS) as (keyof typeof CONTROL_KINDS)[]
  ).find((candidate) => fieldsNamed(candidate) !== undefined);
  if (!name) return undefined;
  const fields = fieldsNamed(name)!;
  const target = await controlTarget(
    client,
    entity,
    client.components[name]!.id,
  );
  if (!target) return undefined;
  const number = (field: string, from = fields) => Number(from[field] ?? 0);
  const pair = (field: string) =>
    [number(`${field}_x`), number(`${field}_y`)] as const;
  const kind = CONTROL_KINDS[name];
  const value: GuiControlValue =
    kind === "checkbox"
      ? { kind: "bool", value: fields.checked === true }
      : kind === "slider"
        ? { kind: "scalar", value: number("value") }
        : kind === "text" && fields.numeric === true
          ? { kind: "scalar", value: number("value") }
          : kind === "text"
            ? { kind: "text", value: String(fields.text ?? "") }
            : kind === "scrollView" || kind === "virtualList"
              ? {
                  kind: "scroll",
                  offset: pair("offset"),
                  anchorIndex: number("anchor_index"),
                  anchorOffset: number("anchor_offset"),
                }
              : { kind: "none" };
  const behavior = fieldsNamed("GuiBehavior") ?? {};
  const bounds = fieldsNamed("CanvasBounds") ?? {};
  const focus = await client.inspectPage({
    collection: "guiFocus",
    target: entity,
  });
  const pointers = await client.inspectPage({
    collection: "guiPointers",
    target: entity,
  });
  const mine = (candidate: GuiTarget) =>
    candidate.entity === entity && candidate.component === target.component;
  const interaction = { hovered: false, pressed: false, captured: false };
  for (const record of pointers.guiPointers ?? [])
    if (mine(record.target)) {
      interaction.hovered ||= record.state.hovered;
      interaction.pressed ||= record.state.pressed;
      interaction.captured ||= record.state.captured;
    }
  const list = kind === "virtualList";
  return {
    target,
    kind,
    fields,
    value,
    label: typeof fields.label === "string" ? fields.label : "",
    enabled: behavior.effective_enabled !== false,
    visible: behavior.effective_visible !== false,
    available: behavior.available !== false,
    focused: (focus.guiFocus ?? []).some((record) => mine(record.target)),
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
  };
}

export async function control(
  client: GuiWorldClient,
  entity: bigint,
): Promise<GuiControlState> {
  const state = await controlState(client, entity);
  check(state, `Entity ${entity} holds no control`);
  return state;
}

/** Every entity row of one World, keyed by symbolic identity. */
export async function entitiesByName(
  client: Client,
): Promise<Map<string, EntitySnapshot>> {
  const rows = new Map<string, EntitySnapshot>();
  let after = 0n;
  do {
    const page = await client.inspectPage({
      collection: "entities",
      after,
      limit: 64,
    });
    for (const entity of page.entities)
      if (entity.metadata.symbolicId)
        rows.set(entity.metadata.symbolicId, entity);
    check(page.next === 0n || page.next > after, "Inspection did not advance");
    after = page.next;
  } while (after !== 0n);
  return rows;
}

export function named(
  rows: ReadonlyMap<string, EntitySnapshot>,
  name: string,
): EntitySnapshot {
  const row = rows.get(name);
  check(row, `Missing ordinary entity ${name}`);
  return row;
}

/** Typed fields of one component in an inspected component list. */
export function fieldsOf(
  client: Client,
  components: EntitySnapshot["components"],
  name: string,
): Record<string, unknown> | undefined {
  return components.find(
    (component) => component.component === client.components[name]?.id,
  )?.fields;
}

/** The deepest relative depth one tree page may cover: a whole GUI subtree. */
export const TREE_PAGE_MAX_DEPTH = 64;

/** Every entity under `root`, in core tree order, with its control if any. */
export async function treeRows(client: GuiWorldClient, root: bigint) {
  const rows: {
    entity: bigint;
    parent: bigint | null;
    depth: number;
    control: GuiControlState | null;
  }[] = [];
  let after: bigint | undefined;
  for (;;) {
    const page = await client.inspectTreePage({
      root,
      ...(after === undefined ? {} : { after }),
      limit: 16,
      maxDepth: TREE_PAGE_MAX_DEPTH,
    });
    for (const node of page.nodes)
      rows.push({
        entity: node.id,
        parent: node.parent,
        depth: node.depth,
        control: (await controlState(client, node.id)) ?? null,
      });
    if (page.next === 0n) return rows;
    check(page.next !== after, "GUI tree cursor did not advance");
    after = page.next;
  }
}

/** Write component fields through ordinary setField commands. */
export function setFields(
  client: Client,
  entity: bigint,
  name: string,
  values: Parameters<typeof componentFields>[2],
): Command[] {
  const component = client.components[name];
  check(component, `Target does not expose ${name}`);
  return componentFields(client, name, values).map((field) => ({
    kind: "setField",
    entity: handle(entity),
    component: component.id,
    field,
  }));
}

/**
 * Compare-and-set one field: write `next` to `field` of component `name` only
 * while it holds `expected`.
 */
export function compareAndSet(
  client: Client,
  entity: bigint,
  name: string,
  field: string,
  expected: Parameters<typeof componentFields>[2][string],
  next: Parameters<typeof componentFields>[2][string],
): Promise<import("@ipp/client").BatchOutcome> {
  const component = client.components[name];
  check(component, `Target does not expose ${name}`);
  const [write] = componentFields(client, name, { [field]: next });
  const [old] = componentFields(client, name, { [field]: expected });
  check(write && old, `Compare-and-set needs ${name}.${field}`);
  return client.batch([
    {
      kind: "setFieldIf",
      entity: handle(entity),
      component: component.id,
      field: write,
      expected: old.value,
    },
  ]);
}

/** The unknown entity handle that makes an operation fail without effects. */
const UNKNOWN_ENTITY = 0xffff_ffff_ffff_ffffn;

/** A command that fails validation without touching any live entity. */
export function failingCommand(): Command {
  return { kind: "delete", entity: handle(UNKNOWN_ENTITY) };
}

/** Button label as its component field holds it. */
async function label(client: GuiWorldClient, entity: bigint): Promise<string> {
  return (await control(client, entity)).label;
}

/** Nodes declared by the 50-entity batch: the column root and 49 buttons. */
const BATCH_ENTITIES = 50;

/**
 * Exercise ordinary GUI declarations against a live World: one-request
 * declaration of 50 entities, prefix-preserving failure, paged batches with
 * FIFO order, explicit held batches, pipelined declarations and reads,
 * one typed store per value, stable identity across reordering,
 * compare-and-set, control incarnations, entity removal and
 * persistence of structure and committed values with fresh targets.
 *
 * Headless: no presentation is selected, so it runs on any Host with GUI.
 * Returned counts keep the batch acknowledgements comparable across drivers.
 */
export async function exerciseGuiLifecycle(
  host: GuiHost,
  contract: GuiContract,
) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let completed = false;
  try {
    const created = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "gui-lifecycle",
      canvas: { extent: [4, 3], unitsPerMetre: 1 },
    });
    worlds.push(created.reference);
    const client = await openGui(host, created.reference);
    sessions.push(client);
    const peerWorld = await host.createWorld({
      symbolicId: "gui-lifecycle-peer",
      selectedSystems: [],
    });
    worlds.push(peerWorld.reference);
    const peer = await host.openWorld(peerWorld.reference);
    sessions.push(peer);

    // One request declares a 50-entity GUI: a column layout root and 49
    // buttons, each placed under the root in declaration order.
    const declaration: Command[] = [
      createEntity(1, "gui-batch-panel"),
      insertComponent(client, "GuiLayout", alias(1), {
        kind: LAYOUT.column,
        width: 4,
        height: 3,
      }),
    ];
    for (let index = 1; index < BATCH_ENTITIES; index++)
      declaration.push(
        createEntity(index + 1, `gui-batch-${index}`),
        insertComponent(client, "GuiButton", alias(index + 1), {
          label: `node ${index}`,
        }),
        place(alias(index + 1), alias(1)),
      );
    const batchOutcome = successfulBatch(await client.batch(declaration));
    check(
      batchOutcome.aliases.length === BATCH_ENTITIES,
      `The GUI declaration acknowledged ${batchOutcome.aliases.length} entities`,
    );
    const batchRoot = aliasId(batchOutcome, 1);
    const node = (index: number) => aliasId(batchOutcome, index + 1);
    await client.waitForFrame();
    const declared = await treeRows(client, batchRoot);
    check(
      declared.length === BATCH_ENTITIES &&
        declared.every(
          (row, index) =>
            row.entity === aliasId(batchOutcome, index + 1) &&
            row.depth === (index === 0 ? 0 : 1) &&
            (index === 0) === (row.control === null) &&
            (index === 0 || row.control?.label === `node ${index}`),
        ),
      `The completed GUI declaration frame omitted or reordered entities: ${encoded(declared.map((row) => [row.entity, row.depth, row.control?.label]))}`,
    );

    // A failing operation keeps the acknowledged prefix and skips the suffix.
    const failedBatch = await client.batch([
      ...setFields(client, node(1), "GuiButton", { label: "prefix applied" }),
      failingCommand(),
      ...setFields(client, node(2), "GuiButton", {
        label: "suffix must not apply",
      }),
    ]);
    check(!failedBatch.ok, "The invalid middle GUI operation was accepted");
    const failedBatchApplied = failedBatch.error.operation;
    check(
      failedBatchApplied === 1,
      `The failed GUI batch reported the wrong prefix: ${encoded(failedBatch.error)}`,
    );
    check(
      (await label(client, node(1))) === "prefix applied",
      "The acknowledged GUI prefix was lost",
    );
    check(
      (await label(client, node(2))) === "node 2",
      "A GUI write after the failed operation was applied",
    );
    successfulBatch(
      await client.batch(
        setFields(client, node(2), "GuiButton", { label: "recovered" }),
      ),
    );
    check(
      (await label(client, node(2))) === "recovered",
      "A correction after a failed GUI batch did not recover",
    );

    // Repeated large labels exceed one message, so the client pages them as
    // one logical batch. Page counts belong to the protocol client tests;
    // this asserts visibility and FIFO order behind the paged batch.
    const largeLabels = (character: string) =>
      Array.from({ length: 18 }, (_, index) =>
        setFields(client, node(1), "GuiButton", {
          label: `${character.repeat(60_000)}${index}`,
        }),
      ).flat();
    const largePromise = client.batch(largeLabels("x"));
    const queuedDirectPromise = client.batch(
      setFields(client, node(2), "GuiButton", {
        label: "queued after multi-page",
      }),
    );
    const labelsOf = (entities: readonly bigint[]) =>
      Promise.all(
        entities.map(async (entity) => {
          const page = await client.inspectPage({
            collection: "entities",
            target: entity,
            limit: 1,
          });
          const row = page.entities.find((item) => item.id === entity);
          return String(
            fieldsOf(client, row?.components ?? [], "GuiButton")?.label ?? "",
          );
        }),
      );
    // Both reads are enqueued now, behind the multi-page batch.
    const queuedInspectionPromise = labelsOf([node(1), node(2)]);
    const largeOutcome = successfulBatch(await largePromise);
    const largeBatchApplied = largeLabels("x").length;
    successfulBatch(await queuedDirectPromise);
    const queued = await queuedInspectionPromise;
    check(
      queued[0] === `${"x".repeat(60_000)}17` &&
        queued[1] === "queued after multi-page",
      `An ordinary request overtook a queued multi-page GUI batch: ${encoded(queued.map((label) => label.slice(-24)))}`,
    );
    check(largeOutcome.tick > 0n, "The multi-page batch has no tick");

    const failedLargePromise = client.batch([
      ...largeLabels("y"),
      failingCommand(),
      ...setFields(client, node(3), "GuiButton", {
        label: "multi-page suffix must not apply",
      }),
    ]);
    const recoveredLargePromise = client.batch(
      setFields(client, node(2), "GuiButton", {
        label: "multi-page recovered",
      }),
    );
    const failedInspectionPromise = labelsOf([node(1), node(2), node(3)]);
    const failedLargeOutcome = await failedLargePromise;
    check(!failedLargeOutcome.ok, "A failing multi-page batch was accepted");
    const failedLargeBatchApplied = failedLargeOutcome.error.operation;
    check(
      failedLargeBatchApplied === 18,
      `A failed later GUI page lost its global acknowledged prefix: ${encoded(failedLargeOutcome.error)}`,
    );
    successfulBatch(await recoveredLargePromise);
    const failedRows = await failedInspectionPromise;
    check(
      failedRows[0] === `${"y".repeat(60_000)}17`,
      "The successful prefix on the failed later GUI page was lost",
    );
    check(
      failedRows[2] === "node 3",
      "A suffix after a failed later GUI page was applied",
    );
    check(
      failedRows[1] === "multi-page recovered",
      "An ordinary request overtook queued recovery after batch failure",
    );
    successfulBatch(
      await client.batch(
        setFields(client, node(2), "GuiButton", {
          label: "automatic gate reused",
        }),
      ),
    );
    check(
      (await label(client, node(2))) === "automatic gate reused",
      "The automatic batch gate could not be reused after queued recovery",
    );

    successfulBatch(
      await client.batch(
        declared.map((row) => ({ kind: "delete", entity: handle(row.entity) })),
      ),
    );

    // The main panel: a column layout root with a styled checkbox and a
    // slider, declared by batches pipelined with a snapshot of the tree.
    const panel = aliasId(
      successfulBatch(
        await client.batch([
          createEntity(1, "gui-panel"),
          insertComponent(client, "GuiLayout", alias(1), {
            kind: LAYOUT.column,
            width: 4,
            height: 3,
          }),
        ]),
      ),
      1,
    );
    const style = {
      red: 0.2,
      green: 0.4,
      blue: 0.6,
      alpha: 1,
      opacity: 1,
    } as const;
    const [checkboxOutcome, sliderOutcome, pipelined] = await Promise.all([
      client.batch([
        createEntity(1, "gui-checkbox"),
        insertComponent(client, "GuiCheckbox", alias(1), {
          checked: false,
          label: "checkbox",
        }),
        insertComponent(client, "GuiLayout", alias(1), { width: 4, height: 1 }),
        insertComponent(client, "CanvasStyle", alias(1), style),
        place(alias(1), handle(panel)),
      ]),
      client.batch([
        createEntity(1, "gui-slider"),
        insertComponent(client, "GuiSlider", alias(1), {
          value: 0.25,
          min: 0,
          max: 1,
          step: 0,
        }),
        insertComponent(client, "GuiLayout", alias(1), { width: 4, height: 1 }),
        place(alias(1), handle(panel)),
      ]),
      client.inspectTreePage({ root: panel, maxDepth: TREE_PAGE_MAX_DEPTH }),
    ]);
    const checkbox = aliasId(successfulBatch(checkboxOutcome), 1);
    const sliderEntity = aliasId(successfulBatch(sliderOutcome), 1);
    check(
      encoded(pipelined.nodes.map((row) => row.id)) ===
        encoded([panel, checkbox, sliderEntity]),
      `A tree read pipelined behind declarations missed them: ${encoded(pipelined.nodes.map((row) => row.id))}`,
    );

    // Every value has one typed store: 16 fully styled entities expose their
    // exact layout and style fields, with no second property representation.
    const denseLayout = {
      kind: LAYOUT.leaf,
      width: 1,
      height: 1,
      min_width: 0.25,
      min_height: 0.25,
      max_width: 2,
      max_height: 2,
      flex: 1,
      align_x: 0,
      align_y: 0,
      padding_top: 0.01,
      padding_right: 0.02,
      padding_bottom: 0.03,
      padding_left: 0.04,
      margin_top: 0.04,
      margin_right: 0.03,
      margin_bottom: 0.02,
      margin_left: 0.01,
      clip: false,
    } as const;
    const denseStyle = {
      x: 0,
      y: 0,
      scale_x: 1,
      scale_y: 1,
      red: 0.1,
      green: 0.2,
      blue: 0.3,
      alpha: 1,
      opacity: 0.75,
      clipped: false,
      clip_min_x: 0,
      clip_min_y: 0,
      clip_max_x: 0,
      clip_max_y: 0,
      layer: 1,
    } as const;
    const denseOutcome = successfulBatch(
      await client.batch(
        Array.from({ length: 16 }, (_, index): Command[] => [
          createEntity(index + 1, `gui-dense-${index}`),
          insertComponent(client, "CanvasText", alias(index + 1), {
            text: `dense ${index}`,
          }),
          insertComponent(client, "GuiLayout", alias(index + 1), denseLayout),
          insertComponent(client, "CanvasStyle", alias(index + 1), denseStyle),
          place(alias(index + 1), handle(panel)),
        ]).flat(),
      ),
    );
    const exact = (values: Readonly<Record<string, number | boolean>>) =>
      Object.fromEntries(
        Object.entries(values).map(([name, value]) => [
          name,
          typeof value === "number" ? Math.fround(value) : value,
        ]),
      );
    const dense = await entitiesByName(client);
    for (let index = 0; index < 16; index++) {
      const entity = named(dense, `gui-dense-${index}`);
      for (const components of [entity.components]) {
        check(
          encoded(fieldsOf(client, components, "GuiLayout")) ===
            encoded(exact(denseLayout)) &&
            encoded(fieldsOf(client, components, "CanvasStyle")) ===
              encoded(exact(denseStyle)) &&
            components.every(
              (component) =>
                Object.keys(component.properties ?? {}).length === 0,
            ),
          `Dense entity ${index} did not keep one typed store: ${encoded(components)}`,
        );
      }
    }
    successfulBatch(
      await client.batch(
        denseOutcome.aliases.map(({ id }) => ({
          kind: "delete",
          entity: handle(id),
        })),
      ),
    );

    // Reordering retains entity and control identity.
    let slider = await control(client, sliderEntity);
    const firstCheckbox = await control(client, checkbox);
    successfulBatch(
      await client.batch([
        place(handle(sliderEntity), handle(panel), handle(checkbox)),
      ]),
    );
    const reordered = await treeRows(client, panel);
    check(
      encoded(reordered.map((row) => row.entity)) ===
        encoded([panel, sliderEntity, checkbox]) &&
        encoded(reordered[1]?.control?.target) === encoded(slider.target) &&
        encoded(reordered[2]?.control?.target) ===
          encoded(firstCheckbox.target),
      "Reordering must retain entity and control identities",
    );

    // Compare-and-set rejects a writer that expected an older value.
    successfulBatch(
      await compareAndSet(
        client,
        sliderEntity,
        "GuiSlider",
        "value",
        0.25,
        0.75,
      ),
    );
    const stale = await compareAndSet(
      client,
      sliderEntity,
      "GuiSlider",
      "value",
      0.25,
      0.1,
    );
    check(
      !stale.ok && stale.error.reason === "ValueMismatch",
      `A stale compare-and-set was accepted: ${encoded(stale)}`,
    );
    slider = await control(client, sliderEntity);
    check(
      slider.value.kind === "scalar" && slider.value.value === 0.75,
      `Compared slider ${encoded(slider)}`,
    );

    // A write delayed across control -> non-control -> control transitions
    // of the same entity is fenced by the control incarnation.
    const sliderType = client.components.GuiSlider!.id;
    const [, , delayed] = await Promise.all([
      client.batch([
        {
          kind: "removeComponent",
          entity: handle(sliderEntity),
          component: sliderType,
        },
      ]),
      client.batch([
        insertComponent(client, "GuiSlider", handle(sliderEntity), {
          value: 0.25,
          min: 0,
          max: 1,
          step: 0,
        }),
      ]),
      guiAction(client, slider.target, { kind: "scalar", value: 0.1 }),
    ]);
    check(
      rejectedFor(delayed, "StaleTarget"),
      `A stale write applied to a re-created control: ${encoded(delayed)}`,
    );
    const recreated = await control(client, sliderEntity);
    check(
      recreated.target.incarnation !== slider.target.incarnation &&
        recreated.value.kind === "scalar" &&
        recreated.value.value === 0.25,
      `The re-created slider reused its incarnation or lost initialization: ${encoded(recreated)}`,
    );
    applied(
      await guiAction(client, recreated.target, {
        kind: "scalar",
        value: 0.75,
      }),
    );

    // A partial style write preserves omitted fields.
    successfulBatch(
      await client.batch(
        setFields(client, checkbox, "CanvasStyle", { opacity: 0.5 }),
      ),
    );
    const patched = fieldsOf(
      client,
      named(await entitiesByName(client), "gui-checkbox").components,
      "CanvasStyle",
    );
    check(
      patched?.opacity === 0.5 &&
        patched.red === Math.fround(style.red) &&
        patched.green === Math.fround(style.green) &&
        patched.blue === Math.fround(style.blue),
      `Style write replaced omitted fields: ${encoded(patched)}`,
    );

    // Skin override rows live on their entity and die with it; the removed
    // control's target cannot be retargeted and identities are not reused.
    const background = contract.guiPaintPartIndex({ part: "background" });
    successfulBatch(
      await client.batch([
        {
          kind: "insertComponent",
          entity: handle(checkbox),
          component: client.components.GuiSkin!.id,
          fields: [
            {
              offset: client.components.GuiSkin!.fields.parts!.offset,
              value: {
                kind: "rows",
                value: contract.GuiSkin.encodeParts({
                  nextSlot: 1,
                  rows: new Map([
                    [0, { part: background, color: [1, 0, 0, 1] }],
                  ]),
                }),
              },
            },
          ],
        },
      ]),
    );
    const skinned = named(await entitiesByName(client), "gui-checkbox");
    check(
      fieldsOf(client, skinned.components, "GuiSkin") !== undefined,
      "A skin override did not attach to its control",
    );
    const removedCheckbox = await control(client, checkbox);
    successfulBatch(
      await client.batch([{ kind: "delete", entity: handle(checkbox) }]),
    );
    const afterRemoval = await entitiesByName(client);
    check(
      !afterRemoval.has("gui-checkbox") &&
        encoded((await treeRows(client, panel)).map((row) => row.entity)) ===
          encoded([panel, sliderEntity]),
      "The removed control entity or its skin rows survived",
    );
    const removedAction = await guiAction(client, removedCheckbox.target, {
      kind: "toggle",
    });
    check(
      rejectedFor(removedAction, "StaleTarget"),
      `A removed control target was accepted: ${encoded(removedAction)}`,
    );
    check(
      !(
        await client.batch(
          setFields(client, checkbox, "CanvasStyle", { opacity: 1 }),
        )
      ).ok,
      "A removed entity handle was accepted",
    );
    const reused = aliasId(
      successfulBatch(
        await client.batch([
          createEntity(1, "gui-reused"),
          insertComponent(client, "GuiCheckbox", alias(1)),
          place(alias(1), handle(panel)),
        ]),
      ),
      1,
    );
    check(reused !== checkbox, "A removed entity identity was reused");

    // An invalid GUI value is rejected at the write with no effect.
    const invalidWrite = await client.batch(
      setFields(client, sliderEntity, "GuiLayout", { align_x: 5 }),
    );
    check(
      !invalidWrite.ok &&
        invalidWrite.error.operation === 0 &&
        invalidWrite.error.reason === "InvalidValue",
      `An out-of-range GUI value was accepted: ${encoded(invalidWrite)}`,
    );
    const rejectedLayout = fieldsOf(
      client,
      named(await entitiesByName(client), "gui-slider").components,
      "GuiLayout",
    );
    check(
      rejectedLayout?.align_x === 2 && rejectedLayout.width === 4,
      `A rejected write changed the layout: ${encoded(rejectedLayout)}`,
    );

    // Persistence keeps structure, configuration and committed values; the
    // restored World takes only its own fresh targets.
    const before = await treeRows(client, panel);
    const beforeSlider = await control(client, sliderEntity);
    const bytes = await host.saveWorld(client.session);
    const loaded = await host.loadWorld(bytes, { symbolicId: "gui-restored" });
    worlds.push(...loaded.created.values());
    const restored = await openGui(host, loaded.root);
    sessions.push(restored);
    const restoredRows = await entitiesByName(restored);
    const names = new Map(
      [...(await entitiesByName(client))].map(([name, row]) => [row.id, name]),
    );
    const restoredNames = new Map(
      [...restoredRows].map(([name, row]) => [row.id, name]),
    );
    const restoredPanel = named(restoredRows, "gui-panel").id;
    const after = await treeRows(restored, restoredPanel);
    check(
      encoded(after.map((row) => restoredNames.get(row.entity))) ===
        encoded(before.map((row) => names.get(row.entity))),
      `Restored tree ${encoded(after.map((row) => restoredNames.get(row.entity)))}`,
    );
    const restoredSlider = await control(
      restored,
      named(restoredRows, "gui-slider").id,
    );
    check(
      encoded(restoredSlider.value) === encoded(beforeSlider.value) &&
        restoredSlider.target.world.id === loaded.root.id,
      `Restored World lost the committed slider value: ${encoded(restoredSlider)}`,
    );
    // Restore keeps entity and component identities, and a command's target
    // is World-local: the World a batch is submitted to decides which control
    // it names, so an action there never reaches the saved World.
    applied(
      await guiAction(restored, restoredSlider.target, {
        kind: "scalar",
        value: 0.5,
      }),
    );
    check(
      encoded((await control(client, sliderEntity)).value) ===
        encoded(beforeSlider.value),
      "A restored-World action mutated the saved World",
    );
    completed = true;
    return {
      batchApplied: batchOutcome.aliases.length,
      failedBatchApplied,
      largeBatchApplied,
      failedLargeBatchApplied,
      sliderIncarnations: [
        String(slider.target.incarnation),
        String(recreated.target.incarnation),
      ],
      restoredWorld: encoded(loaded.root),
      restoredTree: after.map((row) => restoredNames.get(row.entity)),
      restoredSlider: { value: restoredSlider.value },
    };
  } finally {
    await cleanup(host, sessions, worlds, completed);
  }
}

/** "notSent" when the SDK refuses a request before transmission. */
/** A layout root whose root-filling ScrollView reports the World canvas's
 * evaluated logical extent, with a themed 1 x 0.5 button at its content
 * origin. */
function densityCanvas(
  client: Client,
  first: number,
  name: string,
  theme: ReturnType<typeof alias>,
): Command[] {
  const root = alias(first);
  const viewport = alias(first + 1);
  const button = alias(first + 2);
  return [
    createEntity(first, name),
    insertComponent(client, "GuiLayout", root, { kind: LAYOUT.column }),
    createEntity(first + 1, `${name}-viewport`),
    insertComponent(client, "GuiScrollView", viewport, { axis: 1 }),
    insertComponent(client, "GuiLayout", viewport, { kind: LAYOUT.column }),
    place(viewport, root),
    createEntity(first + 2, `${name}-button`),
    insertComponent(client, "GuiButton", button, { label: "" }),
    insertComponent(client, "GuiLayout", button, { width: 1, height: 0.5 }),
    {
      kind: "insertComponent",
      entity: button,
      component: client.components.GuiSkin!.id,
      fields: [
        {
          offset: client.components.GuiSkin!.fields.theme!.offset,
          value: { kind: "entity", value: theme },
        },
      ],
    },
    place(button, viewport),
  ];
}

/** Opaque red button background, independent of hover or press. */
function redTheme(
  client: Client,
  contract: GuiContract,
  at: number,
): Command[] {
  const background = (state?: "hovered" | "pressed") =>
    contract.guiPaintPartIndex({
      part: "background",
      ...(state ? { state } : {}),
    });
  return [
    createEntity(at, "gui-red-theme"),
    {
      kind: "insertComponent",
      entity: alias(at),
      component: client.components.GuiTheme!.id,
      fields: [
        {
          offset: client.components.GuiTheme!.fields.parts!.offset,
          value: {
            kind: "rows",
            value: contract.GuiTheme.encodeParts({
              nextSlot: 3,
              rows: new Map([
                [
                  0,
                  // A plain box: no line and none of the default look's
                  // corner cuts, which would cut away much of a small button.
                  {
                    part: background(),
                    color: [1, 0, 0, 1],
                    corner_radius: [0, 0],
                    corner_cut: [0, 0, 0, 0],
                    border_width: 0,
                  },
                ],
                [1, { part: background("hovered"), color: [1, 0, 0, 1] }],
                [2, { part: background("pressed"), color: [1, 0, 0, 1] }],
              ]),
            }),
          },
        },
      ],
    },
  ];
}

/** Whether a pixel is the opaque red of {@link redTheme}. */
function red([r, g, b]: readonly number[]): boolean {
  return r! > 200 && g! < 60 && b! < 60;
}

/** One presented density root, in its own World. */
interface DensitySide {
  readonly name: string;
  readonly world: WorldReference;
  readonly client: GuiTestClient;
  readonly viewport: bigint;
  readonly button: bigint;
  readonly x: number;
}

/**
 * Each World canvas owns its density: two canvas Worlds presented side by
 * side evaluate their logical extents from their own units per metre, while
 * authored logical sizes stay fixed. Canvas state updates reflow only their
 * own World, writing the authored value back restores it, invalid values have
 * no effect, and restored Worlds evaluate their persisted densities.
 * Observed through semantic ScrollView viewports, physical hit testing and
 * completed frames. A Surface attachment presents exactly one child World,
 * so each root lives in its own World.
 */
export async function exerciseGuiDensity(host: GuiHost, contract: GuiContract) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let presentation: PresentedGui | undefined;
  let completed = false;
  try {
    const sides: DensitySide[] = [];
    for (const [index, { name, units }] of [
      { name: "gui-density-plain", units: 1 / 32 },
      { name: "gui-density-dense", units: 1 / 16 },
    ].entries()) {
      const created = await host.createWorld({
        selectedSystems: selectSystems(GUI, LIFECYCLE),
        symbolicId: name,
        canvas: { extent: [4, 3], unitsPerMetre: units },
      });
      worlds.push(created.reference);
      const client = await openGui(host, created.reference);
      sessions.push(client);
      const outcome = successfulBatch(
        await client.batch([
          ...redTheme(client, contract, 9),
          ...densityCanvas(client, 1, name, alias(9)),
        ]),
      );
      sides.push({
        name,
        world: created.reference,
        client,
        viewport: aliasId(outcome, 2),
        button: aliasId(outcome, 3),
        x: index * 128,
      });
    }
    const [plain, dense] = sides as [DensitySide, DensitySide];
    const anchor = (world: WorldReference, x: number): GuiAnchor => ({
      child: world,
      x,
      width: 128,
      height: 192,
    });
    presentation = await presentGui(
      host,
      sides.map((side) => anchor(side.world, side.x)),
    );
    const p = presentation;
    const effects: GuiObservedEffectLike[] = [];
    const subscriptions = await Promise.all(
      sides.map((side) =>
        side.client.subscribeGuiEffects((effect) => effects.push(effect), {
          classes: "all",
        }),
      ),
    );
    const extents = async (
      observed: readonly { client: GuiWorldClient; viewport: bigint }[],
      expected: readonly (readonly [number, number])[],
    ) => {
      const deadline = Date.now() + 10_000;
      for (;;) {
        await p.frame();
        const actual = await Promise.all(
          observed.map(
            async ({ client, viewport }) =>
              (await control(client, viewport)).scroll?.viewport,
          ),
        );
        if (
          actual.every(
            (extent, index) =>
              extent !== undefined &&
              Math.abs(extent[0] - expected[index]![0]) < 1e-3 &&
              Math.abs(extent[1] - expected[index]![1]) < 1e-3,
          )
        )
          return actual;
        check(
          Date.now() < deadline,
          `Canvas extents ${encoded(actual)} never reached ${encoded(expected)}`,
        );
      }
    };
    const pressCount = (side: DensitySide) =>
      effects.filter(
        (effect) =>
          effect.target.world.id === side.world.id &&
          effect.target.entity === side.button &&
          effect.effect.kind === "pressed",
      ).length;
    const tap = async (point: [number, number], pointer: bigint) => {
      await p.send({ kind: "pointerDown", pointer, point });
      return await p.send({ kind: "pointerUp", pointer, point });
    };

    // Plain spans 128 x 192 m at 1/32 unit per metre: 4 x 6 units; dense
    // spans the same Surface at 1/16: 8 x 12 units. The 1 x 0.5 button is
    // 32 x 16 px on plain and 16 x 8 px on dense.
    await extents(sides, [
      [4, 6],
      [8, 12],
    ]);
    let frame = await p.settled();
    const pixels = (x: number) => red(pixel(frame, [x / 256, 4 / 192]));
    check(
      pixels(24) && !pixels(40) && pixels(128 + 12) && !pixels(128 + 20),
      `Per-World densities did not scale the painted button independently: ${encoded([0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 128, 132, 136, 140, 144].map((x) => [x, pixel(frame, [x / 256, 4 / 192])]))}`,
    );
    await tap(p.point(0, [0.9, 0.25], 1 / 32), 1n);
    await tap(p.point(0, [1.1, 0.25], 1 / 32), 2n);
    await tap(p.point(1, [0.9, 0.25], 1 / 16), 3n);
    await tap(p.point(1, [1.1, 0.25], 1 / 16), 4n);
    const deadline = Date.now() + 10_000;
    while (pressCount(plain) + pressCount(dense) < 2) {
      check(Date.now() < deadline, "Density hit tests published no press");
      await p.frame();
    }
    check(
      pressCount(plain) === 1 && pressCount(dense) === 1,
      `Hit testing ignored per-World density: ${encoded(effects.map((effect) => [effect.target.world.id, effect.target.entity, effect.effect.kind]))}`,
    );

    // A density update reflows only its own World canvas.
    const setDensity = (side: DensitySide, unitsPerMetre: number) =>
      side.client.sendCommand({
        type: "CanvasStateUpdateCommand",
        unitsPerMetre,
      });
    setDensity(plain, 1 / 64);
    await extents(sides, [
      [2, 3],
      [8, 12],
    ]);
    frame = await p.settled();
    check(
      pixels(48) && !pixels(72) && pixels(128 + 12) && !pixels(128 + 20),
      "A density update did not reflow only its own World canvas",
    );

    // A density update over the dense canvas reflows it; writing the
    // authored density back restores its layout.
    const density = async () => (await canvasState(dense.client)).unitsPerMetre;
    setDensity(dense, 1 / 8);
    await extents(sides, [
      [2, 3],
      [16, 24],
    ]);
    check(
      (await density()) === 1 / 8,
      `A density write was not stored: ${encoded(await density())}`,
    );
    setDensity(dense, 1 / 16);
    await extents(sides, [
      [2, 3],
      [8, 12],
    ]);
    check(
      (await density()) === 1 / 16,
      "Writing the authored density back did not restore it",
    );
    await Promise.all(
      subscriptions.map((subscription) => subscription.unsubscribe()),
    );

    // Invalid densities have no effect: both canvases keep their stored
    // densities and keep evaluating. A later valid extent update in the same
    // session order shows the invalid updates were applied first.
    const invalidWrites = [0, -1];
    for (const units of invalidWrites) setDensity(plain, units);
    plain.client.sendCommand({
      type: "CanvasStateUpdateCommand",
      extent: [5, 3],
    });
    const invalidState = {
      units: (
        await awaitCanvasState(plain.client, (state) => state.extent[0] === 5)
      ).unitsPerMetre,
    };
    check(
      invalidState.units === 1 / 64,
      `An invalid density changed the stored value: ${encoded(invalidState)}`,
    );
    await extents(sides, [
      [2, 3],
      [8, 12],
    ]);

    // Restored Worlds evaluate their persisted densities.
    const restored = [];
    for (const [index, side] of sides.entries()) {
      const bytes = await host.saveWorld(side.client.session);
      const loaded = await host.loadWorld(bytes, {
        symbolicId: `${side.name}-restored`,
      });
      worlds.push(...loaded.created.values());
      const client = await openGui(host, loaded.root);
      sessions.push(client);
      const rows = await entitiesByName(client);
      await p.retarget(index, anchor(loaded.root, side.x));
      restored.push({
        client,
        viewport: named(rows, `${side.name}-viewport`).id,
        units: (await canvasState(client)).unitsPerMetre,
      });
    }
    const restoredExtents = await extents(restored, [
      [2, 3],
      [8, 12],
    ]);
    check(
      encoded(restored.map((side) => side.units)) === encoded([1 / 64, 1 / 16]),
      `Restored densities ${encoded(restored.map((side) => side.units))}`,
    );
    completed = true;
    return {
      presses: [pressCount(plain), pressCount(dense)],
      invalidWrites,
      invalidState,
      restoredUnits: restored.map((side) => side.units),
      restoredExtents,
    };
  } finally {
    await presentation?.close().catch(() => {});
    await cleanup(host, sessions, worlds, completed);
  }
}

type GuiObservedEffectLike = Parameters<
  Parameters<GuiWorldClient["subscribeGuiEffects"]>[0]
>[0];

/**
 * Save a World while a TextInput holds focus, a selection and an open
 * composition and a checkbox holds a pointer press, then restore it: the
 * restored World keeps the structure, committed values and skin overrides,
 * holds no focus, capture, selection or composition, and takes only fresh
 * targets. Its completed frame matches the frame of the same committed state
 * before any interaction, while the interacting frame differs from both.
 *
 * The 4 x 3 panel stacks a checkbox, a TextInput and a slider, one unit each.
 */
export async function exerciseGuiTransientRestore(
  host: GuiHost,
  contract: GuiContract,
  fontBytes: ArrayBuffer,
) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let presentation: PresentedGui | undefined;
  let completed = false;
  try {
    const created = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "gui-transient",
      canvas: PANEL_CANVAS,
    });
    worlds.push(created.reference);
    const client = await openGui(host, created.reference);
    sessions.push(client);
    const font = await client.createAsset(17, fontBytes);
    const row = { width: 4, height: 1 } as const;
    const outcome = successfulBatch(
      await client.batch([
        createEntity(1, "gui-transient-panel"),
        insertComponent(client, "GuiLayout", alias(1), {
          kind: LAYOUT.column,
          width: 4,
          height: 3,
        }),
        insertComponent(client, "GuiFont", alias(1), {
          source: font.source,
          font_size: 0.6,
        }),
        createEntity(2, "gui-transient-checkbox"),
        insertComponent(client, "GuiCheckbox", alias(2), {
          checked: true,
        }),
        insertComponent(client, "GuiLayout", alias(2), row),
        {
          kind: "insertComponent",
          entity: alias(2),
          component: client.components.GuiSkin!.id,
          fields: [
            {
              offset: client.components.GuiSkin!.fields.parts!.offset,
              value: {
                kind: "rows",
                value: contract.GuiSkin.encodeParts({
                  nextSlot: 1,
                  rows: new Map([
                    [
                      0,
                      {
                        part: contract.guiPaintPartIndex({
                          part: "background",
                        }),
                        color: [0.9, 0.2, 0.1, 1],
                      },
                    ],
                  ]),
                }),
              },
            },
          ],
        },
        place(alias(2), alias(1)),
        createEntity(3, "gui-transient-text"),
        insertComponent(client, "GuiTextInput", alias(3), {
          text: "ab",
        }),
        insertComponent(client, "GuiLayout", alias(3), row),
        place(alias(3), alias(1)),
        createEntity(4, "gui-transient-slider"),
        insertComponent(client, "GuiSlider", alias(4), {
          value: 0.75,
          min: 0,
          max: 1,
          step: 0,
        }),
        insertComponent(client, "GuiLayout", alias(4), row),
        place(alias(4), alias(1)),
      ]),
    );
    const controls = [2, 3, 4].map((index) => aliasId(outcome, index));
    presentation = await presentGui(host, [{ child: created.reference }]);
    const p = presentation;
    // Caret and focus fences need evaluated layout with a ready font.
    await loadedFont(client);
    await client.waitForFrame();
    const committed = await p.settled();
    const checkboxPoint = p.point(0, [2, 0.5]);

    // Interaction state: a held press capturing pointer 1 on the checkbox,
    // then keyboard focus, a selection and a composition on the TextInput.
    const press = await p.send({
      kind: "pointerDown",
      pointer: 1n,
      point: checkboxPoint,
    });
    // The press may focus the checkbox; traversal continues to the input.
    const traversal: GuiInputRoutingOutcome[] = [];
    while (
      !(await control(client, controls[1]!)).focused &&
      traversal.length < 3
    )
      traversal.push(await p.send({ kind: "key", key: "tab" }));
    check(
      routed(press) && traversal.every(routed),
      `Interaction before save was not routed: ${encoded([press, traversal])}`,
    );
    const text = await nativeText(p, (state) => state?.text === "ab");
    await p.input.editText(text!.fence, {
      kind: "selection",
      start: 0,
      end: 1,
    });
    await p.input.editText(p.input.nativeText!.fence, {
      kind: "composition",
      text: "zz",
      caretStart: 2,
      caretEnd: 2,
    });
    const composing = await nativeText(
      p,
      (state) => state?.composition?.text === "zz",
    );
    const interacting = await p.settled();
    const before = await Promise.all(
      controls.map((entity) => control(client, entity)),
    );
    check(
      before[0]!.interaction.pressed && before[1]!.focused,
      `The saved World held no press or focus: ${encoded(before)}`,
    );

    const bytes = await host.saveWorld(client.session);
    const loaded = await host.loadWorld(bytes, {
      symbolicId: "gui-transient-restored",
    });
    worlds.push(...loaded.created.values());
    const restored = await openGui(host, loaded.root);
    sessions.push(restored);
    const rows = await entitiesByName(restored);
    const names = [
      "gui-transient-checkbox",
      "gui-transient-text",
      "gui-transient-slider",
    ];
    const after = await Promise.all(
      names.map((name) => control(restored, named(rows, name).id)),
    );
    const committedValues = (states: readonly GuiControlState[]) =>
      encoded(states.map((state) => state.value));
    check(
      committedValues(after) === committedValues(before),
      `Restored committed values ${committedValues(after)} differ from ${committedValues(before)}`,
    );
    check(
      after.every(
        (state) =>
          !state.focused &&
          !state.interaction.hovered &&
          !state.interaction.pressed &&
          !state.interaction.captured,
      ),
      `Restored World kept interaction state: ${encoded(after)}`,
    );
    check(
      fieldsOf(
        restored,
        named(rows, "gui-transient-checkbox").components,
        "GuiSkin",
      ) !== undefined,
      "Restored World lost the checkbox skin override",
    );

    // Present the restored World in place of the saved one: the old focus
    // and composition end, the held press completes nothing, and stale
    // fences and targets are refused.
    await p.retarget(0, { child: loaded.root });
    await nativeText(p, (state) => state === null);
    const release = await p.send({
      kind: "pointerUp",
      pointer: 1n,
      point: checkboxPoint,
    });
    const commit = await p.input
      .editText(composing!.fence, { kind: "commitComposition" })
      .catch((error: unknown) => ({ error: String(error) }));
    await restored.waitForFrame();
    const unchanged = await Promise.all(
      names.map((name) => control(restored, named(rows, name).id)),
    );
    const sourceCheckbox = await control(client, controls[0]!);
    check(
      committedValues(unchanged) === committedValues(before) &&
        encoded(sourceCheckbox.value) === encoded(before[0]!.value) &&
        (!("applied" in commit) || commit.applied === 0),
      `The held press or composition completed after restore: ${encoded({ release, commit, unchanged, sourceCheckbox })}`,
    );
    // A fresh tap focuses the restored TextInput with a collapsed selection
    // and no composition.
    const textPoint = p.point(0, [2, 1.5]);
    await p.send({ kind: "pointerDown", pointer: 2n, point: textPoint });
    await p.send({ kind: "pointerUp", pointer: 2n, point: textPoint });
    const refocused = await nativeText(
      p,
      (state) => state?.fence.target.world.id === loaded.root.id,
    );
    check(
      refocused!.text === "ab" &&
        refocused!.composition === undefined &&
        refocused!.selectionStart === refocused!.selectionEnd,
      `Refocused restored TextInput kept transient text state: ${encoded(refocused)}`,
    );

    // The restored frame shows the committed values and skin exactly as
    // before any interaction; the interacting frame differs from both.
    await p.send({ kind: "key", key: "escape" });
    await nativeText(p, (state) => state === null);
    await restored.waitForFrame();
    const restoredFrame = await p.settled();
    const interactionPixels = changedPixels(committed, interacting);
    const restoredPixels = changedPixels(committed, restoredFrame);
    const panelPixels = contentPixels(restoredFrame);
    check(
      panelPixels > 1000 && interactionPixels > 50 && restoredPixels === 0,
      `Restored frame does not match the committed frame: ${encoded({ panelPixels, interactionPixels, restoredPixels })}`,
    );
    completed = true;
    return {
      values: committedValues(after),
      restoredWorld: encoded(loaded.root),
      release: release.disposition,
      frames: { panelPixels, interactionPixels, restoredPixels },
    };
  } finally {
    await presentation?.close().catch(() => {});
    await cleanup(host, sessions, worlds, completed);
  }
}

/** Wait, presenting frames, until the context's native text satisfies
 * `accept`; native state is published asynchronously after routing. */
export async function nativeText(
  presentation: PresentedGui,
  accept: (state: GuiPhysicalContext["nativeText"]) => boolean,
  context: () => Promise<unknown> = async () => undefined,
): Promise<GuiPhysicalContext["nativeText"]> {
  const deadline = Date.now() + 10_000;
  for (;;) {
    const state = presentation.input.nativeText;
    if (accept(state)) return state;
    if (Date.now() >= deadline)
      throw new Error(
        `Native text never reached the expected state: ${encoded({ state, context: await context() })}`,
      );
    await presentation.frame();
  }
}

/** Close sessions and destroy Worlds; failures only matter after success. */
export async function cleanup(
  host: GuiHost,
  sessions: readonly Client[],
  worlds: readonly WorldReference[],
  strict: boolean,
): Promise<void> {
  const closed = await Promise.allSettled(
    sessions.map((session) => session.close()),
  );
  const destroyed = await Promise.allSettled(
    worlds.map((world) => host.destroyWorld(world)),
  );
  if (strict)
    check(
      [...closed, ...destroyed].every(
        (result) => result.status === "fulfilled",
      ),
      `GUI fixture cleanup failed: ${encoded([...closed, ...destroyed].filter((result) => result.status === "rejected").map((result) => String((result as PromiseRejectedResult).reason)))}`,
    );
}

/** Wait until the World's asset resources have loaded, so text layout has
 * font metrics before focus and caret placement. */
export async function loadedFont(client: Client): Promise<void> {
  for (let attempt = 0; ; attempt += 1) {
    const page = await client.inspectPage({ collection: "resources" });
    if (
      page.resources.length > 0 &&
      page.resources.every((item) => item.status === "loaded")
    )
      return;
    check(attempt < 400, "The GUI font never loaded");
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

/** Physical root viewport of every presented scenario, in CSS pixels. */
export const VIEWPORT = {
  width: 256,
  height: 192,
  devicePixelRatio: 1,
} as const;

/** Units per parent metre that give a full-viewport panel a 4 x 3 logical
 * extent. The parent World's root canvas maps one CSS pixel to one metre. */
export const PANEL_DENSITY = 1 / 64;

/** Canvas state of a 4 x 3 panel World presented over the full viewport. */
export const PANEL_CANVAS = {
  extent: [4, 3],
  unitsPerMetre: PANEL_DENSITY,
} as const;

/** The Canvas System state of a canvas World. */
export async function canvasState(client: Client) {
  const record = (await client.inspectPage({ collection: "canvas" })).canvas;
  check(record, "The World reported no Canvas System state");
  return record.state;
}

/** Wait until a canvas World's state satisfies `ready`: Canvas state
 * updates carry no reply and apply in session order. */
export async function awaitCanvasState(
  client: Client,
  ready: (state: Awaited<ReturnType<typeof canvasState>>) => boolean,
) {
  const deadline = Date.now() + 10_000;
  for (;;) {
    const state = await canvasState(client);
    if (ready(state)) return state;
    check(
      Date.now() < deadline,
      `Canvas state ${encoded(state)} never became ready`,
    );
    await client.waitForFrame();
  }
}

/** Completed RGBA frame of a presentation, top row first. */
export interface GuiFrame {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}

/** One Surface anchor on the parent World's canvas, in parent metres; it
 * presents the child World's canvas. */
export interface GuiAnchor {
  readonly child: WorldReference;
  readonly x?: number;
  readonly y?: number;
  readonly width?: number;
  readonly height?: number;
}

/** A parent root output presenting GUI Worlds through Surface attachments,
 * with the physical input context of that presentation. */
export interface PresentedGui {
  readonly parent: GuiTestClient;
  readonly anchors: readonly bigint[];
  readonly view: PresentationView;
  readonly input: GuiPhysicalContext;
  /** Native cancellations the Host published for this context. */
  readonly cancellations: GuiInputCancellation[];
  /** Normalized root point of a child-logical point on anchor `index`. */
  point(
    index: number,
    logical: readonly [number, number],
    density?: number,
  ): [number, number];
  send(input: GuiPhysicalInput): Promise<GuiInputRoutingOutcome>;
  frame(): Promise<void>;
  capture(): Promise<GuiFrame>;
  /** Capture until two consecutive completed frames agree. */
  settled(): Promise<GuiFrame>;
  /** Present another child World on anchor `index`. */
  retarget(index: number, anchor: GuiAnchor): Promise<void>;
  close(): Promise<void>;
}

function attachmentFields(client: Client, anchor: GuiAnchor) {
  const attachment = client.components.WorldAttachment!;
  return [
    {
      offset: attachment.fields.child!.offset,
      value: { kind: "world", value: anchor.child },
    },
    {
      offset: attachment.fields.mode!.offset,
      value: { kind: "u32", value: 1 },
    },
  ] as const;
}

/** Present `anchors` on a new parent canvas World selected as root output. */
export async function presentGui(
  host: GuiHost,
  presented: readonly GuiAnchor[],
): Promise<PresentedGui> {
  const anchors = [...presented];
  // The root viewport sizes the parent canvas in CSS pixels.
  const world = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CANVAS, SURFACE, LIFECYCLE),
    symbolicId: "gui-presentation",
  });
  const parent = await openGui(host, world.reference);
  const rects = anchors.map((anchor) => ({
    x: anchor.x ?? 0,
    y: anchor.y ?? 0,
    width: anchor.width ?? VIEWPORT.width,
    height: anchor.height ?? VIEWPORT.height,
  }));
  const commands: Command[] = [];
  anchors.forEach((anchor, index) => {
    const ref = alias(index + 2);
    const rect = rects[index]!;
    commands.push(
      createEntity(index + 2, `gui-presentation-anchor-${index}`),
      insertComponent(parent, "Surface", ref, {
        width: rect.width,
        height: rect.height,
      }),
      insertComponent(parent, "CanvasStyle", ref, { x: rect.x, y: rect.y }),
      {
        kind: "insertComponent",
        entity: ref,
        component: parent.components.WorldAttachment!.id,
        fields: [...attachmentFields(parent, anchor)],
      },
    );
  });
  const outcome = successfulBatch(await parent.batch(commands));
  const binding = await host.setRootOutput(
    canvasOutput(world.reference),
    VIEWPORT,
  );
  const view = await host.presentation.select(
    await host.presentation.surface(),
    binding,
  );
  await host.presentation.frame(view);
  const input = await host.input.open(view);
  const cancellations: GuiInputCancellation[] = [];
  input.onCancel((event) => cancellations.push(event));
  const capture = async (): Promise<GuiFrame> => {
    const frame = await host.presentation.capture(view);
    return {
      width: VIEWPORT.width,
      height: VIEWPORT.height,
      pixels: new Uint8Array(frame.pixels),
    };
  };
  const densities = anchors.map(() => PANEL_DENSITY);
  return {
    parent,
    anchors: anchors.map((_, index) => aliasId(outcome, index + 2)),
    view,
    input,
    cancellations,
    point(index, [x, y], density = densities[index]!) {
      const rect = rects[index]!;
      return [
        (rect.x + x / density) / VIEWPORT.width,
        (rect.y + y / density) / VIEWPORT.height,
      ];
    },
    send: (event) =>
      input.send(event).catch((error: unknown) => {
        throw new Error(`Physical input ${encoded(event)} failed`, {
          cause: error,
        });
      }),
    async frame() {
      await host.presentation.frame(view);
    },
    capture,
    async settled() {
      let previous = await capture();
      for (let attempt = 0; attempt < 60; attempt += 1) {
        const next = await capture();
        if (changedPixels(previous, next, 0) === 0) return next;
        previous = next;
      }
      throw new Error("GUI presentation did not settle");
    },
    async retarget(index, anchor) {
      const attachment = parent.components.WorldAttachment!;
      const entity = handle(aliasId(outcome, index + 2));
      // Another child World replaces the attachment component.
      successfulBatch(
        await parent.batch([
          {
            kind: "insertComponent",
            entity,
            component: attachment.id,
            fields: [...attachmentFields(parent, anchor)],
          },
        ]),
      );
      anchors[index] = anchor;
      await host.presentation.frame(view);
    },
    async close() {
      await input.close().catch(() => {});
      await host.presentation.clear(view).catch(() => {});
      await parent.close().catch(() => {});
      await host.destroyWorld(world.reference);
    },
  };
}

/** Pixels whose colour channels differ by more than `tolerance`. */
export function changedPixels(a: GuiFrame, b: GuiFrame, tolerance = 2): number {
  check(
    a.width === b.width && a.height === b.height,
    "Compared frames differ in size",
  );
  let changed = 0;
  for (let offset = 0; offset < a.pixels.length; offset += 4)
    for (let channel = 0; channel < 3; channel += 1)
      if (
        Math.abs(a.pixels[offset + channel]! - b.pixels[offset + channel]!) >
        tolerance
      ) {
        changed += 1;
        break;
      }
  return changed;
}

/** Pixels that differ from the frame's top-left background pixel. */
export function contentPixels(frame: GuiFrame): number {
  let content = 0;
  for (let offset = 0; offset < frame.pixels.length; offset += 4)
    for (let channel = 0; channel < 3; channel += 1)
      if (
        Math.abs(frame.pixels[offset + channel]! - frame.pixels[channel]!) > 2
      ) {
        content += 1;
        break;
      }
  return content;
}

/** RGBA of the pixel under a normalized root point. */
export function pixel(
  frame: GuiFrame,
  [x, y]: readonly [number, number],
): [number, number, number, number] {
  const column = Math.min(frame.width - 1, Math.floor(x * frame.width));
  const row = Math.min(frame.height - 1, Math.floor(y * frame.height));
  const offset = (row * frame.width + column) * 4;
  return [
    frame.pixels[offset]!,
    frame.pixels[offset + 1]!,
    frame.pixels[offset + 2]!,
    frame.pixels[offset + 3]!,
  ];
}

/** Whether a physical routing outcome applied at least one GUI action. */
export function routed(outcome: GuiInputRoutingOutcome): boolean {
  return outcome.disposition === "routed" && outcome.error === undefined;
}

/** Target identity key for effect filtering. */
export function sameTarget(a: GuiTarget, b: GuiTarget): boolean {
  return (
    a.world.id === b.world.id &&
    a.world.incarnation === b.world.incarnation &&
    a.entity === b.entity &&
    a.component === b.component &&
    a.incarnation === b.incarnation
  );
}
