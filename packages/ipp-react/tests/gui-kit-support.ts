/**
 * Shared harness of the GUI kit declaration tests (`gui-kit-*.test.ts`): a
 * recording World that acknowledges every batch at once, the tokens, parts
 * and states its rows are checked against, `render`, and the row lookups and
 * comparisons more than one family uses.
 */
import assert from "node:assert/strict";
import { setImmediate as turn } from "node:timers/promises";
import { createElement as h, type ReactNode } from "react";
import {
  FieldKind,
  type AnimationControllerDescription,
  type AnimationPlaybackEvent,
  type AnimationWorldClient,
  type AssetResourceSnapshot,
  type AssetWorldClient,
  type BatchOutcome,
  type ClientAssetSource,
  type Command,
  type EntityRef,
  type FieldValue,
  type GuiEffectSubscription,
  type GuiObservedEffect,
  type GuiSubscriptionCut,
  type LifecycleTargetSelection,
  type LifecycleTargetWatch,
  type LifecycleWatchEvent,
  type SystemCommand,
} from "@ipp/client";
import { Children, Entity, createRoot } from "../src/index.js";
import type { ReactWorldClient } from "../src/reconciler/world-client.js";
import { guiComponentContract } from "../src/gui/fields.js";
import {
  GuiKit,
  type GuiKitContract,
  type GuiKitTokens,
} from "../src/gui-kit.js";

type Color = readonly [number, number, number, number];

export const TOKENS = {
  em: 16,
  page: [0.1, 0.1, 0.1, 1],
  surface: [0.2, 0.2, 0.2, 1],
  accent: [0, 0.9, 0.95, 1],
  text: [0.8, 0.9, 0.9, 1],
  neutral: [0.3, 0.4, 0.5, 1],
  line: [0.05, 0.1, 0.15, 1],
  amber: [0.95, 0.7, 0.15, 1],
  error: [0.9, 0.05, 0.35, 1],
  rowTint: [0, 0.9, 0.95, 0.04],
  lineWidth: 1.25,
  litLineWidth: 1.5,
  cut: 8,
  partCut: 4,
  cornerAccent: 8,
  cornerAccentWidth: 2.5,
  focusGlowIntensity: 0.04,
  glowFalloff: 2.5,
  frameGlowReach: 16,
  controlHeight: 40,
  smallHeight: 32,
  dial: 80,
  dockedHeight: 24,
  dockedWidth: 32,
  bar: 8,
  inset: 16,
  row: 36,
  denseRow: 24,
  selectionGutter: 4,
  textSmall: 13,
  textBody: 16,
  textDisplay: 24,
  icon: 24,
} as const satisfies GuiKitTokens;

/**
 * Paint key indices of the test contract: a part's base row, then its states
 * and checked variants at fixed distances.
 */
export const PART = { background: 0, icon: 5, label: 6, focusRing: 7 } as const;
export const STATE = {
  idle: 0,
  pressed: 100,
  hovered: 200,
  disabled: 300,
} as const;
export const CHECKED = 1000;
export const PRESSED = STATE.pressed;
export const HOVERED = STATE.hovered;

export const CHECK_MARK = {
  part: PART.icon,
  color: TOKENS.surface,
  shape: 1,
  border_width: 4,
  stroke_a: [0, 0.5, 0.4, 0.9],
  stroke_b: [0.2, 0.9, 1, 0.2],
};

/**
 * Fields the runtime writes on a VirtualList, which its range callback
 * watches; the authoring contract lists only authored fields.
 */
const VIRTUAL_LIST_STATE = Object.fromEntries(
  [
    "offset_x",
    "offset_y",
    "anchor_index",
    "anchor_offset",
    "viewport_x",
    "viewport_y",
    "content_x",
    "content_y",
    "capacity_x",
    "capacity_y",
    "range_first",
    "range_last",
  ].map((field) => [field, "number"]),
);

/** Fields the runtime writes on a ScrollView, which its callbacks watch. */
const SCROLL_VIEW_STATE = Object.fromEntries(
  [
    "offset_x",
    "offset_y",
    "viewport_x",
    "viewport_y",
    "content_x",
    "content_y",
    "capacity_x",
    "capacity_y",
  ].map((field) => [field, "number"]),
);

/** Component descriptors: every GUI component, fields eight bytes apart. */
export const DESCRIPTORS = Object.fromEntries(
  Object.entries(guiComponentContract).map(([name, definition], index) => [
    name,
    {
      id: index + 1,
      fields: Object.fromEntries(
        Object.entries({
          ...definition.fields,
          ...(name === "GuiVirtualList" ? VIRTUAL_LIST_STATE : {}),
          ...(name === "GuiScrollView" ? SCROLL_VIEW_STATE : {}),
        }).map(([field, type], slot) => [
          field,
          {
            offset: slot * 8,
            kind: {
              number: FieldKind.F32,
              string: FieldKind.String,
              boolean: FieldKind.Bool,
              bytes: FieldKind.Rows,
              entity: FieldKind.Entity,
            }[type as string]!,
          },
        ]),
      ),
    },
  ]),
) as ReactWorldClient["components"];

/** A generated contract stand-in whose theme encoder writes JSON rows. */
export function contract(): GuiKitContract {
  const layout = DESCRIPTORS.GuiLayout!;
  return {
    GUI_SKIN_TOKENS: TOKENS,
    GUI_SKIN_LOOKS: {
      button: { em: 16, parts: [] },
      checkbox: { em: 16, parts: [CHECK_MARK] },
      amber: {
        em: 16,
        parts: [
          {
            part: PART.background,
            corner_cut: [8, 0, 8, 0],
            border_color: TOKENS.amber,
          },
        ],
      },
      secondary: {
        em: 16,
        parts: [{ part: PART.background, corner_cut: [4, 0, 4, 0] }],
      },
      secondaryAmber: {
        em: 16,
        parts: [
          {
            part: PART.background,
            corner_cut: [4, 0, 4, 0],
            border_color: TOKENS.amber,
          },
        ],
      },
      docked: {
        em: 16,
        parts: [
          {
            part: PART.background,
            color: TOKENS.surface,
            border_width: 1.25,
            corner_cut: [0, 0, 0, 0],
          },
          { part: PRESSED, color: TOKENS.accent },
        ],
      },
      textInput: {
        em: 16,
        parts: [{ part: PART.background, corner_cut: [8, 0, 8, 0] }],
      },
      scroll: {
        em: 16,
        parts: [
          { part: PART.background, border_width: 1.25, color: TOKENS.surface },
        ],
      },
    },
    guiPaintPartIndex: ({ part, state, variant }) =>
      PART[part as keyof typeof PART] +
      (state ? STATE[state as keyof typeof STATE] : 0) +
      (variant === "checked" ? CHECKED : 0),
    GuiTheme: {
      encodeParts: ({ nextSlot, rows }) =>
        new TextEncoder().encode(
          JSON.stringify({ nextSlot, rows: [...rows] }),
        ) as Uint8Array<ArrayBuffer>,
    },
    components: {
      GuiLayout: {
        id: layout.id,
        fields: { align_x: { offset: layout.fields.align_x!.offset } },
      },
      CanvasStyle: {
        id: DESCRIPTORS.CanvasStyle!.id,
        fields: {
          opacity: { offset: DESCRIPTORS.CanvasStyle!.fields.opacity!.offset },
        },
      },
    },
    GuiSkin: {
      id: DESCRIPTORS.GuiSkin!.id,
      partsOffset: rowOffset,
      encodeParts: ({ nextSlot, rows }) =>
        new TextEncoder().encode(
          JSON.stringify({ nextSlot, rows: [...rows] }),
        ) as Uint8Array<ArrayBuffer>,
    },
  };
}

/** Row property offsets of the test contract: 32 properties a slot. */
export function rowOffset(slot: number, property: string): number {
  const index = ["border_width", "arc_start", "arc_sweep", "opacity"].indexOf(
    property,
  );
  assert.ok(index >= 0, `unexpected row property ${property}`);
  return 0x1000_0000 + slot * 32 + index;
}

interface Declared {
  /** Component fields by component and field name. */
  readonly components: Map<string, Map<string, unknown>>;
  parent: string | null;
}

/**
 * A World that acknowledges every batch at once and keeps what it holds by
 * symbolic id, with component and field names, links to parents, registered
 * assets and animation controllers.
 */
export class KitWorld
  implements
    ReactWorldClient,
    Pick<
      AssetWorldClient,
      "registerAsset" | "releaseAsset" | "onResourceChange"
    >,
    Pick<
      AnimationWorldClient,
      | "encodeAnimationClip"
      | "createAnimationController"
      | "updateAnimationController"
      | "deleteAnimationController"
      | "controlAnimationController"
      | "onPlaybackEvent"
    >
{
  session = 1n;
  schemaHash = 1n;
  worldReference = { id: 1n, incarnation: 1n };
  components = DESCRIPTORS;
  readonly entities = new Map<string, Declared>();
  readonly controllers = new Map<bigint, AnimationControllerDescription>();
  readonly clips = new Map<string, unknown>();
  private readonly ids = new Map<bigint, string>();
  private next = 100n;
  private readonly resources = new Set<
    (resource: AssetResourceSnapshot) => void
  >();
  private readonly names = new Map(
    Object.entries(DESCRIPTORS).map(([name, descriptor]) => [
      descriptor.id,
      {
        name,
        fields: new Map(
          Object.entries(descriptor.fields).map(([field, { offset }]) => [
            offset,
            field,
          ]),
        ),
      },
    ]),
  );

  async batch(operations: Command[]): Promise<BatchOutcome> {
    const aliases = new Map<number, bigint>();
    const outcome = {
      ok: true as const,
      batchId: 1n,
      tick: 1n,
      aliases: [] as { alias: number; id: bigint }[],
      symbols: [] as { symbol: string; id: bigint }[],
      effects: [] as BatchOutcome["effects"],
    };
    const resolve = (reference: EntityRef): string => {
      if (reference.kind === "alias")
        return this.ids.get(aliases.get(reference.alias)!)!;
      if (reference.kind === "symbol") {
        const id = [...this.ids].find(
          ([, symbol]) => symbol === reference.symbol,
        )?.[0];
        assert.ok(id !== undefined, `missing ${reference.symbol}`);
        outcome.symbols.push({ symbol: reference.symbol, id });
        return reference.symbol;
      }
      return this.ids.get(reference.id)!;
    };
    const value = (field: FieldValue): unknown =>
      field.kind === "entity"
        ? resolve(field.value)
        : field.kind === "unset"
          ? undefined
          : field.value;
    for (const operation of operations) {
      switch (operation.kind) {
        case "create": {
          const id = this.next++;
          const symbol = operation.metadata.symbolicId!;
          this.ids.set(id, symbol);
          aliases.set(operation.alias, id);
          outcome.aliases.push({ alias: operation.alias, id });
          this.entities.set(symbol, { components: new Map(), parent: null });
          break;
        }
        case "insertComponent": {
          const { name, fields } = this.names.get(operation.component)!;
          const declared = this.entities.get(resolve(operation.entity))!;
          const written = operation.fields.map(
            (field) => [fields.get(field.offset)!, value(field.value)] as const,
          );
          // Adoption writes the listed fields over a present component.
          const present = operation.adopt && declared.components.get(name);
          if (present)
            for (const [field, to] of written) present.set(field, to);
          else declared.components.set(name, new Map(written));
          break;
        }
        case "setField": {
          const { name, fields } = this.names.get(operation.component)!;
          this.entities
            .get(resolve(operation.entity))!
            .components.get(name)!
            .set(
              fields.get(operation.field.offset)!,
              value(operation.field.value),
            );
          break;
        }
        case "removeComponent":
          this.entities
            .get(resolve(operation.entity))!
            .components.delete(this.names.get(operation.component)!.name);
          break;
        case "delete": {
          const symbol = resolve(operation.entity);
          this.entities.delete(symbol);
          for (const [id, name] of this.ids)
            if (name === symbol) this.ids.delete(id);
          break;
        }
        case "placeEntity": {
          const symbol = resolve(operation.entity);
          const parent =
            operation.placement.parent && resolve(operation.placement.parent);
          const before =
            operation.placement.before && resolve(operation.placement.before);
          this.entities.get(symbol)!.parent = parent;
          // Link order: before its anchor, or last.
          this.order = this.order.filter((name) => name !== symbol);
          const at = before ? this.order.indexOf(before) : -1;
          this.order.splice(at < 0 ? this.order.length : at, 0, symbol);
          break;
        }
        case "guiAction":
          this.actions.push([resolve(operation.entity), operation.action]);
          break;
        case "setFieldIf": {
          // Control handles' compare-and-set: recorded, and applied when the
          // field holds the expected value.
          const { name, fields } = this.names.get(operation.component)!;
          const symbol = resolve(operation.entity);
          const field = fields.get(operation.field.offset)!;
          const current = this.entities.get(symbol)!.components.get(name)!;
          this.compared.push([symbol, field, value(operation.field.value)]);
          if (current.get(field) === value(operation.expected))
            current.set(field, value(operation.field.value));
          break;
        }
        default:
          assert.fail(`Unexpected ${operation.kind}`);
      }
    }
    return outcome;
  }

  async registerAsset(resource: ClientAssetSource): Promise<void> {
    setImmediate(() => {
      for (const listener of this.resources)
        listener({
          ...resource,
          id: 1n,
          status: "loaded",
        } as unknown as AssetResourceSnapshot);
    });
  }

  async releaseAsset(): Promise<void> {}

  onResourceChange(listener: (resource: AssetResourceSnapshot) => void) {
    this.resources.add(listener);
    return () => {
      this.resources.delete(listener);
    };
  }

  encodeAnimationClip(clip: object): Uint8Array<ArrayBuffer> {
    const bytes = new TextEncoder().encode(JSON.stringify(clip));
    this.clips.set(new TextDecoder().decode(bytes), clip);
    return bytes as Uint8Array<ArrayBuffer>;
  }

  async createAnimationController(
    description: AnimationControllerDescription,
  ): Promise<bigint> {
    const id = this.next++;
    this.controllers.set(id, description);
    return id;
  }

  async updateAnimationController(): Promise<void> {}

  async deleteAnimationController(id: bigint): Promise<void> {
    this.controllers.delete(id);
  }

  readonly controls: unknown[] = [];
  /** Playback controls by controller, in order. */
  readonly controlled: [bigint, unknown][] = [];

  async controlAnimationController(
    id: bigint,
    control: unknown,
  ): Promise<void> {
    this.controls.push(control);
    this.controlled.push([id, control]);
  }

  private readonly playback = new Set<
    (event: AnimationPlaybackEvent) => void
  >();

  onPlaybackEvent(listener: (event: AnimationPlaybackEvent) => void) {
    this.playback.add(listener);
    return () => {
      this.playback.delete(listener);
    };
  }

  /** Report that controller `id` reached the end of its clip. */
  complete(id: bigint): void {
    for (const listener of this.playback)
      listener({
        controller: { id },
        kind: "completed",
        reason: null,
      } as unknown as AnimationPlaybackEvent);
  }

  /** Semantic actions applied, with their target's symbolic id. */
  readonly actions: [string, unknown][] = [];

  /** System commands sent, in order. */
  readonly commands: SystemCommand[] = [];

  sendCommand(command: SystemCommand): void {
    this.commands.push(command);
  }

  /** The symbolic id of the entity with handle `id`. */
  symbol(id: bigint): string | undefined {
    return this.ids.get(id);
  }

  private readonly observers: {
    listener: (effect: GuiObservedEffect) => void;
    classes: string;
  }[] = [];

  /** Observation channels by class; `effect` delivers to them. */
  async subscribeGuiEffects(
    listener: (effect: GuiObservedEffect) => void,
    options?: { classes?: string },
  ): Promise<GuiEffectSubscription> {
    this.observers.push({
      listener,
      classes: options?.classes ?? "application",
    });
    const cut = { tick: 0n } as unknown as GuiSubscriptionCut;
    return {
      id: 1n as unknown as GuiEffectSubscription["id"],
      world: this.worldReference,
      start: cut,
      closed: new Promise(() => {}),
      unsubscribe: async () => cut,
    };
  }

  /**
   * Deliver `effect` of the control on `symbol` to the observers of its
   * class, as published in the frame `tick`.
   */
  effect(
    symbol: string,
    effect: GuiObservedEffect["effect"],
    control: "GuiButton" | "GuiTextInput" = "GuiButton",
    tick = 1n,
  ): void {
    const entity = [...this.ids].find(([, name]) => name === symbol)?.[0];
    assert.ok(entity !== undefined, `no entity ${symbol}`);
    const feedback =
      effect.kind === "focusChanged" || effect.kind === "interactionChanged";
    for (const { listener, classes } of this.observers)
      if ((classes === "feedback") === feedback)
        listener({
          id: { world: this.worldReference, ordinal: tick },
          target: {
            world: this.worldReference,
            entity,
            component: DESCRIPTORS[control]!.id,
            incarnation: 1n,
          },
          source: "semantic",
          tick,
          ancestry: [entity],
          effect,
        });
  }

  /** Control handles read nothing in these tests. */
  async inspectPage(): Promise<never> {
    assert.fail("Unexpected inspection");
  }

  /** Compare-and-set writes by target symbol, field and value. */
  readonly compared: [string, string, unknown][] = [];

  /** Watched value members: the watch's listener, member and target. */
  private readonly watchedValues: {
    listener: (event: LifecycleWatchEvent) => void;
    member: { output: bigint; generation: bigint };
    entity: bigint;
    component: number;
  }[] = [];
  private tick = 10n;

  /**
   * Deliver a value record of the control component `control` on `symbol`,
   * as the runtime writes it: `fields` by name, with a text input's number
   * fields, which the record always carries, at their defaults unless given.
   */
  value(
    symbol: string,
    control:
      | "GuiButton"
      | "GuiScrollView"
      | "GuiBehavior"
      | "GuiTextInput"
      | "GuiSlider"
      | "GuiColor",
    given: Readonly<Record<string, unknown>>,
  ): void {
    const fields =
      control === "GuiTextInput"
        ? { numeric: false, value: 0, ...given }
        : given;
    const entity = [...this.ids].find(([, name]) => name === symbol)?.[0];
    const descriptor = DESCRIPTORS[control]!;
    const watched = this.watchedValues.filter(
      (entry) => entry.entity === entity && entry.component === descriptor.id,
    );
    assert.ok(watched.length > 0, `${symbol} has no watched ${control}`);
    const tick = this.tick++;
    for (const { listener, member } of watched)
      listener({
        kind: "value",
        world: this.worldReference,
        output: member.output,
        member,
        tick,
        values: Object.entries(fields).map(([name, value]) => ({
          offset: descriptor.fields[name]!.offset,
          value: value as boolean | number,
        })),
      } as LifecycleWatchEvent);
  }

  /** Lifecycle tracking of callback targets: every target stays live. */
  async watchLifecycle(
    targets: readonly LifecycleTargetSelection[],
    listener?: (event: LifecycleWatchEvent) => void,
  ): Promise<LifecycleTargetWatch> {
    const generation = this.next++;
    targets.forEach((selection, index) => {
      if (listener && selection.target.kind === "value")
        this.watchedValues.push({
          listener,
          member: { output: 5n, generation: generation * 100n + BigInt(index) },
          entity: selection.target.entity,
          component: selection.target.component,
        });
    });
    return {
      world: this.worldReference,
      baselines: targets.map((selection, index) => ({
        member: {
          output: 5n,
          generation: generation * 100n + BigInt(index),
        },
        target: selection.target,
        lifetime:
          selection.target.kind === "entity"
            ? { kind: "entity", live: true }
            : { kind: "component", entityLive: true, incarnation: 3n },
      })),
      cuts: [],
      closed: new Promise(() => {}),
      removeMembers: async () => [],
      remove: async () => [],
    } as unknown as LifecycleTargetWatch;
  }

  /** The declared entity `symbol`. */
  entity(symbol: string): Declared {
    const declared = this.entities.get(symbol);
    assert.ok(declared, `no entity ${symbol}`);
    return declared;
  }

  /** One component's fields of `symbol`. */
  fields(symbol: string, component: string): Map<string, unknown> {
    const fields = this.entity(symbol).components.get(component);
    assert.ok(fields, `${symbol} has no ${component}`);
    return fields;
  }

  /** The kit theme `symbol` is skinned with, by name. */
  skin(symbol: string): string {
    const theme = this.fields(symbol, "GuiSkin").get("theme") as string;
    return theme.replace("ipp-kit/theme/", "");
  }

  /** The tint of `symbol`'s style. */
  tone(symbol: string): Color {
    const style = this.fields(symbol, "CanvasStyle");
    return ["red", "green", "blue", "alpha"].map((channel) =>
      style.get(channel),
    ) as unknown as Color;
  }

  /** Placed entities in link order among their siblings. */
  private order: string[] = [];

  /** Children of `symbol` in link order. */
  children(symbol: string): string[] {
    return this.order.filter(
      (name) => this.entities.get(name)?.parent === symbol,
    );
  }
}

export const FONT = "memory:font";

/**
 * Render `content` in a kit at `fontSize` and settle its commits; each draw
 * gives the kit `reducedMotion` or omits it.
 */
export async function render(
  content: ReactNode,
  {
    fontSize = 16,
    world = new KitWorld(),
    reducedMotion,
  }: { fontSize?: number; world?: KitWorld; reducedMotion?: boolean } = {},
) {
  const root = createRoot(world);
  const errors: unknown[] = [];
  const draw = async (
    next: ReactNode,
    options: { reducedMotion?: boolean } = {},
  ) => {
    await root.render(
      h(
        GuiKit,
        {
          contract: contract(),
          font: FONT,
          fontSize,
          ...(options.reducedMotion === undefined
            ? {}
            : { reducedMotion: options.reducedMotion }),
        },
        h(Entity, { id: "panel" }, h(Children, null, next)),
      ),
    );
    // Asset readiness and animation binding follow on later turns.
    for (let attempt = 0; attempt < 10; attempt++) await turn();
    await root.flush();
  };
  await draw(content, reducedMotion === undefined ? {} : { reducedMotion });
  return { world, root, draw, errors };
}

export function themeRows(world: KitWorld, name: string) {
  const parts = world.fields(`ipp-kit/theme/${name}`, "GuiTheme").get("parts");
  return JSON.parse(new TextDecoder().decode(parts as Uint8Array)) as {
    nextSlot: number;
    rows: [number, Record<string, unknown>][];
  };
}

/** Let effects, refs and animation controls reach their callbacks. */
export async function settle(root: { flush(): Promise<void> }) {
  for (let attempt = 0; attempt < 10; attempt++) await turn();
  await root.flush();
  for (let attempt = 0; attempt < 10; attempt++) await turn();
}

export const near = (actual: unknown, expected: number) =>
  assert.ok(
    Math.abs((actual as number) - expected) < 1e-9,
    `${String(actual)} is not ${expected}`,
  );

/** An entity's own skin rows: a ring's thickness, start and sweep. */
export function skinRows(world: KitWorld, symbol: string) {
  const parts = world.fields(symbol, "GuiSkin").get("parts");
  return JSON.parse(new TextDecoder().decode(parts as Uint8Array)).rows as [
    number,
    Record<string, unknown>,
  ][];
}

/** A ring's own row: its background, `thickness` thick, through `sweep`. */
export function ring(
  thickness: number,
  sweep: number,
  start = 0,
  lead = false,
) {
  return [
    [
      0,
      {
        part: PART.background,
        border_width: thickness,
        arc_start: start,
        arc_sweep: sweep,
        ...(lead ? { opacity: 0.6 } : {}),
      },
    ],
  ];
}

/** A theme's row at paint key `part`. */
export function themeRow(world: KitWorld, name: string, part: number) {
  return themeRows(world, name).rows.find(([, row]) => row.part === part)?.[1];
}

/** Paint keys of the test contract's checked variants. */
export const checkedKey = (part: number, state: keyof typeof STATE) =>
  part + STATE[state] + CHECKED;
