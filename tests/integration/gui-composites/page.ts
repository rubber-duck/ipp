/**
 * The browser side of the composites harness: one page's parent canvas and
 * the panel Worlds it presents, the real input relay, and the reads and
 * waits every part's cases share.
 *
 * A part names its panels, each a child World of the parent canvas at its
 * own origin; the parent attaches them in that order, which is also the
 * order Tab crosses them. Each panel's builder declares its content through
 * the panel's generated client, by commands or by a React root, and names
 * the controls and overlays the part's cases address. Real DOM input reaches
 * the Host's physical ingress through the reusable canvas adapter; clients
 * act through their generated batch clients. Reads go through public
 * inspection, effect subscriptions and lifecycle watches only.
 */
import type {
  AssetWorldClient,
  Client,
  Command,
  GuiInputRoutingOutcome,
  GuiObservedEffect,
  GuiPhysicalInput,
  GuiPhysicalKey,
  GuiWorldClient,
  HostClientBase,
  InspectionPage,
  InspectionQuery,
  PresentationView,
  WorldReference,
} from "@ipp/client";
import { canvasOutput } from "../../../packages/ipp-client/src/references.js";
import { attachCanvasGuiInput } from "../../../packages/ipp-react/src/gui/input.js";
import type { GuiKitContract } from "../../../packages/ipp-react/src/gui-kit.js";
import {
  accepted,
  effectLog,
  guiAction,
  type GuiEffectLog,
} from "../gui-actions.js";
import {
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import {
  controlTarget,
  type GuiControlValue,
} from "../scenarios/gui-lifecycle.js";
import {
  ATTACHMENTS,
  GUI,
  LIFECYCLE,
  SURFACE,
  selectSystems,
} from "../system-selections.js";
export { nativePresentationTransport } from "../../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../../packages/ipp-client/src/worker.js";

export type PanelClient = Client & GuiWorldClient;

export function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export const json = (value: unknown) =>
  JSON.stringify(value, (_, item) =>
    typeof item === "bigint" ? item.toString() : item,
  );

/** What the harness hands a part's `prepare`. */
export interface CompositeSetup {
  readonly host: HostClientBase<Client>;
  readonly canvas: HTMLCanvasElement;
  readonly fontBytes: ArrayBuffer;
  /** The Host's generated contract module, for the GUI kit. */
  readonly contract: GuiKitContract;
}

/** What a panel's builder receives: its own World and session. */
export interface PanelContext {
  readonly host: HostClientBase<Client>;
  readonly world: WorldReference;
  readonly client: PanelClient;
  /** The shared GUI font, as an asset source of the panel's World. */
  readonly font: string;
  readonly contract: GuiKitContract;
  /** Errors a React root reports; any fails the part's next wait. */
  readonly report: (error: Error) => void;
}

/** What a panel's builder returns. */
export interface PanelBuild {
  /** Named controls and overlays, unique across the page. */
  readonly entities?: Readonly<Record<string, bigint>>;
  /** A key the runtime returned unhandled, as an application passes it on. */
  unhandledKey?(key: GuiPhysicalKey): void;
  close?(): Promise<void>;
}

/** A panel World of the page: where it sits and what declares it. */
export interface PanelSpec<Build extends PanelBuild = PanelBuild> {
  readonly name: string;
  /** Logical origin in the parent canvas, at one unit per CSS pixel. */
  readonly origin: readonly [number, number];
  readonly extent: readonly [number, number];
  build(context: PanelContext): Promise<Build>;
}

/** The parent canvas that holds every panel at its origin. */
export function canvasOf(panels: readonly PanelSpec[]) {
  return {
    width: Math.max(
      ...panels.map((panel) => panel.origin[0] + panel.extent[0]),
    ),
    height: Math.max(
      ...panels.map((panel) => panel.origin[1] + panel.extent[1]),
    ),
  } as const;
}

/**
 * Host time of one World's ticks: every frame event its session receives and
 * every inspection answered for it, each an exact tick with its World time.
 * The Host lets a newer frame event supersede one waiting behind other
 * output, so a tick without a sample of its own is bounded by the samples
 * around it. Delays and repeats count the frame deltas a World receives, so
 * these bounds hold however loaded the machine is.
 */
export class HostClock {
  readonly #samples = new Map<bigint, number>();
  #stopped = false;

  constructor(private readonly client: PanelClient) {
    void this.#follow();
  }

  /** Record the World time an inspection or frame event reported at `tick`. */
  note(tick: bigint, time: number): void {
    if (tick > 0n) this.#samples.set(tick, time);
  }

  async #follow(): Promise<void> {
    let after = 0n;
    while (!this.#stopped) {
      try {
        const frame = await this.client.waitForFrame(after);
        this.note(frame.tick, frame.time);
        after = frame.tick;
      } catch {
        if (this.#stopped || this.client.closure) return;
      }
    }
  }

  /** A sample of the current tick, answered by an inspection. */
  async sample(): Promise<{ tick: bigint; time: number }> {
    const page = await this.client.inspectPage({ collection: "guiFocus" });
    return { tick: page.tick, time: page.time };
  }

  /**
   * Host-time bounds of `tick`: the latest sample at or before it and the
   * earliest at or after it. The tick was observed, so a fresh sample is at
   * or after it when no recorded one is.
   */
  async bounds(tick: bigint): Promise<{
    tick: bigint;
    early: number;
    late: number;
  }> {
    if (![...this.#samples.keys()].some((sampled) => sampled >= tick))
      await this.sample();
    let early = Number.NEGATIVE_INFINITY;
    let before = -1n;
    let late = Number.POSITIVE_INFINITY;
    let after: bigint | undefined;
    for (const [sampled, time] of this.#samples) {
      if (sampled <= tick && sampled > before) {
        before = sampled;
        early = time;
      }
      if (sampled >= tick && (after === undefined || sampled < after)) {
        after = sampled;
        late = time;
      }
    }
    return { tick, early, late };
  }

  stop(): void {
    this.#samples.clear();
    this.#stopped = true;
  }
}

/**
 * Bounds of the Host time between two observed ticks: the least it can be
 * and the most. A runtime that waits `delay` from `from` before `to` can
 * only produce `most >= delay`, whatever the sampling.
 */
export async function hostInterval(clock: HostClock, from: bigint, to: bigint) {
  const start = await clock.bounds(from);
  const end = await clock.bounds(to);
  return {
    from: start,
    to: end,
    least: end.early - start.late,
    most: end.late - start.early,
  };
}

/** A named entity of the page and the panel holding it. */
interface Named {
  readonly name: string;
  readonly panel: PagePanel;
  readonly entity: bigint;
}

/** One presented panel World. */
export interface PagePanel {
  readonly name: string;
  readonly world: WorldReference;
  readonly client: PanelClient;
  readonly font: string;
  readonly origin: readonly [number, number];
  readonly extent: readonly [number, number];
  readonly build: PanelBuild;
  readonly log: GuiEffectLog;
  readonly clock: HostClock;
}

/** A control as the public reads report it, read in one round trip. */
export interface ControlRead {
  readonly component: string;
  readonly fields: Readonly<Record<string, unknown>>;
  readonly value: GuiControlValue;
  /** The evaluated logical box in its panel's canvas. */
  readonly bounds: readonly [number, number, number, number];
  readonly focused: boolean;
  readonly interaction: {
    hovered: boolean;
    pressed: boolean;
    captured: boolean;
  };
}

const CONTROL_COMPONENTS = [
  "GuiButton",
  "GuiCheckbox",
  "GuiSlider",
  "GuiTextInput",
  "GuiScrollView",
  "GuiVirtualList",
  "GuiColor",
] as const;

/** One component's fields of an entity snapshot, by component name. */
export function componentFields(
  client: Client,
  page: InspectionPage,
  entity: bigint,
  component: string,
): Readonly<Record<string, unknown>> | undefined {
  const id = client.components[component]?.id;
  return page.entities
    .find((item) => item.id === entity)
    ?.components.find((item) => item.component === id)?.fields;
}

/**
 * The control on `entity`: its component's fields, value and evaluated box
 * with its focus and pointer feedback, from three inspections answered
 * together; undefined when it holds no control.
 */
export async function readControl(
  client: PanelClient,
  entity: bigint,
): Promise<ControlRead | undefined> {
  const [page, focus, pointers] = await Promise.all([
    client.inspectPage({ collection: "entities", target: entity, limit: 1 }),
    client.inspectPage({ collection: "guiFocus", target: entity }),
    client.inspectPage({ collection: "guiPointers", target: entity }),
  ]);
  const component = CONTROL_COMPONENTS.find(
    (name) => componentFields(client, page, entity, name) !== undefined,
  );
  if (!component) return undefined;
  const fields = componentFields(client, page, entity, component)!;
  const bounds = componentFields(client, page, entity, "CanvasBounds") ?? {};
  const number = (field: string, from = fields) => Number(from[field] ?? 0);
  const value: GuiControlValue =
    component === "GuiCheckbox"
      ? { kind: "bool", value: fields.checked === true }
      : component === "GuiSlider" ||
          (component === "GuiTextInput" && fields.numeric === true)
        ? { kind: "scalar", value: number("value") }
        : component === "GuiTextInput"
          ? { kind: "text", value: String(fields.text ?? "") }
          : component === "GuiScrollView" || component === "GuiVirtualList"
            ? {
                kind: "scroll",
                offset: [number("offset_x"), number("offset_y")],
                anchorIndex: number("anchor_index"),
                anchorOffset: number("anchor_offset"),
              }
            : { kind: "none" };
  const componentId = client.components[component]!.id;
  const mine = (target: { entity: bigint; component: number }) =>
    target.entity === entity && target.component === componentId;
  const interaction = { hovered: false, pressed: false, captured: false };
  for (const record of pointers.guiPointers ?? [])
    if (mine(record.target)) {
      interaction.hovered ||= record.state.hovered;
      interaction.pressed ||= record.state.pressed;
      interaction.captured ||= record.state.captured;
    }
  return {
    component,
    fields,
    value,
    bounds: [
      number("x", bounds),
      number("y", bounds),
      number("width", bounds),
      number("height", bounds),
    ],
    focused: (focus.guiFocus ?? []).some((record) => mine(record.target)),
    interaction,
  };
}

/** Entities of a World by symbolic id, from its entity pages alone. */
export async function symbols(client: Client): Promise<Map<string, bigint>> {
  const found = new Map<string, bigint>();
  let after = 0n;
  do {
    const page = await client.inspectPage({
      collection: "entities",
      after,
      limit: 64,
    });
    for (const entity of page.entities)
      if (entity.metadata.symbolicId)
        found.set(entity.metadata.symbolicId, entity.id);
    check(page.next === 0n || page.next > after, "Inspection did not advance");
    after = page.next;
  } while (after !== 0n);
  return found;
}

/** Whether `entity` has a GuiBehavior whose `visible` holds: an open overlay. */
export async function overlayVisible(client: Client, entity: bigint) {
  const page = await client.inspectPage({
    collection: "entities",
    target: entity,
    limit: 1,
  });
  return componentFields(client, page, entity, "GuiBehavior")?.visible === true;
}

/** How long a wait may take before it fails with its description. */
const WAIT_MS = 15_000;

/** Frames a transient read across Worlds may take to settle. */
const FOCUS_REREADS = 5;

/** A focus record of the page: the named control, its ring and part. */
export interface FocusRead {
  readonly name: string;
  readonly visible: boolean;
  readonly part: number;
}

/**
 * Open the page: the parent canvas World, each panel World at its origin, the
 * presentation of the parent as the root output, and the input relay.
 */
export async function openCompositePage(
  setup: CompositeSetup,
  specs: readonly PanelSpec[],
) {
  const { host, canvas, fontBytes, contract } = setup;
  const extent = canvasOf(specs);
  const parent = (
    await host.createWorld({
      selectedSystems: selectSystems(ATTACHMENTS, GUI, SURFACE, LIFECYCLE),
      symbolicId: "composite-parent",
      canvas: { extent: [extent.width, extent.height], unitsPerMetre: 1 },
    })
  ).reference;

  // Physical input errors and root errors fail the next wait.
  const failures: string[] = [];
  const report = (error: Error) => failures.push(error.message);
  const panels = new Map<string, PagePanel>();
  const named = new Map<string, Named>();
  for (const spec of specs) {
    const world = (
      await host.createWorld({
        selectedSystems: selectSystems(GUI, LIFECYCLE),
        symbolicId: `composite-${spec.name}`,
        canvas: { extent: [...spec.extent], unitsPerMetre: 1 },
      })
    ).reference;
    const client = (await host.openWorld(world)) as PanelClient;
    check("subscribeGuiEffects" in client, "GUI capability missing");
    // Every inspection answered for the panel is a sample of its clock.
    const clock = new HostClock(client);
    const inspect = client.inspectPage.bind(client);
    client.inspectPage = async (query?: InspectionQuery) => {
      const page = await inspect(query);
      clock.note(page.tick, page.time);
      return page;
    };
    const font = (
      await (client as unknown as AssetWorldClient).createAsset(17, fontBytes)
    ).source;
    const build = await spec.build({
      host,
      world,
      client,
      font,
      contract,
      report,
    });
    const panel: PagePanel = {
      name: spec.name,
      world,
      client,
      font,
      origin: spec.origin,
      extent: spec.extent,
      build,
      log: await effectLog(client),
      clock,
    };
    panels.set(spec.name, panel);
    for (const [name, entity] of Object.entries(build.entities ?? {})) {
      check(!named.has(name), `Two panels name ${name}`);
      named.set(name, { name, panel, entity });
    }
  }

  // The parent stacks each panel's slot at its origin and attaches the
  // panel's World there, in the order the part lists them.
  const alias = (value: number) => ({ kind: "alias" as const, alias: value });
  const parentClient = await host.openWorld(parent);
  const attachment = parentClient.components.WorldAttachment!;
  const parentCommands: Command[] = [
    createEntity(1, "composite-stack"),
    insertComponent(parentClient, "GuiLayout", alias(1), {
      kind: 3,
      width: extent.width,
      height: extent.height,
    }),
  ];
  specs.forEach((spec, index) => {
    const slot = alias(index + 2);
    const [width, height] = spec.extent;
    parentCommands.push(
      createEntity(index + 2, `composite-${spec.name}-slot`),
      insertComponent(parentClient, "FlatSurface", slot, { width, height }),
      insertComponent(parentClient, "GuiLayout", slot, {
        kind: 0,
        width,
        height,
        margin_left: spec.origin[0],
        margin_top: spec.origin[1],
        align_x: -1,
        align_y: -1,
      }),
      {
        kind: "insertComponent",
        entity: slot,
        component: attachment.id,
        fields: [
          {
            offset: attachment.fields.child!.offset,
            value: { kind: "world", value: panels.get(spec.name)!.world },
          },
          {
            offset: attachment.fields.mode!.offset,
            value: { kind: "u32", value: 1 },
          },
        ],
      },
      {
        kind: "placeEntity",
        entity: slot,
        placement: { parent: alias(1), before: null },
      },
    );
  });
  successfulBatch(await parentClient.batch(parentCommands));
  await parentClient.close();

  const binding = await host.setRootOutput(canvasOutput(parent), {
    ...extent,
    devicePixelRatio: 1,
  });
  const view: PresentationView = await host.presentation.select(
    await host.presentation.surface(),
    binding,
  );
  await host.presentation.frame(view);
  const input = await host.input.open(view);

  // Every routed outcome, in send order, beside the input that produced it.
  const outcomes: {
    input: GuiPhysicalInput;
    outcome: GuiInputRoutingOutcome;
  }[] = [];
  const send = input.send.bind(input);
  input.send = async (event) => {
    const outcome = await send(event);
    outcomes.push({ input: event, outcome });
    return outcome;
  };
  // Inputs the adapter reported unhandled to its client, in report order.
  const unhandled: GuiPhysicalInput[] = [];
  const detach = attachCanvasGuiInput(canvas, input, {
    onError: report,
    onUnhandled: (event) => {
      unhandled.push(event);
      if (event.kind === "key")
        for (const panel of panels.values())
          panel.build.unhandledKey?.(event.key);
    },
  });
  // Whether the browser's own menu was suppressed for each context event.
  const browserMenus: boolean[] = [];
  const menus = (event: Event) => browserMenus.push(event.defaultPrevented);
  window.addEventListener("contextmenu", menus);
  canvas.focus({ preventScroll: true });

  const panelNamed = (name: string) => {
    const panel = panels.get(name);
    check(panel, `No panel ${name}`);
    return panel;
  };
  const entry = (name: string) => {
    const found = named.get(name);
    check(found, `No control or overlay named ${name}`);
    return found;
  };
  const read = async (name: string) => {
    const { panel, entity } = entry(name);
    const found = await readControl(panel.client, entity);
    check(found, `Control ${name} is missing`);
    return found;
  };

  /** Frames until `ready` holds, failing with `describe` after a while. */
  const until = async (
    ready: () => Promise<boolean> | boolean,
    describe: () => Promise<string> | string,
  ) => {
    const deadline = performance.now() + WAIT_MS;
    while (!(await ready())) {
      check(performance.now() < deadline, await describe());
      await host.presentation.frame(view);
    }
    check(failures.length === 0, `Input or root errors: ${failures}`);
  };

  /** Positions in the effect logs and routed outcomes to read after. */
  const cut = () => ({
    effects: Object.fromEntries(
      [...panels.values()].map((panel) => [
        panel.name,
        panel.log.effects.length,
      ]),
    ) as Record<string, number>,
    outcomes: outcomes.length,
    unhandled: unhandled.length,
  });
  type Cut = ReturnType<typeof cut>;

  /** Effects of `kind` on `entity` of `panel` since `from`. */
  const effectsOn = (
    panel: PagePanel,
    entity: bigint,
    kind: GuiObservedEffect["effect"]["kind"],
    from: Cut,
  ) =>
    panel.log.effects
      .slice(from.effects[panel.name] ?? 0)
      .filter(
        (effect) =>
          effect.target.entity === entity && effect.effect.kind === kind,
      );
  const effects = (
    name: string,
    kind: GuiObservedEffect["effect"]["kind"],
    from: Cut,
  ) => {
    const { panel, entity } = entry(name);
    return effectsOn(panel, entity, kind, from);
  };

  /** Frames until an input matching `matches` sent after `from` settled. */
  const outcome = async (
    matches: (input: GuiPhysicalInput) => boolean,
    from: Cut,
    describe: string,
  ) => {
    const latest = () =>
      outcomes.slice(from.outcomes).findLast((item) => matches(item.input))
        ?.outcome;
    await until(
      () => latest() !== undefined,
      () => `No routed outcome for ${describe}; sent ${json(outcomes)}`,
    );
    return latest()!;
  };

  /** Panels whose named controls can hold focus. */
  const focusPanels = [...panels.values()].filter((panel) =>
    [...named.values()].some((item) => item.panel === panel),
  );
  /** Reads that found several Worlds focused, for the evidence. */
  const straddles: unknown[] = [];

  /**
   * The named control the presented Worlds focus, with its ring and part, or
   * null. Every panel is read at once, so the reads are normally answered
   * at one boundary. Two Worlds can still report focus together: reads
   * answered on either side of the frame that moves focus between Worlds,
   * or a client's focus in one World before the context adopts it and
   * blurs the other at its next routing boundary. Such a read is taken
   * again after a presented frame, and fails only when several Worlds still
   * hold focus after `FOCUS_REREADS` frames.
   */
  const focus = async (): Promise<FocusRead | null> => {
    for (let attempt = 0; ; attempt++) {
      const reads = await Promise.all(
        focusPanels.map(async (panel) => ({
          panel,
          page: await panel.client.inspectPage({ collection: "guiFocus" }),
        })),
      );
      const focused = reads.flatMap(({ panel, page }) =>
        (page.guiFocus ?? []).map((record) => {
          const name = [...named.values()].find(
            (item) =>
              item.panel === panel && item.entity === record.target.entity,
          )?.name;
          check(name, `Focus names an unknown control ${json(record)}`);
          return {
            name,
            visible: record.visible,
            part: record.part,
            panel: panel.name,
            tick: page.tick,
          };
        }),
      );
      if (focused.length <= 1) {
        const [only] = focused;
        return only
          ? { name: only.name, visible: only.visible, part: only.part }
          : null;
      }
      straddles.push({ attempt, focused });
      check(
        attempt < FOCUS_REREADS,
        `Several Worlds hold focus over ${FOCUS_REREADS} frames: ${json(focused)}`,
      );
      await host.presentation.frame(view);
    }
  };

  /**
   * Wait until `entity` is the context's native text target and its native
   * buffer holds DOM focus, so typed text reaches it; returns the text.
   */
  const expectNativeBuffer = async (entity: bigint) => {
    await until(
      () =>
        input.nativeText?.fence.target.entity === entity &&
        document.activeElement?.hasAttribute("data-ipp-native-text") === true,
      () =>
        `Native text ${json(input.nativeText)} and DOM focus ${document.activeElement?.tagName}, expected entity ${entity}`,
    );
    return input.nativeText!.text;
  };

  /** A client's `GuiAction` and the routing boundary that follows it. */
  const clientAction = async (
    name: string,
    kind: "focus" | "blur",
    part?: number,
  ) => {
    const { panel, entity } = entry(name);
    const found = await readControl(panel.client, entity);
    check(found, `Control ${name} is missing`);
    const target = await controlTarget(
      panel.client,
      entity,
      panel.client.components[found.component]!.id,
    );
    check(target, `Control ${name} has no target`);
    accepted(
      await guiAction(
        panel.client,
        target,
        kind === "focus" && part !== undefined ? { kind, part } : { kind },
      ),
      `Client ${kind} of ${name}`,
    );
    await host.presentation.frame(view);
  };

  /**
   * Open or close an overlay by writing its `visible` field, as a kit does,
   * and wait for the frame that applies it.
   */
  const setOverlay = async (name: string, open: boolean) => {
    const { panel, entity } = entry(name);
    const behavior = panel.client.components.GuiBehavior!;
    successfulBatch(
      await panel.client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: entity },
          component: behavior.id,
          field: {
            offset: behavior.fields.visible!.offset,
            value: { kind: "bool", value: open },
          },
        },
      ]),
    );
    await host.presentation.frame(view);
  };

  /** Lifecycle notifications of watched overlays' `GuiBehavior`. */
  const overlayChanges: { name: string; change: string }[] = [];
  const subscriptions: { unsubscribe(): Promise<void> }[] = [];
  const watches: { remove(): Promise<unknown> }[] = [];

  /**
   * A lifecycle value watch of fields of `component` on a named entity, as
   * React's value callbacks read them: the current values first, then one
   * record, with its tick, whenever a frame ends with them changed.
   */
  const watchValuesOn = async (
    panel: PagePanel,
    entity: bigint,
    component: string,
    fields: readonly string[],
  ) => {
    const descriptor = panel.client.components[component]!;
    const offsets = fields.map((field) => descriptor.fields[field]!.offset);
    const records: { tick: bigint; values: unknown[] }[] = [];
    watches.push(
      await panel.client.watchLifecycle(
        [
          {
            target: {
              kind: "value",
              entity,
              component: descriptor.id,
              fields: offsets,
            },
            kinds: 128,
          },
        ],
        (event) => {
          if (event.kind !== "value" || !event.values) return;
          const values = event.values;
          records.push({
            tick: event.tick,
            values: offsets.map(
              (offset) =>
                values.find((field) => field.offset === offset)?.value,
            ),
          });
        },
      ),
    );
    return records;
  };
  const watchValues = (
    name: string,
    component: string,
    fields: readonly string[],
  ) => {
    const { panel, entity } = entry(name);
    return watchValuesOn(panel, entity, component, fields);
  };

  /** The hint whose opening or closing `hintTiming` measures. */
  let hint:
    | {
        panel: PagePanel;
        parent: bigint;
        records: { tick: bigint; values: unknown[] }[];
        from: number;
        cut: Cut;
      }
    | undefined;

  /** Release the input relay, the observations and every World. */
  const close = async () => {
    window.removeEventListener("contextmenu", menus);
    detach();
    await input.close();
    for (const subscription of subscriptions) await subscription.unsubscribe();
    for (const watch of watches) await watch.remove();
    for (const panel of panels.values()) await panel.log.close();
    await host.presentation.clear(view);
    for (const panel of [...panels.values()].reverse()) {
      await panel.build.close?.();
      panel.clock.stop();
      await panel.client.close();
      await host.destroyWorld(panel.world);
    }
    await host.destroyWorld(parent);
  };

  const steps = {
    /**
     * A point at a fraction of a named control's evaluated box: in its
     * panel's canvas, and in the parent canvas, whose CSS pixels it is.
     */
    async point(name: string, fraction: readonly [number, number]) {
      const { panel } = entry(name);
      const [x, y, width, height] = (await read(name)).bounds;
      const local = [
        x + width * fraction[0],
        y + height * fraction[1],
      ] as const;
      return {
        panel: local,
        canvas: [
          panel.origin[0] + local[0],
          panel.origin[1] + local[1],
        ] as const,
      };
    },
    /** A logical point of a panel's canvas in the parent canvas. */
    panelPoint(name: string, point: readonly [number, number]) {
      const { origin } = panelNamed(name);
      return [origin[0] + point[0], origin[1] + point[1]] as const;
    },
    /** Evaluated logical box of a control in its panel's canvas. */
    async bounds(name: string) {
      return (await read(name)).bounds;
    },
    cut,
    focus,
    /** Reads across Worlds that had to be taken again, for the evidence. */
    focusStraddles() {
      return [...straddles];
    },
    /** A client focuses the control, or the named part of it. */
    async clientFocus(name: string, part?: number) {
      await clientAction(name, "focus", part);
    },
    async clientBlur(name: string) {
      await clientAction(name, "blur");
    },
    /**
     * Wait for the named focus and ring, and its part when given, or for no
     * focus.
     */
    async expectFocus(name: string | null, visible?: boolean, part?: number) {
      let last: FocusRead | null = null;
      await until(
        async () => {
          last = await focus();
          return name === null
            ? last === null
            : last?.name === name &&
                (visible === undefined || last.visible === visible) &&
                (part === undefined || last.part === part);
        },
        () => `Focus ${json(last)}, expected ${name}/${visible}/${part}`,
      );
      return last;
    },
    /**
     * Wait until the named text input is the context's native text target
     * with `text` and its native buffer holds DOM focus.
     */
    async expectNativeText(text: string, name: string) {
      const { entity } = entry(name);
      await until(
        async () =>
          input.nativeText?.fence.target.entity === entity &&
          input.nativeText.text === text &&
          document.activeElement?.hasAttribute("data-ipp-native-text") === true,
        () =>
          `Native text ${json(input.nativeText)} and DOM focus ${document.activeElement?.tagName}, expected ${text}`,
      );
      const field = (await read(name)).value;
      check(
        field.kind === "text" && field.value === text,
        `Committed text ${json(field)}, expected ${text}`,
      );
      return input.nativeText;
    },
    /**
     * Wait until the named input is the context's native text target with
     * `text` being edited, which no field holds.
     */
    async expectEdit(name: string, text: string) {
      const { entity } = entry(name);
      await until(
        () =>
          input.nativeText?.fence.target.entity === entity &&
          input.nativeText.text === text,
        () => `Edit of ${name} ${json(input.nativeText)}, expected ${text}`,
      );
      return input.nativeText!.text;
    },
    /**
     * Wait until the named input takes typed text: the context's native text
     * target, its native buffer holding DOM focus.
     */
    async expectTyping(name: string) {
      return expectNativeBuffer(entry(name).entity);
    },
    /** Wait until no native text remains and the canvas holds DOM focus. */
    async expectNoNativeText() {
      await until(
        () => input.nativeText === null && document.activeElement === canvas,
        () =>
          `Native text ${json(input.nativeText)} and DOM focus ${document.activeElement?.tagName} remain`,
      );
    },
    /** Wait for exactly `count` effects of `kind` on the control since `from`. */
    async expectEffects(
      name: string,
      kind: GuiObservedEffect["effect"]["kind"],
      from: Cut,
      count = 1,
    ) {
      await until(
        () => effects(name, kind, from).length >= count,
        () =>
          `Expected ${count} ${kind} on ${name}, got ${json(effects(name, kind, from))}`,
      );
      const found = effects(name, kind, from);
      check(
        found.length === count,
        `Too many ${kind} on ${name}: ${json(found)}`,
      );
      for (const effect of found)
        check(
          typeof effect.source === "object",
          `${kind} on ${name} lost its routed source`,
        );
      return found;
    },
    /**
     * Frames until every input sent so far settled, then check that no effect
     * of `kind` reached any control since `from`.
     */
    async expectNoEffects(
      kind: GuiObservedEffect["effect"]["kind"],
      from: Cut,
    ) {
      await host.presentation.frame(view);
      await host.presentation.frame(view);
      const found = [...panels.values()].flatMap((panel) =>
        panel.log.effects
          .slice(from.effects[panel.name] ?? 0)
          .filter((effect) => effect.effect.kind === kind),
      );
      check(found.length === 0, `Unexpected ${kind}: ${json(found)}`);
    },
    /** Frames until settled, then the effects of `kind` on a control since `from`. */
    async settledEffects(
      name: string,
      kind: GuiObservedEffect["effect"]["kind"],
      from: Cut,
    ) {
      await host.presentation.frame(view);
      await host.presentation.frame(view);
      return effects(name, kind, from);
    },
    /**
     * Wait until the named control's focus feedback since `from`, whoever
     * moved focus, ends with `focused`; returns the values reported.
     */
    async expectFocusFeedback(name: string, from: Cut, focused: boolean) {
      const reported = () =>
        effects(name, "focusChanged", from).flatMap((effect) =>
          effect.effect.kind === "focusChanged" ? [effect.effect.focused] : [],
        );
      await until(
        () => reported().at(-1) === focused,
        () =>
          `Focus feedback of ${name} ${json(reported())}, expected it to end ${focused}`,
      );
      return reported();
    },
    /**
     * The named control's focus feedback since `from`: whether it held
     * focus, with the focus part it reported.
     */
    focusFeedback(name: string, from: Cut) {
      return effects(name, "focusChanged", from).flatMap((effect) =>
        effect.effect.kind === "focusChanged"
          ? [[effect.effect.focused, effect.effect.part] as const]
          : [],
      );
    },
    /** The routing outcome of the latest press of `key` after `from`. */
    keyOutcome(key: GuiPhysicalKey, from: Cut) {
      return outcome(
        (item) => item.kind === "key" && item.key === key,
        from,
        `key ${key}`,
      );
    },
    /** The routing outcome of the latest wheel after `from`. */
    wheelOutcome(from: Cut) {
      return outcome((item) => item.kind === "wheel", from, "wheel");
    },
    /** The routing outcome of the latest primary press after `from`. */
    pressOutcome(from: Cut) {
      return outcome(
        (item) =>
          item.kind === "pointerDown" &&
          (item.button === undefined || item.button === "primary"),
        from,
        "primary press",
      );
    },
    /** The routing outcome of the latest secondary press after `from`. */
    secondaryOutcome(from: Cut) {
      return outcome(
        (item) => item.kind === "pointerDown" && item.button === "secondary",
        from,
        "secondary press",
      );
    },
    /** Inputs the adapter reported unhandled to its client since `from`. */
    unhandledSince(from: Cut) {
      return unhandled.slice(from.unhandled);
    },
    /** Whether the browser suppressed its own menu for every context event. */
    browserMenus() {
      return [...browserMenus];
    },
    /** A slider's or numeric input's committed value. */
    async scalar(name: string) {
      const field = (await read(name)).value;
      check(field.kind === "scalar", `${name} holds no number`);
      return field.value;
    },
    /** Frames until a slider's or numeric input's value is `value`. */
    async expectScalar(name: string, value: number) {
      let last: GuiControlValue | undefined;
      await until(
        async () => {
          last = (await read(name)).value;
          return last.kind === "scalar" && last.value === value;
        },
        () => `${name} holds ${json(last)}, expected ${value}`,
      );
      return value;
    },
    /** A control's component fields. */
    async fields(name: string) {
      return (await read(name)).fields;
    },
    /** The named text input's committed text. */
    async textValue(name: string) {
      const field = (await read(name)).value;
      check(field.kind === "text", `The text input ${name} lost its text`);
      return field.value;
    },
    /** A scroll view's committed vertical offset. */
    async scrollOffset(name: string) {
      const field = (await read(name)).value;
      check(field.kind === "scroll", `${name} is not a scroll view`);
      return field.offset[1];
    },
    /** Frames until a scroll view's vertical offset differs from `from`. */
    async expectScrollMoved(name: string, from: number) {
      let last: GuiControlValue | undefined;
      await until(
        async () => {
          last = (await read(name)).value;
          return last.kind === "scroll" && last.offset[1] !== from;
        },
        () => `${name} offset ${json(last)}, expected it to leave ${from}`,
      );
      return (last as { offset: readonly [number, number] }).offset[1];
    },
    /** Frames until a scroll view's vertical offset exceeds `from`. */
    async expectScrolledPast(name: string, from: number) {
      let last: GuiControlValue | undefined;
      await until(
        async () => {
          last = (await read(name)).value;
          return last.kind === "scroll" && last.offset[1] > from;
        },
        () => `${name} offset ${json(last)}, expected past ${from}`,
      );
      return (last as { offset: readonly [number, number] }).offset[1];
    },
    /**
     * Wait until exactly `expected` among the named buttons are selected;
     * returns the selection read.
     */
    async expectSelected(
      names: readonly string[],
      expected: readonly string[],
    ) {
      let last: string[] = [];
      await until(
        async () => {
          const reads = await Promise.all(names.map((name) => read(name)));
          last = names.filter(
            (_, index) => reads[index]!.fields.selected === true,
          );
          return json(last) === json(expected);
        },
        () => `Selected ${json(last)}, expected ${json(expected)}`,
      );
      return last;
    },
    /**
     * Wait until the active item of `panel`, by default the named control's,
     * is the named control, or none; returns the `guiActiveItems` records.
     */
    async expectActive(name: string | null, panel?: string) {
      const owner = panelNamed(panel ?? entry(name!).panel.name);
      let records: unknown[] = [];
      await until(
        async () => {
          const page = await owner.client.inspectPage({
            collection: "guiActiveItems",
          });
          records = [...(page.guiActiveItems ?? [])];
          const active = (page.guiActiveItems ?? []).map(
            (record) =>
              [...named.values()].find(
                (item) =>
                  item.panel === owner && item.entity === record.target.entity,
              )?.name,
          );
          return name === null
            ? active.length === 0
            : active.length === 1 && active[0] === name;
        },
        () => `Active items ${json(records)}, expected ${name}`,
      );
      return records;
    },
    setOverlay,
    /** Wait until an overlay is open or closed; the runtime may write it. */
    async expectOverlay(name: string, open: boolean) {
      const { panel, entity } = entry(name);
      let last: boolean | undefined;
      await until(
        async () =>
          (last = await overlayVisible(panel.client, entity)) === open,
        () => `Overlay ${name} open ${last}, expected ${open}`,
      );
    },
    /**
     * Subscribe to an overlay's `GuiBehavior` changes, as a client following
     * the runtime's writes of its `visible` field does.
     */
    async watchOverlay(name: string) {
      const { panel, entity } = entry(name);
      subscriptions.push(
        await panel.client.subscribeLifecycle(
          {
            entities: false,
            components: true,
            assets: false,
            entity,
            component: panel.client.components.GuiBehavior!.id,
          },
          (event) => {
            if (event.observation.kind === "component")
              overlayChanges.push({ name, change: event.observation.change });
          },
        ),
      );
    },
    /**
     * Wait until the watched overlay's `GuiBehavior` was observed updated at
     * least `count` times; returns the changes observed.
     */
    async expectOverlayChanges(name: string, count: number) {
      const changes = () =>
        overlayChanges
          .filter((item) => item.name === name)
          .map((item) => item.change);
      await until(
        () =>
          changes().filter((change) => change === "updated").length >= count,
        () =>
          `Overlay ${name} changes ${json(changes())}, expected ${count} updates`,
      );
      return changes();
    },
    /**
     * Before the pointer moves onto (or off) a hint's parent: watch the
     * hint's `visible` field and mark the parent's pointer feedback, so
     * `hintTiming` reads both ticks.
     */
    async prepareHint(parent: string, overlay: string) {
      const target = entry(parent);
      await prepareHintOn(target.panel, target.entity, entry(overlay).entity);
    },
    /**
     * Frames until the prepared hint is `open`, then the Host-clock interval
     * from the tick its parent's hover became `open` to the tick of the
     * frame that ended with the hint so: its least and most possible length.
     */
    async hintTiming(open: boolean) {
      return measureHint(open);
    },
    /** Every effect since `from` on a panel, for diagnostics. */
    panelEffects(name: string, from: Cut) {
      const panel = panelNamed(name);
      return {
        effects: panel.log.effects
          .slice(from.effects[name] ?? 0)
          .map((effect) => [effect.target.entity, effect.effect.kind]),
        outcomes: outcomes
          .slice(from.outcomes)
          .map(({ input: event, outcome: result }) => [
            event.kind,
            result.disposition,
          ]),
      };
    },
    /** The presented canvas, as base64 RGBA rows. */
    async capture() {
      const capture = await host.presentation.capture(view);
      const bytes = new Uint8Array(capture.pixels);
      let binary = "";
      for (let index = 0; index < bytes.length; index += 0x8000)
        binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
      return { ...extent, rgba: btoa(binary) };
    },
    close,
  };

  /** See `prepareHint`, for a hint and parent of `panel` by entity. */
  const prepareHintOn = async (
    panel: PagePanel,
    parent: bigint,
    overlay: bigint,
  ) => {
    const records = await watchValuesOn(panel, overlay, "GuiBehavior", [
      "visible",
    ]);
    await panel.clock.sample();
    hint = { panel, parent, records, from: records.length, cut: cut() };
  };

  /** See `hintTiming`. */
  const measureHint = async (open: boolean) => {
    check(hint, "No hint prepared");
    const { panel, parent, records, from } = hint;
    const reached = () =>
      records.slice(from).find((record) => record.values[0] === open);
    // Sample the clock while waiting, so the ticks around the change have
    // Host times of their own.
    await until(
      async () => {
        await panel.clock.sample();
        return reached() !== undefined;
      },
      () => `The hint did not become ${open ? "open" : "closed"}`,
    );
    const hover = effectsOn(panel, parent, "interactionChanged", hint.cut).find(
      (effect) =>
        effect.effect.kind === "interactionChanged" &&
        effect.effect.state.hovered === open,
    );
    check(
      hover,
      `No hover feedback ${open} on the hint's parent: ${json(
        effectsOn(panel, parent, "interactionChanged", hint.cut),
      )}`,
    );
    const shown = reached()!;
    return {
      hover: hover.tick,
      hint: shown.tick,
      ...(await hostInterval(panel.clock, hover.tick, shown.tick)),
    };
  };

  return {
    steps,
    host,
    view,
    input,
    expectNativeBuffer,
    panel: panelNamed,
    entry,
    read,
    until,
    cut,
    effects,
    effectsOn,
    watchValues,
    watchValuesOn,
    prepareHintOn,
    report,
  };
}

export type CompositePage = Awaited<ReturnType<typeof openCompositePage>>;
export type CompositeCut = ReturnType<CompositePage["cut"]>;
