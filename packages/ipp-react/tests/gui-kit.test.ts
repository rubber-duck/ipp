/**
 * Declarations of the GUI kit: which entities, components, links, themes and
 * animation each composition writes, through the real reconciler against a
 * recording World. Rendered appearance is the skin lab's evidence
 * (`tests/skin-lab/specimens/i0*`, `h02-expander`); these tests pin the
 * structure, the theme references and the token arithmetic.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate as turn } from "node:timers/promises";
import { Fragment, createElement as h, useState, type ReactNode } from "react";
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
import type { ReactWorldClient } from "../src/contract.js";
import { guiComponentContract } from "../src/gui/manifest.js";
import {
  DataGrid,
  EmptyState,
  Expander,
  GUI_KIT_ICONS,
  GuiKit,
  Icon,
  InlineAlert,
  LabelledSeparator,
  Panel,
  PanelFooter,
  PanelHeader,
  ProgressBar,
  Row,
  SecondaryButton,
  Separator,
  StatusBadge,
  TextLine,
  ToastStack,
  WindowControl,
  WindowControls,
  type DataGridColumn,
  type DataGridProps,
  type DataGridRow,
  type ExpanderProps,
  type GuiKitContract,
  type GuiKitTokens,
  type ProgressBarProps,
  type ToastItem,
  type ToastStackProps,
  CircularProgress,
  Spinner,
  type CircularProgressProps,
  RadioGroup,
  SegmentedControl,
  Tabs,
  TreeView,
  type RadioGroupProps,
  type SegmentedControlProps,
  type TabsProps,
  type TreeNode,
  type TreeViewHandle,
  type TreeViewProps,
  Floating,
  GUI_KIT_LAYERS,
  Menu,
  useOverlayOpen,
  type MenuItem,
  type OverlayOpenProps,
  ConfirmationDialog,
  ContextMenu,
  Popover,
  Tooltip,
  useContextMenu,
  type ConfirmationDialogProps,
  Autocomplete,
  Dropdown,
  MultiSelect,
  SearchableDropdown,
  selectionSummary,
  type AutocompleteProps,
  type DropdownProps,
  type MultiSelectProps,
  type SearchableDropdownProps,
  type SelectOption,
  Knob,
  LabelledSlider,
  NumericStepper,
  RangeSlider,
  SliderScale,
  ColorPicker,
  formatHex,
  hsvaToRgba,
  parseHex,
  rgbaToHsva,
  type ColorPickerProps,
  type KnobProps,
  type NumericStepperProps,
  type LabelledSliderProps,
  type RangeSliderProps,
} from "../src/gui-kit.js";
import { Button } from "../src/gui/controls.js";

type Color = readonly [number, number, number, number];

const TOKENS = {
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
const PART = { background: 0, icon: 5, label: 6, focusRing: 7 } as const;
const STATE = { idle: 0, pressed: 100, hovered: 200, disabled: 300 } as const;
const CHECKED = 1000;
const PRESSED = STATE.pressed;
const HOVERED = STATE.hovered;

const CHECK_MARK = {
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
const DESCRIPTORS = Object.fromEntries(
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
function contract(): GuiKitContract {
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
function rowOffset(slot: number, property: string): number {
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
class KitWorld
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

const FONT = "memory:font";

/**
 * Render `content` in a kit at `fontSize` and settle its commits; each draw
 * gives the kit `reducedMotion` or omits it.
 */
async function render(
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

function themeRows(world: KitWorld, name: string) {
  const parts = world.fields(`ipp-kit/theme/${name}`, "GuiTheme").get("parts");
  return JSON.parse(new TextDecoder().decode(parts as Uint8Array)) as {
    nextSlot: number;
    rows: [number, Record<string, unknown>][];
  };
}

test("GuiKit declares the kit's themes once per World as top-level theme entities", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  await root.render(
    h(
      GuiKit,
      { contract: contract(), font: FONT, fontSize: 16 },
      h(
        GuiKit,
        { fontSize: 32 },
        h(
          Entity,
          { id: "panel" },
          h(Children, null, h(Separator, { id: "line" })),
        ),
      ),
    ),
  );
  const themes = [...world.entities.keys()].filter((name) =>
    name.startsWith("ipp-kit/theme/"),
  );
  assert.ok(themes.includes("ipp-kit/theme/container"));
  assert.ok(themes.includes("ipp-kit/theme/alertWarning"));
  // Looks drawn at another type size scale their em with it.
  const ems: Record<string, number> = {
    secondarySmall: TOKENS.textSmall,
    secondarySmallAmber: TOKENS.textSmall,
    dockedIcon: TOKENS.icon,
    dockedIconAmber: TOKENS.icon,
    secondaryIcon: TOKENS.icon,
    secondaryIconAmber: TOKENS.icon,
  };
  for (const theme of themes) {
    assert.equal(world.entity(theme).parent, null);
    assert.equal(
      world.fields(theme, "GuiTheme").get("em"),
      ems[theme.replace("ipp-kit/theme/", "")] ?? TOKENS.em,
      theme,
    );
  }

  // Rows are tokens, in the contract's encoding.
  assert.deepEqual(themeRows(world, "alertWarning").rows, [
    [
      0,
      {
        part: PART.background,
        color: [...TOKENS.amber.slice(0, 3), 0.01],
        border_width: TOKENS.lineWidth,
        border_color: TOKENS.amber,
        corner_cut: [TOKENS.cut, 0, TOKENS.cut, 0],
      },
    ],
  ]);
  // The check mark is the checkbox look's own, drawn as a Background.
  assert.deepEqual(themeRows(world, "check").rows, [
    [0, { ...CHECK_MARK, part: PART.background }],
  ]);
  // A small-label secondary button keeps the look's lengths against body text.
  assert.equal(
    world.fields("ipp-kit/theme/secondarySmall", "GuiTheme").get("em"),
    13,
  );
  assert.deepEqual(themeRows(world, "expanderHeader").rows, [
    [0, { part: PRESSED, color: TOKENS.surface }],
  ]);

  // The nested kit inherits and changes only the size.
  assert.equal(world.fields("line", "GuiLayout").get("height"), 2.5);
  await root.unmount();
});

test("kit components need a GuiKit in their own World", async () => {
  const root = createRoot(new KitWorld(), { onError: () => {} });
  await assert.rejects(
    root.render(
      h(
        Entity,
        { id: "panel" },
        h(Children, null, h(Separator, { id: "line" })),
      ),
    ),
    /enclosing GuiKit/,
  );
  const bare = createRoot(new KitWorld(), { onError: () => {} });
  await assert.rejects(bare.render(h(GuiKit)), /contract, font and fontSize/);
});

/** The GUI preferences update that sets reduced motion to `reducedMotion`. */
const preference = (reducedMotion: boolean): SystemCommand => ({
  type: "GuiPreferencesUpdateCommand",
  reducedMotion,
});

test("the World's GuiKit sends its reduced motion once per change; nested kits send nothing", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  const kit = (reducedMotion?: boolean, content?: ReactNode) =>
    h(
      GuiKit,
      {
        contract: contract(),
        font: FONT,
        fontSize: 16,
        ...(reducedMotion === undefined ? {} : { reducedMotion }),
      },
      h(Entity, { id: "panel" }, h(Children, null, content)),
    );
  const sent: SystemCommand[] = [];

  // Omitted, the kit leaves the World's preference alone.
  await root.render(kit());
  assert.deepEqual(world.commands, sent);
  await root.render(kit(true));
  sent.push(preference(true));
  assert.deepEqual(world.commands, sent);
  await root.render(kit(true, h(Separator, { id: "line" })));
  assert.deepEqual(world.commands, sent);

  // A nested kit changes only the kit animations beneath it.
  await root.render(
    kit(true, h(GuiKit, { reducedMotion: false }, h(Separator, { id: "in" }))),
  );
  await root.render(
    kit(false, h(GuiKit, { reducedMotion: true }, h(Separator, { id: "in" }))),
  );
  sent.push(preference(false));
  assert.deepEqual(world.commands, sent);
  await root.render(kit(false));
  assert.deepEqual(world.commands, sent);

  // Like the themes, a preference the kit turned on goes with the setting or
  // the kit, while unmounting the root leaves both.
  await root.render(kit(true));
  await root.render(kit());
  sent.push(preference(true), preference(false));
  assert.deepEqual(world.commands, sent);
  await root.render(kit(true));
  await root.render(h(Entity, { id: "bare" }));
  sent.push(preference(true), preference(false));
  assert.deepEqual(world.commands, sent);
  assert.equal(world.entities.has("ipp-kit/theme/container"), false);
  await root.render(kit(true));
  sent.push(preference(true));
  await root.unmount();
  assert.deepEqual(world.commands, sent);
  assert.ok(world.entities.has("ipp-kit/theme/container"));
});

test("kit animations follow the nearest GuiKit, so a nested kit holds part of a World still", async () => {
  const spinners = (nested: boolean) => [
    h(Spinner, { key: "running", id: "running", label: "Running" }),
    h(
      GuiKit,
      { key: "nested", reducedMotion: nested },
      h(Spinner, { id: "nested", label: "Nested" }),
    ),
  ];
  const turning = (world: KitWorld) =>
    [...world.controllers.values()].map((controller) =>
      world.symbol(controller.drivers[0]!.target),
    );
  const { world, draw } = await render(spinners(true));
  assert.deepEqual(turning(world), ["running/arc"]);
  assert.deepEqual(skinRows(world, "nested/arc"), ring(3, 0.25));
  assert.deepEqual(world.commands, []);

  // Under the World's setting, only the kit that turns motion back on moves.
  await draw(spinners(false), { reducedMotion: true });
  assert.deepEqual(turning(world), ["nested/arc"]);
  assert.deepEqual(world.commands, [preference(true)]);
  await draw(spinners(true), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(world.commands, [preference(true)]);
});

test("Panel and Separator take the container and line themes", async () => {
  const { world } = await render(
    h(
      Panel,
      { id: "section", layout: { width: 200 } },
      h(Separator, { id: "rule", tone: "rule" }),
      h(Separator, { id: "divider", vertical: true }),
    ),
  );
  assert.equal(world.skin("section"), "container");
  assert.equal(world.fields("section", "GuiLayout").get("kind"), 2);
  assert.equal(world.fields("section", "GuiLayout").get("width"), 200);
  // Its parts lie inside its line.
  assert.equal(world.fields("section", "GuiLayout").get("padding_left"), 1.25);
  assert.equal(world.fields("section", "GuiLayout").get("padding_right"), 1.25);
  assert.equal(world.fields("section", "GuiFont").get("font_size"), 16);
  assert.deepEqual(world.children("section"), ["rule", "divider"]);
  assert.equal(world.skin("rule"), "rule");
  assert.equal(world.fields("rule", "GuiLayout").get("height"), 1.25);
  assert.equal(world.skin("divider"), "division");
  assert.equal(world.fields("divider", "GuiLayout").get("width"), 1.25);
  assert.deepEqual(themeRows(world, "container").rows[0]![1], {
    part: PART.background,
    color: TOKENS.page,
    border_width: TOKENS.lineWidth,
    border_color: TOKENS.accent,
    corner_accent: [8, 8, 8, 8],
    corner_accent_width: TOKENS.cornerAccentWidth,
  });
});

test("Row centres its children on its own height; text and icons take the type scale and palette", async () => {
  const { world } = await render(
    h(
      Row,
      { id: "row", height: 24, layout: { padding_top: 4, padding_bottom: 4 } },
      h(TextLine, { id: "name", text: "Gain", tone: "accent", size: "small" }),
      h(Icon, { id: "mark", icon: "sync", tone: "amber" }),
    ),
    { fontSize: 32 },
  );
  assert.equal(world.fields("row", "GuiLayout").get("height"), 48);
  // The strut is as tall as the row's content box, so children centre on it.
  assert.deepEqual(world.children("row"), ["row/strut", "name", "mark"]);
  assert.equal(world.fields("row/strut", "GuiLayout").get("height"), 40);
  assert.equal(world.fields("name", "GuiLayout").get("align_y"), 0);
  assert.equal(world.fields("name", "CanvasText").get("font_size"), 26);
  assert.deepEqual(world.tone("name"), TOKENS.accent);
  assert.equal(
    world.fields("mark", "CanvasText").get("text"),
    GUI_KIT_ICONS.sync,
  );
  assert.equal(world.fields("mark", "CanvasText").get("font_size"), 48);
  assert.deepEqual(world.tone("mark"), TOKENS.amber);
});

test("InlineAlert draws its severity's frame, icon and tone with optional action", async () => {
  const { world, draw } = await render(
    h(InlineAlert, {
      id: "alert",
      severity: "warning",
      text: "Connection lost.",
      action: { label: "Reconnect" },
    }),
    { fontSize: 32 },
  );
  // Every length is twice its design size at twice the body size.
  const layout = world.fields("alert", "GuiLayout");
  assert.equal(layout.get("kind"), 1);
  assert.equal(layout.get("height"), 80);
  assert.equal(layout.get("padding_left"), 32);
  assert.equal(layout.get("padding_right"), 16);
  // Children centre on the row's content height, not on the tallest child.
  assert.equal(world.fields("alert/strut", "GuiLayout").get("height"), 80);
  assert.equal(world.fields("alert/strut", "GuiLayout").get("width"), 0);
  assert.equal(world.skin("alert"), "alertWarning");
  assert.deepEqual(world.children("alert"), [
    "alert/strut",
    "alert/icon",
    "alert/text",
    "alert/action",
  ]);
  assert.equal(
    world.fields("alert/icon", "CanvasText").get("text"),
    GUI_KIT_ICONS.warning,
  );
  assert.equal(world.fields("alert/icon", "CanvasText").get("font_size"), 48);
  assert.deepEqual(world.tone("alert/icon"), TOKENS.amber);
  assert.deepEqual(world.tone("alert/text"), TOKENS.text);
  assert.equal(world.fields("alert/text", "CanvasText").get("source"), FONT);
  assert.equal(world.fields("alert/text", "CanvasText").get("font_size"), 32);
  assert.equal(
    world.fields("alert/action", "GuiButton").get("label"),
    "Reconnect",
  );
  assert.equal(world.skin("alert/action"), "secondarySmall");
  assert.equal(world.fields("alert/action", "GuiFont").get("font_size"), 26);
  const action = world.fields("alert/action", "GuiLayout");
  assert.equal(action.get("height"), 64);
  assert.ok(
    Math.abs((action.get("width") as number) - (9 * 0.54 * 26 + 64)) < 1e-9,
  );

  await draw(
    h(InlineAlert, { id: "alert", severity: "error", text: "Failed." }),
  );
  assert.equal(world.skin("alert"), "alertError");
  assert.equal(
    world.fields("alert/icon", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.deepEqual(world.tone("alert/icon"), TOKENS.error);
  assert.equal(world.entities.has("alert/action"), false);
  assert.equal(world.fields("alert", "GuiLayout").get("padding_right"), 32);

  await draw(
    h(InlineAlert, { id: "alert", severity: "information", text: "Note." }),
  );
  assert.equal(world.skin("alert"), "alertInformation");
  assert.deepEqual(world.tone("alert/icon"), TOKENS.accent);
});

test("StatusBadge gives each status its frame, marker shape and tone, hugging its label", async () => {
  const { world } = await render([
    h(StatusBadge, { key: 1, id: "active", status: "active", label: "Online" }),
    h(StatusBadge, { key: 2, id: "busy", status: "busy", label: "Syncing" }),
    h(StatusBadge, {
      key: 3,
      id: "inactive",
      status: "inactive",
      label: "Offline",
    }),
    h(StatusBadge, {
      key: 4,
      id: "warning",
      status: "warning",
      label: "Degraded",
    }),
    h(StatusBadge, { key: 5, id: "error", status: "error", label: "Error" }),
  ]);
  const expected = {
    active: { frame: "badgeAccent", tone: TOKENS.accent, marker: "markerLit" },
    busy: {
      frame: "badgeAccent",
      tone: TOKENS.accent,
      marker: GUI_KIT_ICONS.sync,
    },
    inactive: {
      frame: "badgeNeutral",
      tone: TOKENS.neutral,
      marker: "markerUnlit",
    },
    warning: {
      frame: "badgeAmber",
      tone: TOKENS.amber,
      marker: GUI_KIT_ICONS.warning,
    },
    error: {
      frame: "badgeError",
      tone: TOKENS.error,
      marker: GUI_KIT_ICONS.error,
    },
  };
  for (const [id, { frame, tone, marker }] of Object.entries(expected)) {
    assert.equal(world.skin(id), frame, id);
    assert.equal(world.fields(id, "GuiLayout").get("height"), 32);
    assert.deepEqual(world.tone(`${id}/label`), tone, id);
    assert.equal(
      world.fields(`${id}/label`, "CanvasText").get("font_size"),
      13,
    );
    if (marker.startsWith("marker")) {
      assert.equal(world.skin(`${id}/marker`), marker, id);
      assert.equal(world.fields(`${id}/marker`, "GuiLayout").get("width"), 12);
    } else {
      assert.equal(
        world.fields(`${id}/marker`, "CanvasText").get("text"),
        marker,
      );
      assert.deepEqual(world.tone(`${id}/marker`), tone, id);
    }
    // Badges are not controls.
    assert.equal(world.entity(id).components.has("GuiButton"), false);
  }
  const label = "Online".length * 0.54 * 13;
  assert.ok(
    Math.abs(
      (world.fields("active", "GuiLayout").get("width") as number) -
        (10 + 12 + 8 + 10 + label),
    ) < 1e-9,
  );
});

const upload = (props: Partial<ProgressBarProps>) =>
  h(ProgressBar, { id: "upload", label: "Uploading", ...props });

const scan = (props: Partial<ProgressBarProps>) =>
  h(ProgressBar, { id: "scan", label: "Scanning", ...props });

/** A fill piece's own row: its cut corners, and the leading section's rest. */
function piece(cut: readonly number[], lead = false) {
  return [
    [
      0,
      {
        corner_cut: cut,
        ...(lead ? { opacity: 0.6 } : {}),
        part: PART.background,
      },
    ],
  ];
}

const START = [4, 0, 0, 0];
const END = [0, 0, 4, 0];
const BOTH = [4, 0, 4, 0];
const NONE = [0, 0, 0, 0];

test("ProgressBar fills the reported fraction and labels each outcome", async () => {
  const { world, draw } = await render(upload({ value: 0.656 }));
  assert.equal(world.fields("upload", "GuiLayout").get("height"), 24 + 4 + 32);
  assert.equal(world.skin("upload/frame"), "frame");
  // The frame holds the track in its clearance: a stack of its own, so
  // alignment in it stays inside the clearance.
  const frame = world.fields("upload/frame", "GuiLayout");
  assert.equal(frame.get("kind"), 4);
  assert.equal(frame.get("height"), 32);
  assert.equal(frame.get("padding_left"), 4);
  assert.deepEqual(world.children("upload/frame"), ["upload/track"]);
  assert.equal(world.fields("upload/track", "GuiLayout").get("kind"), 3);
  assert.equal(world.fields("upload/track", "GuiLayout").get("height"), 24);
  assert.deepEqual(world.children("upload/track"), ["upload/fills"]);
  assert.equal(world.fields("upload/fills", "GuiLayout").get("kind"), 1);
  assert.equal(world.fields("upload/fills", "GuiLayout").get("height"), 24);
  // The fill, the leading section just ahead of it, and the rest of the track.
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/lead",
    "upload/rest",
  ]);
  assert.equal(world.skin("upload/fill/0"), "valueAccent");
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("flex"), 0.656);
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("height"), 24);
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(START));
  // The leading section is the language's bar wide and ends the run's cut.
  assert.equal(world.skin("upload/lead"), "valueAccent");
  assert.equal(world.fields("upload/lead", "GuiLayout").get("width"), 8);
  assert.equal(world.fields("upload/lead", "GuiLayout").get("flex"), undefined);
  assert.deepEqual(skinRows(world, "upload/lead"), piece(END, true));
  assert.ok(
    Math.abs(
      (world.fields("upload/rest", "GuiLayout").get("flex") as number) - 0.344,
    ) < 1e-9,
  );
  // The readout never runs ahead of the report.
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "65%");
  assert.deepEqual(world.tone("upload/readout"), TOKENS.text);
  assertPulsing(world);

  await draw(upload({ value: 0.999 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "99%");
  // The reported value itself, not one less from rounding.
  await draw(upload({ value: 0.57 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "57%");
  await draw(upload({ segments: [{ value: 0.1 }, { value: 0.7 }] }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "80%");

  // Finished states have no leading section and no animation.
  await draw(upload({ status: "complete" }));
  assert.deepEqual(world.children("upload/fills"), ["upload/fill/0"]);
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("flex"), 1);
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(BOTH));
  assert.deepEqual(world.children("upload/track"), [
    "upload/fills",
    "upload/check",
  ]);
  assert.equal(world.skin("upload/check"), "check");
  const check = world.fields("upload/check", "GuiLayout");
  assert.equal(check.get("width"), 16);
  assert.equal(check.get("align_x"), 1);
  assert.equal(check.get("margin_right"), 4);
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "Complete",
  );
  assert.deepEqual(world.tone("upload/readout"), TOKENS.accent);
  assert.equal(world.controllers.size, 0);

  await draw(upload({ value: 0.4, status: "cancelled" }));
  assert.equal(world.skin("upload/fill/0"), "valueNeutral");
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("flex"), 0.4);
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(BOTH));
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/rest",
  ]);
  assert.equal(world.entities.has("upload/check"), false);
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "Cancelled",
  );
  assert.deepEqual(world.tone("upload/readout"), TOKENS.neutral);
  assert.equal(world.controllers.size, 0);

  await draw(upload({ value: 0.4, status: "failed" }));
  assert.equal(world.skin("upload/fill/0"), "valueError");
  assert.equal(
    world.fields("upload/outcome-icon", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "Failed",
  );
  assert.deepEqual(world.tone("upload/readout"), TOKENS.error);
  assert.equal(world.controllers.size, 0);

  // At zero the leading section sits at the start of the track, both cuts its.
  await draw(upload({ value: 0 }));
  assert.deepEqual(world.children("upload/fills"), [
    "upload/lead",
    "upload/rest",
  ]);
  assert.deepEqual(skinRows(world, "upload/lead"), piece(BOTH, true));
  assert.equal(world.fields("upload/rest", "GuiLayout").get("flex"), 1);
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "0%");
  assertPulsing(world);
});

test("ProgressBar draws its parts in order in their tones, within the track", async () => {
  // Parts past the track's end are clamped; empty parts draw nothing.
  const { world, draw } = await render(
    upload({
      segments: [
        { value: 0.25 },
        { value: 0, tone: "amber" },
        { value: 0.125, tone: "neutral" },
      ],
    }),
  );
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/fill/2",
    "upload/lead",
    "upload/rest",
  ]);
  assert.equal(world.skin("upload/fill/0"), "valueAccent");
  assert.equal(world.skin("upload/fill/2"), "valueNeutral");
  // Square joints inside the run, the part cut on its outer ends.
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(START));
  assert.deepEqual(skinRows(world, "upload/fill/2"), piece(NONE));
  assert.deepEqual(skinRows(world, "upload/lead"), piece(END, true));
  assert.equal(world.fields("upload/rest", "GuiLayout").get("flex"), 0.625);
  // The readout is the floored total.
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "37%");

  await draw(
    upload({
      segments: [
        { value: 0.5 },
        { value: 0.25, tone: "amber" },
        { value: 0.5, tone: "error" },
      ],
    }),
  );
  assert.equal(world.skin("upload/fill/1"), "valueAmber");
  assert.equal(world.skin("upload/fill/2"), "valueError");
  assert.equal(world.fields("upload/fill/2", "GuiLayout").get("flex"), 0.25);
  // A full track leaves no rest; the fill shares it with the leading section.
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/fill/1",
    "upload/fill/2",
    "upload/lead",
  ]);
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "100%",
  );
  assertPulsing(world);

  // Complete fills the track with the parts in proportion.
  await draw(
    upload({
      segments: [{ value: 0.25 }, { value: 0.125, tone: "amber" }],
      status: "complete",
    }),
  );
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/fill/1",
  ]);
  assert.ok(
    Math.abs(
      (world.fields("upload/fill/0", "GuiLayout").get("flex") as number) -
        2 / 3,
    ) < 1e-9,
  );
  assert.equal(world.skin("upload/fill/1"), "valueAmber");
  assert.deepEqual(skinRows(world, "upload/fill/1"), piece(END));
  assert.equal(world.controllers.size, 0);

  // An outcome recolours the reached parts.
  await draw(
    upload({
      segments: [{ value: 0.25 }, { value: 0.125, tone: "amber" }],
      status: "failed",
    }),
  );
  assert.equal(world.skin("upload/fill/0"), "valueError");
  assert.equal(world.skin("upload/fill/1"), "valueError");
  assert.equal(world.fields("upload/rest", "GuiLayout").get("flex"), 0.625);
});

test("the leading section pulses only while the task runs and the bar is declared", async () => {
  const bar = (props: Partial<ProgressBarProps>) =>
    upload({ value: 0.3, ...props });
  const { world, draw } = await render(bar({}), { reducedMotion: true });
  // Reduced motion: present and steady at its rest.
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "upload/lead"), piece(END, true));

  await draw(bar({}));
  assertPulsing(world);
  await draw(bar({ status: "complete" }));
  assert.equal(world.controllers.size, 0);
  assert.equal(world.entities.has("upload/lead"), false);
  await draw(bar({}));
  assert.equal(world.controllers.size, 1);
  await draw(h(Entity, { id: "done" }));
  assert.equal(world.controllers.size, 0);
  assert.equal(world.entities.has("upload/lead"), false);
});

test("an unknown duration moves its segment across the frame on the Host clock", async () => {
  const { world, draw } = await render(scan({}));
  assert.equal(world.entities.has("scan/readout"), false);
  assert.deepEqual(world.children("scan/track"), ["scan/segment"]);
  assert.equal(world.skin("scan/segment"), "valueAccent");
  const segment = world.fields("scan/segment", "GuiLayout");
  assert.equal(segment.get("width"), 96);
  assert.equal(segment.get("height"), 24);
  // It rests at the start; the animation adds its change to the alignment.
  assert.equal(segment.get("align_x"), -1);
  // Its clearance from the frame's ends is the frame's padding: it moves in
  // the track, a stack of its own inside it, since a stack aligns its
  // children within its outer box.
  const frame = world.fields("scan/frame", "GuiLayout");
  assert.equal(frame.get("kind"), 4);
  assert.equal(frame.get("padding_left"), 4);
  assert.equal(frame.get("padding_right"), 4);
  assert.equal(segment.get("margin_left"), undefined);

  // One looping controller drives the segment's layout alignment from one
  // end of the frame to the other and back.
  const [controller] = [...world.controllers.values()];
  assert.ok(controller, "no animation controller");
  assert.equal(controller.looping, true);
  assert.equal(world.controllers.size, 1);
  assert.deepEqual(world.controls, [{ action: "play" }]);
  const layout = DESCRIPTORS.GuiLayout!;
  assert.deepEqual(controller.drivers[0]!.property, {
    component: layout.id,
    offsets: [layout.fields.align_x!.offset],
  });
  const [clip] = [...world.clips.values()] as {
    duration: number;
    tracks: { keys: { time: number; value: { value: number } }[] }[];
  }[];
  assert.equal(clip!.duration, 2);
  assert.deepEqual(
    clip!.tracks[0]!.keys.map((key) => [key.time, key.value.value]),
    [
      [0, -1],
      [1, 1],
      [2, -1],
    ],
  );

  // Reduced motion holds it centred; a reported value replaces it.
  await draw(scan({}), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.equal(world.fields("scan/segment", "GuiLayout").get("align_x"), 0);
  await draw(scan({ value: 0.2 }));
  assert.equal(world.entities.has("scan/segment"), false);
  assert.deepEqual(world.children("scan/track"), ["scan/fills"]);
});

test("Expander declares its content after its header only while expanded", async () => {
  const content = h(Entity, { id: "gain" });
  const expander = (props: Omit<ExpanderProps, "children">) =>
    h(Expander, props, content);
  const { world, draw } = await render(
    expander({
      id: "advanced",
      label: "Advanced settings",
      summary: "3 options",
    }),
  );
  assert.equal(world.fields("advanced", "GuiButton").get("label"), "");
  assert.equal(
    world.fields("advanced", "GuiBehavior").get("semantic_label"),
    "Advanced settings",
  );
  assert.equal(world.fields("advanced", "GuiBehavior").get("enabled"), true);
  assert.equal(world.skin("advanced"), "expanderHeader");
  assert.equal(world.fields("advanced", "GuiLayout").get("height"), 40);
  assert.deepEqual(world.children("advanced"), [
    "advanced/strut",
    "advanced/chevron",
    "advanced/label",
    "advanced/summary",
  ]);
  assert.equal(
    world.fields("advanced/chevron", "CanvasText").get("text"),
    GUI_KIT_ICONS.collapsed,
  );
  assert.deepEqual(world.tone("advanced/label"), TOKENS.accent);
  assert.deepEqual(world.tone("advanced/summary"), TOKENS.neutral);
  assert.equal(world.entities.has("gain"), false);

  // Controlled: the content follows the header in its container.
  await draw(
    expander({ id: "advanced", label: "Advanced settings", expanded: true }),
  );
  assert.deepEqual(world.children("panel"), ["advanced", "gain"]);
  assert.equal(
    world.fields("advanced/chevron", "CanvasText").get("text"),
    GUI_KIT_ICONS.expanded,
  );
  assert.equal(world.entities.has("advanced/summary"), false);

  await draw(
    expander({
      id: "advanced",
      label: "Advanced settings",
      expanded: false,
      disabled: true,
    }),
  );
  assert.equal(world.entities.has("gain"), false);
  assert.equal(world.fields("advanced", "GuiBehavior").get("enabled"), false);
  assert.deepEqual(world.tone("advanced/label"), TOKENS.neutral);

  // Uncontrolled: the initial expansion.
  const { world: open } = await render(
    expander({ id: "other", label: "Other", defaultExpanded: true }),
  );
  assert.deepEqual(open.children("panel"), ["other", "gain"]);
});

/** Let effects, refs and animation controls reach their callbacks. */
async function settle(root: { flush(): Promise<void> }) {
  for (let attempt = 0; attempt < 10; attempt++) await turn();
  await root.flush();
  for (let attempt = 0; attempt < 10; attempt++) await turn();
}

const near = (actual: unknown, expected: number) =>
  assert.ok(
    Math.abs((actual as number) - expected) < 1e-9,
    `${String(actual)} is not ${expected}`,
  );

test("Panel parts: a header strip with window controls, a labelled division and a footer row", async () => {
  const pressed: string[] = [];
  const press = (name: string) => () => pressed.push(name);
  const panel = (minimized: boolean) =>
    h(
      Panel,
      { id: "monitor", minimized, layout: { width: 400, height: 300 } },
      h(
        PanelHeader,
        { id: "monitor/header", title: "SIGNAL MONITOR" },
        h(
          WindowControls,
          minimized
            ? { id: "monitor/controls", onRestore: press("restore") }
            : {
                id: "monitor/controls",
                onMinimize: press("minimize"),
                onMaximize: press("maximize"),
                onClose: press("close"),
              },
        ),
      ),
      !minimized && [
        h(LabelledSeparator, {
          key: "events",
          id: "monitor/events",
          label: "EVENTS",
        }),
        h(
          PanelFooter,
          { key: "footer", id: "monitor/footer" },
          h(TextLine, {
            id: "monitor/count",
            text: "2 EVENTS",
            layout: { flex: 1 },
          }),
          h(SecondaryButton, {
            id: "monitor/clear",
            label: "CLEAR",
            onPress: press("clear"),
          }),
        ),
      ],
    );
  const { world, root, draw } = await render(panel(false), { fontSize: 32 });
  assert.equal(world.skin("monitor"), "container");
  assert.deepEqual(world.children("monitor"), [
    "monitor/header",
    "monitor/header/separator",
    "monitor/events",
    "monitor/footer/separator",
    "monitor/footer",
  ]);

  // The header is a control-height strip: the title at the inset, the docked
  // buttons as far from its end as from its edges, then a division.
  const header = world.fields("monitor/header", "GuiLayout");
  assert.equal(header.get("height"), 80);
  assert.equal(header.get("padding_left"), 32);
  assert.equal(header.get("padding_right"), 16);
  assert.deepEqual(world.tone("monitor/header/title"), TOKENS.accent);
  assert.equal(
    world.fields("monitor/header/title", "GuiLayout").get("flex"),
    1,
  );
  assert.equal(world.skin("monitor/header/separator"), "division");
  assert.deepEqual(world.children("monitor/controls"), [
    "monitor/controls/minimize",
    "monitor/controls/maximize",
    "monitor/controls/close",
  ]);
  const controls = world.fields("monitor/controls", "GuiLayout");
  assert.equal(controls.get("width"), (3 * 32 + 2 * 8) * 2);
  assert.equal(controls.get("height"), 48);
  for (const kind of ["minimize", "maximize", "close"] as const) {
    const id = `monitor/controls/${kind}`;
    assert.equal(
      world.fields(id, "GuiButton").get("label"),
      GUI_KIT_ICONS[kind],
    );
    assert.equal(world.skin(id), "dockedIcon");
    assert.equal(world.fields(id, "GuiFont").get("font_size"), 48);
    assert.equal(world.fields(id, "GuiLayout").get("width"), 64);
    assert.equal(world.fields(id, "GuiLayout").get("height"), 48);
  }
  assert.equal(
    world.fields("monitor/controls/close", "GuiBehavior").get("semantic_label"),
    "Close",
  );
  assert.equal(
    world.fields("monitor/controls/maximize", "GuiLayout").get("margin_left"),
    16,
  );

  // A labelled division: a stub, the accent label and the rest of the line.
  assert.deepEqual(world.children("monitor/events"), [
    "monitor/events/strut",
    "monitor/events/stub",
    "monitor/events/label",
    "monitor/events/line",
  ]);
  assert.equal(world.fields("monitor/events", "GuiLayout").get("height"), 48);
  assert.equal(
    world.fields("monitor/events/stub", "GuiLayout").get("width"),
    32,
  );
  assert.equal(world.fields("monitor/events/line", "GuiLayout").get("flex"), 1);
  assert.equal(world.skin("monitor/events/line"), "division");
  assert.deepEqual(world.tone("monitor/events/label"), TOKENS.accent);

  // The footer: small buttons in half-inset margins at the content inset.
  const footer = world.fields("monitor/footer", "GuiLayout");
  assert.equal(footer.get("height"), (32 + 16) * 2);
  assert.equal(footer.get("padding_left"), 32);
  assert.equal(footer.get("padding_right"), 32);
  assert.equal(world.skin("monitor/clear"), "secondarySmall");
  assert.equal(world.fields("monitor/clear", "GuiLayout").get("height"), 64);
  near(
    world.fields("monitor/clear", "GuiLayout").get("width"),
    5 * 0.54 * 26 + 64,
  );

  // Presses reach the application's callbacks.
  world.effect("monitor/controls/close", { kind: "pressed" });
  world.effect("monitor/clear", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(pressed, ["close", "clear"]);

  // Minimised: the same frame unlit, as tall as its header, the title in the
  // text colour and no division.
  await draw(panel(true));
  assert.equal(world.skin("monitor"), "containerUnlit");
  assert.equal(world.fields("monitor", "GuiLayout").get("height"), 80);
  assert.deepEqual(world.tone("monitor/header/title"), TOKENS.text);
  assert.deepEqual(world.children("monitor"), ["monitor/header"]);
  assert.deepEqual(world.children("monitor/controls"), [
    "monitor/controls/restore",
  ]);
  assert.equal(world.fields("monitor/controls", "GuiLayout").get("width"), 64);
});

test("WindowControl stands free at the control height and takes the amber variant", async () => {
  const { world } = await render([
    h(WindowControl, { key: 1, id: "free", kind: "close", docked: false }),
    h(WindowControl, { key: 2, id: "docked", kind: "close", amber: true }),
    h(WindowControl, {
      key: 3,
      id: "off",
      kind: "close",
      docked: false,
      amber: true,
      disabled: true,
    }),
  ]);
  assert.equal(world.skin("free"), "secondaryIcon");
  assert.equal(world.fields("free", "GuiLayout").get("width"), 68);
  assert.equal(world.fields("free", "GuiLayout").get("height"), 40);
  assert.equal(world.fields("free", "GuiBehavior").get("enabled"), true);
  assert.equal(world.skin("docked"), "dockedIconAmber");
  assert.equal(world.skin("off"), "secondaryIconAmber");
  assert.equal(world.fields("off", "GuiBehavior").get("enabled"), false);
  // The docked amber look is the amber secondary look with square corners.
  assert.deepEqual(themeRows(world, "dockedIconAmber").rows, [
    [
      0,
      {
        part: PART.background,
        corner_cut: [0, 0, 0, 0],
        border_color: TOKENS.amber,
      },
    ],
  ]);
});

test("SecondaryButton takes the amber variant at the small type size", async () => {
  const { world } = await render([
    h(SecondaryButton, { key: 1, id: "clear", label: "CLEAR" }),
    h(SecondaryButton, { key: 2, id: "purge", label: "PURGE", amber: true }),
  ]);
  assert.equal(world.skin("clear"), "secondarySmall");
  assert.equal(world.skin("purge"), "secondarySmallAmber");
  assert.equal(
    world.fields("ipp-kit/theme/secondarySmallAmber", "GuiTheme").get("em"),
    TOKENS.textSmall,
  );
  // The amber look keeps the secondary geometry and swaps its colours.
  assert.deepEqual(themeRows(world, "secondarySmallAmber").rows, [
    [
      0,
      {
        part: PART.background,
        corner_cut: [4, 0, 4, 0],
        border_color: TOKENS.amber,
      },
    ],
  ]);
  assert.equal(
    world.fields("purge", "GuiLayout").get("width"),
    world.fields("clear", "GuiLayout").get("width"),
  );
});

test("EmptyState centres its message in a content frame", async () => {
  const { world } = await render(
    h(EmptyState, { id: "empty", text: "No records" }),
  );
  assert.equal(world.skin("empty"), "frame");
  assert.equal(world.fields("empty", "GuiLayout").get("kind"), 3);
  assert.equal(world.fields("empty", "GuiLayout").get("height"), 40);
  assert.equal(
    world.fields("empty/text", "CanvasText").get("text"),
    "No records",
  );
  assert.equal(world.fields("empty/text", "GuiLayout").get("align_x"), 0);
  assert.deepEqual(world.tone("empty/text"), TOKENS.neutral);
  assert.equal(world.fields("empty/text", "CanvasText").get("font_size"), 13);
});

const COLUMNS: readonly DataGridColumn[] = [
  { key: "node", title: "NODE", width: 100 },
  { key: "signal", title: "SIGNAL", width: 96, align: "end" },
  { key: "status", title: "STATUS" },
];

const record = (node: string, signal: string, status: string): DataGridRow => ({
  key: node.toLowerCase(),
  cells: { node, signal, status },
});

const ROWS = [
  record("Alpha", "65%", "Online"),
  record("Bravo", "42%", "Standby"),
  record("Charlie", "88%", "Online"),
];

const grid = (props: Partial<DataGridProps>) =>
  h(DataGrid, {
    id: "grid",
    columns: COLUMNS,
    rows: ROWS,
    ...props,
  } as DataGridProps);

test("DataGrid lays out header, rows and lines; rows are Buttons the application selects", async () => {
  const pressed: string[] = [];
  const requested: unknown[] = [];
  const { world, root, draw } = await render(
    grid({
      sort: { column: "signal", direction: "descending" },
      selected: "bravo",
      focusedCell: { row: "bravo", column: "signal" },
      onRowPress: (key) => pressed.push(key),
      onRowContextMenu: (key, event) => requested.push([key, event.point]),
      footer: "1-3 of 3",
    }),
  );
  // The grid holds its rows: header, three rows and the footer line.
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 36 * 4 + 24);
  assert.deepEqual(world.children("grid"), ["grid/table", "grid/footer"]);
  assert.deepEqual(world.children("grid/table"), [
    "grid/lines",
    "grid/header",
    "grid/body",
    "grid/bottom-line",
  ]);
  assert.equal(world.skin("grid/bottom-line"), "quiet");
  assert.equal(world.fields("grid/bottom-line", "GuiLayout").get("align_y"), 1);

  // Column lines once for the table, centred on each inner boundary; columns
  // take their width or share the rest.
  const lines = world.fields("grid/lines", "GuiLayout");
  assert.equal(lines.get("padding_left"), 4);
  assert.equal(lines.get("padding_right"), 0);
  assert.equal(world.fields("grid/lines/node", "GuiLayout").get("width"), 100);
  assert.equal(world.fields("grid/lines/status", "GuiLayout").get("flex"), 1);
  assert.deepEqual(world.children("grid/lines/status"), []);
  assert.equal(world.skin("grid/lines/node/line"), "quiet");
  const line = world.fields("grid/lines/node/line", "GuiLayout");
  assert.equal(line.get("width"), 1.25);
  assert.equal(line.get("margin_right"), -0.625);

  // The header: small accent titles, all at the start, and the sort marker.
  assert.deepEqual(world.tone("grid/header/signal/text"), TOKENS.accent);
  assert.equal(
    world.fields("grid/header/signal/text", "CanvasText").get("font_size"),
    13,
  );
  assert.equal(
    world.fields("grid/header/signal/text", "GuiLayout").get("align_x"),
    -1,
  );
  assert.equal(
    world.fields("grid/header/signal/sort", "CanvasText").get("text"),
    GUI_KIT_ICONS.sortDescending,
  );
  near(
    world.fields("grid/header/signal/sort", "GuiLayout").get("margin_left"),
    16 + 6 * 0.54 * 13 + 4,
  );
  assert.equal(world.entities.has("grid/header/node/sort"), false);

  // Rows: docked Buttons the row height tall, the selected one selected;
  // numbers end their cells.
  assert.deepEqual(world.children("grid/body"), [
    "grid/row/alpha",
    "grid/row/bravo",
    "grid/row/charlie",
  ]);
  assert.equal(world.skin("grid/row/bravo"), "gridRow");
  assert.equal(world.fields("grid/row/bravo", "GuiLayout").get("height"), 36);
  assert.equal(
    world.fields("grid/row/bravo", "GuiButton").get("selected"),
    true,
  );
  assert.equal(
    world.fields("grid/row/alpha", "GuiButton").get("selected"),
    false,
  );
  assert.equal(
    world.entity("grid/row/alpha").components.has("GuiBehavior"),
    false,
  );
  assert.deepEqual(world.children("grid/row/bravo"), [
    "grid/row/bravo/cells",
    "grid/row/bravo/line",
  ]);
  const cells = world.fields("grid/row/bravo/cells", "GuiLayout");
  assert.equal(cells.get("padding_left"), 4);
  assert.equal(cells.get("padding_bottom"), 1.25);
  assert.equal(
    world.fields("grid/row/bravo/signal", "GuiLayout").get("width"),
    96,
  );
  const end = world.fields("grid/row/bravo/signal/text", "GuiLayout");
  assert.equal(end.get("align_x"), 1);
  assert.equal(end.get("margin_right"), 16);
  const start = world.fields("grid/row/bravo/node/text", "GuiLayout");
  assert.equal(start.get("align_x"), -1);
  assert.equal(start.get("margin_left"), 16);
  assert.deepEqual(world.tone("grid/row/bravo/node/text"), TOKENS.text);
  assert.equal(
    world.fields("grid/row/bravo/node/text", "CanvasText").get("font_size"),
    16,
  );

  // The focused cell's edge sits on its four lines.
  assert.equal(world.skin("grid/row/bravo/signal/focus"), "cellFocus");
  const focus = world.fields("grid/row/bravo/signal/focus", "GuiLayout");
  assert.equal(focus.get("margin_left"), -0.75);
  assert.equal(focus.get("margin_right"), -0.75);
  assert.equal(focus.get("margin_top"), -1.375);
  assert.equal(world.entities.has("grid/row/alpha/signal/focus"), false);

  // The row look: nothing at rest, a legible press, and the selected row's
  // tint with the accent bar over the first line.
  const rows = new Map(
    themeRows(world, "gridRow").rows.map(([, row]) => [row.part, row]),
  );
  assert.deepEqual(rows.get(PART.background), {
    part: PART.background,
    color: [0, 0, 0, 0],
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
  });
  assert.deepEqual(rows.get(PRESSED), { part: PRESSED, color: [0, 0, 0, 0] });
  for (const state of [STATE.idle, STATE.hovered, STATE.pressed])
    assert.deepEqual(rows.get(PART.background + state + CHECKED), {
      part: PART.background + state + CHECKED,
      fill_mode: 1,
      gradient_start: [4.625, 0],
      gradient_end: [4.635, 0],
      gradient_color0: TOKENS.accent,
      gradient_color1: TOKENS.rowTint,
    });

  // The footer's small text at the cell inset.
  assert.equal(
    world.fields("grid/footer/text", "CanvasText").get("text"),
    "1-3 of 3",
  );
  assert.equal(
    world.fields("grid/footer", "GuiLayout").get("padding_left"),
    20,
  );

  // A press names its row; the application owns the selection.
  world.effect("grid/row/charlie", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(pressed, ["charlie"]);
  // A context request names its row and keeps its point.
  world.effect("grid/row/alpha", { kind: "contextRequested", point: [40, 70] });
  await settle(root);
  assert.deepEqual(requested, [["alpha", [40, 70]]]);
  assert.deepEqual(pressed, ["charlie"]);
  assert.equal(
    world.fields("grid/row/bravo", "GuiButton").get("selected"),
    true,
  );

  // Rows that do not take focus, for a grid a group drives.
  await draw(grid({ focusableRows: false }));
  assert.equal(
    world.fields("grid/row/alpha", "GuiBehavior").get("focusable"),
    false,
  );
  assert.equal(
    world.fields("grid/row/bravo", "GuiButton").get("selected"),
    false,
  );
  assert.equal(world.entities.has("grid/footer"), false);
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 36 * 4);

  // A grid that flexes takes its container's height instead.
  const { world: flexed } = await render(grid({ layout: { flex: 1 } }));
  assert.equal(flexed.fields("grid", "GuiLayout").has("height"), false);
  assert.equal(flexed.fields("grid", "GuiLayout").get("flex"), 1);
});

test("DataGrid scrolls, virtualises, edits a cell and shows the empty state", async () => {
  const many = Array.from({ length: 9 }, (_, index) =>
    record(`Node${index}`, `${index}%`, "Online"),
  );
  const scrollHandles: unknown[] = [];
  const { world, root, draw } = await render(
    grid({
      rows: many,
      scroll: true,
      scrollRef: (handle) => {
        if (handle) scrollHandles.push(handle);
      },
      layout: { height: 200 },
    }),
  );
  // The body's ScrollView hands its handle to the application.
  await settle(root);
  assert.equal(scrollHandles.length > 0, true);
  // A scrolling body takes the height it is given and keeps its bar in a
  // column of its own after the last column.
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 200);
  const body = world.fields("grid/body", "GuiScrollView");
  assert.equal(body.get("bar_thickness"), 8);
  assert.equal(body.get("bar_inset"), 8);
  assert.equal(body.get("bar_end_inset"), 0);
  assert.equal(world.skin("grid/body"), "gridBody");
  assert.equal(
    world.fields("grid/body/rows", "GuiLayout").get("padding_right"),
    24,
  );
  assert.equal(world.children("grid/body/rows").length, 9);
  assert.equal(
    world.fields("grid/lines", "GuiLayout").get("padding_right"),
    24,
  );
  assert.equal(world.children("grid/lines/status").length, 2);
  assert.equal(
    world.fields("grid/header/cells", "GuiLayout").get("padding_right"),
    24,
  );
  assert.equal(
    world.fields("grid/row/node0/cells", "GuiLayout").get("padding_right"),
    0,
  );
  assert.equal(
    world.fields("grid/bottom-line", "GuiLayout").get("margin_right"),
    24,
  );
  const gridBody = new Map(
    themeRows(world, "gridBody").rows.map(([, row]) => [row.part, row]),
  );
  assert.deepEqual(gridBody.get(PART.background), {
    part: PART.background,
    border_width: 0,
    color: [0, 0, 0, 0],
  });

  // A virtual list realises the rows the runtime asks for, the row height each.
  await draw(
    h(DataGrid, {
      id: "grid",
      columns: COLUMNS,
      rowCount: 500,
      row: (index: number) => record(`Node${index}`, "1%", "Online"),
      layout: { height: 200 },
    }),
  );
  const list = world.fields("grid/body", "GuiVirtualList");
  assert.equal(list.get("item_count"), 500);
  assert.equal(list.get("item_extent"), 36);
  assert.equal(list.get("bar_inset"), 8);

  // The editing cell holds a text input in the square text input look that
  // takes focus when it appears; Enter reports its text.
  const edits: unknown[] = [];
  await draw(
    grid({
      editingCell: { row: "bravo", column: "signal" },
      onCellEdit: (cell, text) => edits.push(cell, text),
    }),
  );
  await settle(root);
  assert.equal(world.entities.has("grid/row/bravo/signal/text"), false);
  assert.equal(world.skin("grid/row/bravo/signal/editor"), "cellEditor");
  assert.equal(
    world.fields("grid/row/bravo/signal/editor", "GuiTextInput").get("text"),
    "42%",
  );
  assert.deepEqual(world.actions, [
    ["grid/row/bravo/signal/editor", { kind: "focus" }],
  ]);
  assert.deepEqual(themeRows(world, "cellEditor").rows, [
    [0, { part: PART.background, corner_cut: [0, 0, 0, 0] }],
  ]);
  world.effect(
    "grid/row/bravo/signal/editor",
    { kind: "submitted", text: "41%" },
    "GuiTextInput",
  );
  await settle(root);
  assert.deepEqual(edits, [{ row: "bravo", column: "signal" }, "41%"]);

  // Without rows the body shows the empty state, as tall as it needs.
  await draw(grid({ rows: [], scroll: true, emptyText: "No nodes" }));
  assert.deepEqual(world.children("grid/body"), ["grid/body/empty"]);
  assert.equal(
    world.fields("grid/body/empty/text", "CanvasText").get("text"),
    "No nodes",
  );
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 36 + 40 + 32);
  assert.equal(world.fields("grid/lines", "GuiLayout").get("padding_right"), 0);
  const empty = world.fields("grid/body", "GuiLayout");
  assert.deepEqual(
    ["top", "right", "bottom", "left"].map((side) =>
      empty.get(`padding_${side}`),
    ),
    [16, 16, 16, 4 + 16],
  );

  // Rows that replace the empty state lose its inset: the body is the same
  // entity, and its row forms name their padding as well.
  for (const scroll of [false, true]) {
    await draw(grid({ rows: [], scroll }));
    await draw(grid({ scroll }));
    const rows = world.fields("grid/body", "GuiLayout");
    assert.deepEqual(
      ["top", "right", "bottom", "left"].map((side) =>
        rows.get(`padding_${side}`),
      ),
      [0, 0, 0, 0],
      scroll ? "scrolled rows" : "rows",
    );
    assert.equal(rows.get("margin_top"), 36);
  }
});

const TOASTS: readonly ToastItem[] = [
  { key: "saved", severity: "success", text: "Scene saved" },
  {
    key: "upload",
    severity: "error",
    text: "Upload failed",
    action: { label: "Retry" },
  },
  { key: "note", text: "Third" },
];

const stack = (props: Partial<ToastStackProps>) =>
  h(ToastStack, { id: "toasts", toasts: TOASTS, ...props });

type Clip = {
  duration: number;
  tracks: {
    property: unknown;
    keys: { time: number; value: { value: number } }[];
  }[];
};

/** The keys of the one clip the World holds, as time and value. */
function clipKeys(world: KitWorld) {
  const clips = [...world.clips.values()] as Clip[];
  return clips.map((clip) =>
    clip.tracks[0]!.keys.map((key) => [key.time, key.value.value]),
  );
}

test("the kit's layer planes have one role each, a dialog's anchored overlays below the toasts", () => {
  // The runtime's plane for `layer` under a parent on plane `parent`.
  const resolve = (parent: number, layer: number) =>
    layer === 0 ? parent : Math.max(layer, parent + 1);
  const { anchored, dialog, toast } = GUI_KIT_LAYERS;
  const content = 0;
  const anchoredInDialog = resolve(dialog, anchored);

  // Content, anchored overlays, dialogs, a dialog's anchored overlays and
  // toasts each take a plane of their own, in that order.
  const planes = [
    content,
    resolve(content, anchored),
    resolve(content, dialog),
    anchoredInDialog,
    resolve(content, toast),
  ];
  assert.ok(
    planes.every((plane, index) => index === 0 || plane > planes[index - 1]!),
  );
  assert.ok(Math.max(anchored, dialog + 1) < toast);

  // A menu inside a popover inside a dialog reaches the toast plane, where
  // tree order decides.
  assert.equal(resolve(anchoredInDialog, anchored), toast);
});

test("ToastStack is a top-level manual overlay of toasts in stable order", async () => {
  const { world, draw } = await render(stack({ limit: 2 }));
  // On the toast layer, above dialogs.
  const style = world.fields("toasts", "CanvasStyle");
  assert.equal(style.get("layer"), GUI_KIT_LAYERS.toast);
  assert.ok(GUI_KIT_LAYERS.toast > GUI_KIT_LAYERS.dialog);
  assert.equal(style.get("x"), -16);
  assert.equal(style.get("y"), -16);
  const overlay = world.fields("toasts", "GuiOverlay");
  assert.equal(overlay.get("side"), 0);
  assert.equal(overlay.get("align"), 2);
  assert.equal(world.fields("toasts", "GuiBehavior").get("visible"), true);
  assert.equal(world.fields("toasts", "GuiLayout").get("kind"), 2);
  assert.equal(world.fields("toasts", "GuiLayout").get("width"), 480);
  // The limit shows the first toasts; the rest wait.
  assert.deepEqual(world.children("toasts"), ["toasts/saved", "toasts/upload"]);

  // A toast is a non-focusable Button: a control row in half-inset margins.
  const saved = world.fields("toasts/saved", "GuiLayout");
  assert.equal(saved.get("kind"), 1);
  assert.equal(saved.get("height"), 56);
  assert.equal(world.fields("toasts/saved", "GuiButton").get("label"), "");
  const behavior = world.fields("toasts/saved", "GuiBehavior");
  assert.equal(behavior.get("focusable"), false);
  assert.equal(behavior.get("semantic_label"), "Scene saved");
  assert.equal(world.fields("toasts/saved", "CanvasStyle").get("opacity"), 1);
  assert.equal(world.skin("toasts/saved"), "toastAccent");
  assert.deepEqual(world.children("toasts/saved"), [
    "toasts/saved/strut",
    "toasts/saved/mark",
    "toasts/saved/text",
    "toasts/saved/close",
  ]);
  assert.equal(world.skin("toasts/saved/mark"), "checkLit");
  assert.equal(world.skin("toasts/saved/close"), "dockedIcon");
  assert.equal(
    world.fields("toasts/saved/close", "GuiBehavior").get("semantic_label"),
    "Dismiss",
  );

  // An error with an action: its colour and icon, the action, a divider.
  assert.equal(world.skin("toasts/upload"), "toastError");
  assert.equal(
    world.fields("toasts/upload", "GuiLayout").get("margin_top"),
    16,
  );
  assert.equal(
    world.fields("toasts/upload/mark", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.deepEqual(world.tone("toasts/upload/mark"), TOKENS.error);
  assert.deepEqual(world.children("toasts/upload").slice(3), [
    "toasts/upload/action",
    "toasts/upload/divider",
    "toasts/upload/close",
  ]);
  assert.equal(world.skin("toasts/upload/action"), "secondarySmall");
  assert.equal(world.skin("toasts/upload/divider"), "division");
  // A toast floats over content: its tint lies on an opaque interior, the
  // surface with a hundredth of its role colour, blended in linear light.
  const interior = [
    ...TOKENS.surface
      .slice(0, 3)
      .map((value, channel) => value * 0.99 + TOKENS.error[channel]! * 0.01),
    1,
  ];
  assert.deepEqual(themeRows(world, "toastError").rows, [
    [
      PART.background,
      {
        part: PART.background,
        color: interior,
        border_color: TOKENS.error,
      },
    ],
    [
      1,
      {
        part: PRESSED,
        color: interior,
        border_color: TOKENS.error,
        glow_color: TOKENS.error,
      },
    ],
    [
      2,
      { part: HOVERED, border_color: TOKENS.error, glow_color: TOKENS.error },
    ],
  ]);

  // Only the success toast dismisses itself: it holds its opacity for six
  // seconds of the Host clock and fades out over a tenth of a second.
  assert.equal(world.controllers.size, 1);
  const [controller] = [...world.controllers.values()];
  const canvasStyle = DESCRIPTORS.CanvasStyle!;
  assert.deepEqual(controller!.drivers[0]!.property, {
    component: canvasStyle.id,
    offsets: [canvasStyle.fields.opacity!.offset],
  });
  assert.equal(controller!.looping, false);
  assert.deepEqual(world.controls, [{ action: "play" }]);
  assert.deepEqual(clipKeys(world), [
    [
      [0, 1],
      [6, 1],
      [6.1, 0],
    ],
  ]);

  // Other edges; an empty stack closes.
  await draw(stack({ side: "top", align: "start", toasts: [] }));
  assert.equal(world.fields("toasts", "GuiOverlay").get("side"), 1);
  assert.equal(world.fields("toasts", "GuiOverlay").get("align"), 0);
  assert.equal(world.fields("toasts", "CanvasStyle").get("x"), 16);
  assert.equal(world.fields("toasts", "CanvasStyle").get("y"), 16);
  assert.equal(world.fields("toasts", "GuiBehavior").get("visible"), false);
  assert.deepEqual(world.children("toasts"), []);
});

test("a toast dismisses on its close button or at the end of its time, paused while hovered or focused", async () => {
  const dismissed: string[] = [];
  const { world, root, draw } = await render(
    stack({ onDismiss: (key) => dismissed.push(key) }),
  );
  const [id] = [...world.controllers.keys()];
  assert.ok(id !== undefined, "no controller");

  // The close button reports its toast.
  world.effect("toasts/upload/close", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(dismissed, ["upload"]);

  // A hovering pointer or focus on a button pauses the time; play resumes
  // when neither remains.
  const hover = (symbol: string, hovered: boolean) =>
    world.effect(symbol, {
      kind: "interactionChanged",
      pointer: 1n,
      state: { hovered, pressed: false, captured: false },
      changed: true,
    });
  const controls = () =>
    world.controlled
      .filter(([controller]) => controller === id)
      .map(([, control]) => (control as { action: string }).action);
  hover("toasts/saved", true);
  await settle(root);
  assert.deepEqual(controls(), ["play", "pause"]);
  world.effect("toasts/saved/close", {
    kind: "focusChanged",
    focused: true,
    changed: true,
    part: 0,
  });
  hover("toasts/saved", false);
  await settle(root);
  assert.deepEqual(controls(), ["play", "pause"]);
  world.effect("toasts/saved/close", {
    kind: "focusChanged",
    focused: false,
    changed: true,
    part: 0,
  });
  await settle(root);
  assert.deepEqual(controls(), ["play", "pause", "play"]);

  // Completion on the Host clock ends the toast.
  world.complete(id);
  await settle(root);
  assert.deepEqual(dismissed, ["upload", "saved"]);

  // The time and the fade follow the stack's options; persistent toasts and
  // reduced motion change them.
  await draw(
    stack({
      toasts: [
        { key: "quick", text: "Quick" },
        { key: "kept", text: "Kept", persistent: true },
      ],
      duration: 1500,
    }),
    { reducedMotion: true },
  );
  const quick = [...world.controllers.values()];
  assert.equal(quick.length, 1);
  assert.deepEqual(clipKeys(world).at(-1), [
    [0, 1],
    [1.5, 0],
  ]);
});

/** An entity's own skin rows: a ring's thickness, start and sweep. */
function skinRows(world: KitWorld, symbol: string) {
  const parts = world.fields(symbol, "GuiSkin").get("parts");
  return JSON.parse(new TextDecoder().decode(parts as Uint8Array)).rows as [
    number,
    Record<string, unknown>,
  ][];
}

/** A ring's own row: its background, `thickness` thick, through `sweep`. */
function ring(thickness: number, sweep: number, start = 0, lead = false) {
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

/** The leading arcs' sweeps: 8 units along the middle of each ring. */
const LEAD_LARGE = 8 / (Math.PI * 116);
const LEAD_SMALL = 8 / (Math.PI * 56);

/** The one turning arc's controller: a looping turn a second of its start. */
interface RowClip {
  readonly duration: number;
  readonly tracks: readonly {
    readonly property: { readonly offsets: readonly number[] };
    readonly keys: readonly {
      readonly time: number;
      readonly value: unknown;
    }[];
  }[];
}

/** The one controller, looping, and the clip it plays on an own-row `property`. */
function loopingRowClip(world: KitWorld, property: string): RowClip {
  assert.equal(world.controllers.size, 1);
  const [controller] = [...world.controllers.values()];
  assert.equal(controller!.looping, true);
  const offset = rowOffset(0, property);
  assert.deepEqual(controller!.drivers[0]!.property, {
    component: DESCRIPTORS.GuiSkin!.id,
    offsets: [offset],
  });
  const clip = ([...world.clips.values()] as RowClip[]).find(
    (candidate) => candidate.tracks[0]?.property.offsets[0] === offset,
  );
  assert.ok(clip, `no clip on ${property}`);
  return clip;
}

/** A row property's key: a dynamic value. */
function dynamic(value: number) {
  return { kind: "dynamic", value: { kind: "f32", value } };
}

/** The one turning arc's controller: a looping turn a second of its start. */
function assertTurning(world: KitWorld) {
  const clip = loopingRowClip(world, "arc_start");
  assert.equal(clip.duration, 1);
  assert.ok(clip.tracks[0]!.keys.every((key) => !("interpolation" in key)));
  assert.deepEqual(
    clip.tracks[0]!.keys.map((key) => [key.time, key.value]),
    [
      [0, dynamic(0)],
      [1, dynamic(1)],
    ],
  );
}

/**
 * The one pulsing section's controller: its opacity eases from its rest of
 * 0.6 down to 0.1 and back every 1.5 seconds, so it stays within 0..1.
 */
function assertPulsing(world: KitWorld) {
  const clip = loopingRowClip(world, "opacity");
  assert.equal(clip.duration, 1.5);
  const keys = clip.tracks[0]!.keys;
  assert.equal(keys.length, 9);
  // Linear by the encoder's default: an interpolation on the last key, which
  // has no next, fails to encode.
  assert.deepEqual(keys[0], { time: 0, value: dynamic(0) });
  assert.ok(keys.every((key) => !("interpolation" in key)));
  assert.equal(keys[4]!.time, 0.75);
  assert.deepEqual(keys[4]!.value, dynamic(-0.5));
  assert.equal(keys[8]!.time, 1.5);
  for (const key of keys) {
    const change = (key.value as { value: { value: number } }).value.value;
    assert.ok(change <= 1e-12 && 0.6 + change >= 0.1 - 1e-9, `${change}`);
  }
}

test("the arc themes are the ring shape in role colours, and the lit check", async () => {
  const { world } = await render(h(Entity, { id: "empty" }));
  for (const [name, color] of [
    ["arcTrack", TOKENS.line],
    ["arcAccent", TOKENS.accent],
    ["arcError", TOKENS.error],
  ] as const)
    assert.deepEqual(themeRows(world, name).rows, [
      [0, { part: PART.background, shape: 2, color }],
    ]);
  assert.deepEqual(themeRows(world, "checkLit").rows, [
    [0, { ...CHECK_MARK, part: PART.background, color: TOKENS.accent }],
  ]);
});

test("Spinner turns a lit quarter over a quiet track once a second beside its text", async () => {
  const { world } = await render(
    h(Spinner, { id: "busy", label: "Preparing..." }),
    { fontSize: 32 },
  );
  const layout = world.fields("busy", "GuiLayout");
  assert.equal(layout.get("kind"), 1);
  assert.equal(layout.get("height"), 48);
  // The text and a hundredth of its size, so rounding never wraps it.
  assert.ok(
    Math.abs((layout.get("width") as number) - (64 + 12 * 0.54 * 32 + 0.32)) <
      1e-9,
  );
  assert.equal(world.fields("busy", "GuiFont").get("font_size"), 32);
  assert.deepEqual(world.children("busy"), [
    "busy/strut",
    "busy/track",
    "busy/label",
  ]);
  assert.deepEqual(world.children("busy/track"), ["busy/arc"]);
  assert.equal(world.skin("busy/track"), "arcTrack");
  assert.equal(world.skin("busy/arc"), "arcAccent");
  for (const symbol of ["busy/track", "busy/arc"]) {
    const ringLayout = world.fields(symbol, "GuiLayout");
    assert.equal(ringLayout.get("width"), 48);
    assert.equal(ringLayout.get("height"), 48);
    assert.equal(ringLayout.get("align_y"), 0);
  }
  // Thickness is absolute in the World's units; the start is present to bind.
  assert.deepEqual(skinRows(world, "busy/track"), ring(6, 1));
  assert.deepEqual(skinRows(world, "busy/arc"), ring(6, 0.25));
  assert.equal(
    world.fields("busy/label", "CanvasText").get("text"),
    "Preparing...",
  );
  assert.equal(world.fields("busy/label", "CanvasText").get("font_size"), 32);
  assert.equal(world.fields("busy/label", "GuiLayout").get("margin_left"), 16);
  assert.deepEqual(world.tone("busy/label"), TOKENS.text);
  assert.equal(world.entity("busy").components.has("GuiButton"), false);

  assertTurning(world);
  assert.deepEqual(world.controls, [{ action: "play" }]);
});

test("a spinner's animation exists only while it is declared and moving", async () => {
  const busy = () => h(Spinner, { id: "busy", label: "Preparing..." });
  const { world, draw } = await render(busy(), { reducedMotion: true });
  // Reduced motion: the same symbol, frozen at its rest.
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "busy/arc"), ring(3, 0.25));

  await draw(busy(), { reducedMotion: false });
  assertTurning(world);
  await draw(busy(), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.ok(world.entities.has("busy/arc"));

  // Removing a turning spinner removes its animation with it.
  await draw(busy());
  assert.equal(world.controllers.size, 1);
  await draw(h(Entity, { id: "done" }));
  assert.equal(world.controllers.size, 0);
  assert.equal(world.entities.has("busy"), false);
  assert.equal(world.entities.has("busy/arc"), false);
});

const ringProgress = (props: Partial<CircularProgressProps>) =>
  h(CircularProgress, { id: "upload", label: "Uploading", ...props });

test("CircularProgress draws the reported fraction from twelve o'clock and its outcomes", async () => {
  const { world, draw } = await render(ringProgress({ value: 0.656 }));
  const layout = world.fields("upload", "GuiLayout");
  assert.equal(layout.get("kind"), 2);
  assert.equal(layout.get("width"), 128);
  assert.equal(layout.get("height"), 128 + 8 + 24);
  assert.deepEqual(world.children("upload"), ["upload/ring", "upload/caption"]);
  assert.equal(world.skin("upload/ring"), "arcTrack");
  assert.equal(world.fields("upload/ring", "GuiLayout").get("width"), 128);
  assert.equal(world.fields("upload/ring", "GuiLayout").get("align_x"), 0);
  assert.deepEqual(skinRows(world, "upload/ring"), ring(12, 1));
  assert.deepEqual(world.children("upload/ring"), [
    "upload/value",
    "upload/lead",
    "upload/readout",
  ]);
  // The lit arc shares the ring with the leading arc just ahead of it.
  assert.equal(world.skin("upload/value"), "arcAccent");
  const reached = 0.656 * (1 - LEAD_LARGE);
  assert.deepEqual(skinRows(world, "upload/value"), ring(12, reached));
  assert.equal(world.skin("upload/lead"), "arcAccent");
  assert.deepEqual(
    skinRows(world, "upload/lead"),
    ring(12, LEAD_LARGE, reached, true),
  );
  // The readout is floored, so it never runs ahead of the report.
  const readout = world.fields("upload/readout", "CanvasText");
  assert.equal(readout.get("text"), "65%");
  assert.equal(readout.get("font_size"), 24);
  assert.deepEqual(world.tone("upload/readout"), TOKENS.text);
  assert.equal(world.fields("upload/readout", "GuiLayout").get("align_x"), 0);
  // The caption centres under the ring and hugs the task.
  const caption = world.fields("upload/caption", "GuiLayout");
  assert.equal(caption.get("align_x"), 0);
  assert.equal(caption.get("margin_top"), 8);
  assert.ok(
    Math.abs((caption.get("width") as number) - (9 * 0.54 * 16 + 0.16)) < 1e-9,
  );
  assert.deepEqual(world.children("upload/caption"), [
    "upload/caption/strut",
    "upload/label",
  ]);
  assert.equal(
    world.fields("upload/label", "CanvasText").get("text"),
    "Uploading",
  );
  // Read-only: no control; only the leading arc's pulse moves.
  assert.equal(world.entity("upload").components.has("GuiButton"), false);
  assert.equal(world.entity("upload").components.has("GuiBehavior"), false);
  assertPulsing(world);

  await draw(ringProgress({ value: 0.999 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "99%");
  await draw(ringProgress({ value: 0.57 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "57%");

  // Zero is the bare track, the leading arc at twelve o'clock.
  await draw(ringProgress({ value: 0 }));
  assert.equal(world.entities.has("upload/value"), false);
  assert.deepEqual(
    skinRows(world, "upload/lead"),
    ring(12, LEAD_LARGE, 0, true),
  );
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "0%");
  await draw(ringProgress({ value: 0 }), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.ok(world.entities.has("upload/lead"));

  // Complete: the whole ring lit, the check in the centre, 100% beside the task.
  await draw(ringProgress({ status: "complete" }));
  assert.equal(world.entities.has("upload/lead"), false);
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "upload/value"), ring(12, 1));
  assert.equal(world.skin("upload/value"), "arcAccent");
  assert.equal(world.entities.has("upload/readout"), false);
  assert.equal(world.skin("upload/symbol"), "checkLit");
  const check = world.fields("upload/symbol", "GuiLayout");
  assert.equal(check.get("width"), 48);
  // The checkbox's 16-unit mark drawn three times larger, stroke and all.
  assert.equal(world.fields("upload/symbol", "GuiFont").get("font_size"), 48);
  assert.equal(check.get("align_x"), 0);
  assert.equal(check.get("align_y"), 0);
  assert.equal(
    world.fields("upload/outcome", "CanvasText").get("text"),
    "100%",
  );
  assert.deepEqual(world.tone("upload/outcome"), TOKENS.accent);
  assert.equal(
    world.fields("upload/outcome", "GuiLayout").get("margin_left"),
    8,
  );
  assert.ok(
    Math.abs(
      (world.fields("upload/caption", "GuiLayout").get("width") as number) -
        (13 * 0.54 * 16 + 8 + 0.16),
    ) < 1e-9,
  );

  // Failed: the reached arc in the error colour, the error icon in its centre.
  await draw(ringProgress({ value: 0.4, status: "failed" }));
  assert.equal(world.entities.has("upload/lead"), false);
  assert.equal(world.skin("upload/value"), "arcError");
  assert.deepEqual(skinRows(world, "upload/value"), ring(12, 0.4));
  assert.equal(
    world.fields("upload/symbol", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.deepEqual(world.tone("upload/symbol"), TOKENS.error);
  assert.ok(
    Math.abs(
      (world.fields("upload/symbol", "CanvasText").get("font_size") as number) -
        32 / 0.54,
    ) < 1e-9,
  );
  assert.equal(
    world.fields("upload/outcome", "CanvasText").get("text"),
    "Failed",
  );
  assert.deepEqual(world.tone("upload/outcome"), TOKENS.error);
  assert.equal(world.controllers.size, 0);
});

test("an idle ring is the quiet track alone above its task", async () => {
  const { world, draw } = await render(
    ringProgress({ label: "PULSE", size: "small", idle: true, value: 0.5 }),
  );
  assert.equal(world.skin("upload/ring"), "arcTrack");
  assert.deepEqual(skinRows(world, "upload/ring"), ring(8, 1));
  // No arc, no leading cue and no percentage, so nothing moves.
  assert.deepEqual(world.children("upload/ring"), []);
  assert.equal(world.controllers.size, 0);
  assert.equal(world.fields("upload/label", "CanvasText").get("text"), "PULSE");
  // An outcome given with it is not shown either.
  await draw(
    ringProgress({
      label: "PULSE",
      size: "small",
      idle: true,
      status: "complete",
    }),
  );
  assert.deepEqual(world.children("upload/ring"), []);
  assert.equal(world.entities.has("upload/outcome"), false);
  // Running again draws the arc and the percentage.
  await draw(ringProgress({ label: "PULSE", size: "small", value: 0.5 }));
  assert.deepEqual(world.children("upload/ring"), [
    "upload/value",
    "upload/lead",
    "upload/readout",
  ]);
});

test("a ring of unknown total turns the spinner's quarter and shows no percentage", async () => {
  const { world, draw } = await render(
    ringProgress({ label: "Scanning", size: "small" }),
  );
  const layout = world.fields("upload", "GuiLayout");
  // The caption is wider than the small ring, so it sets the width.
  assert.ok(
    Math.abs((layout.get("width") as number) - (8 * 0.54 * 16 + 0.16)) < 1e-9,
  );
  assert.equal(layout.get("height"), 64 + 8 + 24);
  assert.equal(world.fields("upload/ring", "GuiLayout").get("width"), 64);
  assert.deepEqual(skinRows(world, "upload/ring"), ring(8, 1));
  assert.deepEqual(world.children("upload/ring"), ["upload/value"]);
  assert.deepEqual(skinRows(world, "upload/value"), ring(8, 0.25));
  assert.deepEqual(world.children("upload/caption"), [
    "upload/caption/strut",
    "upload/label",
  ]);
  assertTurning(world);

  await draw(ringProgress({ label: "Scanning", size: "small" }), {
    reducedMotion: true,
  });
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "upload/value"), ring(8, 0.25));

  // A report replaces the turning arc with the leading arc's pulse.
  await draw(ringProgress({ size: "small", value: 0.5 }));
  assertPulsing(world);
  assert.deepEqual(
    skinRows(world, "upload/value"),
    ring(8, 0.5 * (1 - LEAD_SMALL)),
  );
  assert.deepEqual(
    skinRows(world, "upload/lead"),
    ring(8, LEAD_SMALL, 0.5 * (1 - LEAD_SMALL), true),
  );
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "50%");
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("font_size"),
    16,
  );

  await draw(ringProgress({ size: "small" }));
  assertTurning(world);
  await draw(h(Entity, { id: "done" }));
  assert.equal(world.controllers.size, 0);
});

/** A theme's row at paint key `part`. */
function themeRow(world: KitWorld, name: string, part: number) {
  return themeRows(world, name).rows.find(([, row]) => row.part === part)?.[1];
}

/** Paint keys of the test contract's checked variants. */
const checkedKey = (part: number, state: keyof typeof STATE) =>
  part + STATE[state] + CHECKED;

const axisOptions: RadioGroupProps["options"] = [
  { value: "x", label: "X" },
  { value: "y", label: "Y" },
  { value: "z", label: "Z", disabled: true },
];

const axis = (props: Partial<RadioGroupProps> = {}) =>
  h(RadioGroup, {
    id: "axis",
    label: "AXIS",
    options: axisOptions,
    ...props,
  });

test("RadioGroup is a labelled group of radio marks whose arrows select", async () => {
  const { world, draw } = await render(axis({ defaultValue: "x" }), {
    fontSize: 32,
  });
  // A column of the caption and the options, at twice the design size.
  const root = world.fields("axis", "GuiLayout");
  assert.equal(root.get("kind"), 2);
  assert.equal(root.get("height"), 2 * (24 + 3 * 32));
  assert.deepEqual(world.children("axis"), ["axis/caption", "axis/options"]);
  assert.deepEqual(world.tone("axis/caption/text"), TOKENS.accent);
  // One group whose arrows move focus and select.
  const group = world.fields("axis/options", "GuiGroup");
  assert.equal(group.get("axis"), 1);
  assert.equal(group.get("selection"), 2);
  assert.deepEqual(world.children("axis/options"), [
    "axis/x",
    "axis/y",
    "axis/z",
  ]);
  // Each option is its mark, an icon-sized Button, beside its label.
  const option = world.fields("axis/x", "GuiLayout");
  assert.equal(option.get("height"), 64);
  near(option.get("width"), 48 + 16 + (0.54 * 32 + 0.32));
  assert.deepEqual(world.children("axis/x"), ["axis/x/strut", "axis/x/mark"]);
  for (const [value, selected, enabled] of [
    ["x", true, true],
    ["y", false, true],
    ["z", false, false],
  ] as const) {
    const mark = `axis/${value}/mark`;
    assert.equal(world.skin(mark), "radio");
    assert.equal(world.fields(mark, "GuiLayout").get("width"), 48);
    assert.equal(world.fields(mark, "GuiButton").get("label"), "");
    assert.equal(world.fields(mark, "GuiButton").get("selected"), selected);
    const behavior = world.fields(mark, "GuiBehavior");
    assert.equal(behavior.get("semantic_label"), value.toUpperCase());
    assert.equal(behavior.get("enabled"), enabled);
    // The label is a pointer-only Button inside the mark, so it is part of
    // the option's item. Its margins place it beside the mark, as wide as
    // its text, while its outer box stays the mark's width, which is all
    // the room the mark leaves a child: a long label neither overlaps the
    // mark nor loses presses past the mark's width.
    const label = `axis/${value}/label`;
    assert.deepEqual(world.children(mark), [label]);
    assert.equal(world.skin(label), "radioLabel");
    assert.equal(
      world.fields(label, "GuiButton").get("label"),
      value.toUpperCase(),
    );
    assert.equal(world.fields(label, "GuiBehavior").get("focusable"), false);
    const labelLayout = world.fields(label, "GuiLayout");
    const text = 0.54 * 32 + 0.32;
    near(labelLayout.get("width"), text);
    assert.equal(labelLayout.get("margin_left"), 48 + 16);
    near(labelLayout.get("margin_right"), -(16 + text));
  }

  // The mark: a circle round a dark interior, its focus ring on the same
  // circle; selected, a lit ring and a filled dot.
  assert.deepEqual(themeRow(world, "radio", PART.background), {
    part: PART.background,
    corner_cut: [0, 0, 0, 0],
    corner_radius: [12, 12],
  });
  assert.deepEqual(themeRow(world, "radio", PART.focusRing), {
    part: PART.focusRing,
    corner_cut: [0, 0, 0, 0],
    corner_radius: [12, 12],
  });
  assert.deepEqual(themeRow(world, "radio", PART.icon), {
    part: PART.icon,
    shape: 2,
    border_width: TOKENS.icon,
    color: [0, 0, 0, 0],
  });
  for (const [state, color] of [
    ["idle", TOKENS.accent],
    ["hovered", TOKENS.accent],
    ["pressed", TOKENS.surface],
    ["disabled", TOKENS.neutral],
  ] as const)
    assert.deepEqual(
      themeRow(world, "radio", checkedKey(PART.icon, state))?.color,
      color,
      state,
    );
  assert.deepEqual(
    themeRow(world, "radio", checkedKey(PART.background, "idle"))?.color,
    TOKENS.surface,
  );
  // The label paints nothing but its text, neutral while disabled.
  assert.deepEqual(themeRows(world, "radioLabel").rows, [
    [
      0,
      {
        part: PART.background,
        color: [0, 0, 0, 0],
        border_width: 0,
        glow_intensity: 0,
      },
    ],
    [1, { part: PART.label, color: TOKENS.text }],
    [2, { part: PART.label + STATE.disabled, color: TOKENS.neutral }],
  ]);

  // In a row, options follow each other an inset apart, without a caption.
  await draw(
    h(RadioGroup, {
      id: "axis",
      options: axisOptions,
      defaultValue: "x",
      horizontal: true,
    }),
  );
  assert.deepEqual(world.children("axis"), ["axis/options"]);
  assert.equal(world.fields("axis/options", "GuiLayout").get("kind"), 1);
  assert.equal(world.fields("axis/options", "GuiGroup").get("axis"), 0);
  assert.equal(world.fields("axis/y", "GuiLayout").get("margin_left"), 32);
});

test("a choice reports the runtime's selection and writes only the application's own", async () => {
  const changes: string[] = [];
  const onChange = (value: string) => changes.push(value);
  const { world, root, draw } = await render(axis({ value: "x", onChange }));
  await settle(root);

  // Arrows in the group select Y: the runtime writes both fields.
  world.value("axis/x/mark", "GuiButton", { selected: false });
  world.value("axis/y/mark", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["y"]);
  // The application follows; nothing is written back, so a later arrow is
  // never undone by a stale write.
  await draw(axis({ value: "y", onChange }));
  await settle(root);
  assert.deepEqual(world.compared, []);
  assert.equal(world.fields("axis/x/mark", "GuiButton").get("selected"), true);
  assert.equal(world.fields("axis/y/mark", "GuiButton").get("selected"), false);

  // A value of the application's own is written to its item, and the
  // runtime's echo of it is not reported back.
  await draw(axis({ value: "x", onChange }));
  await settle(root);
  assert.deepEqual(world.compared, [["axis/x/mark", "selected", true]]);
  world.value("axis/x/mark", "GuiButton", { selected: true });
  world.value("axis/y/mark", "GuiButton", { selected: false });
  await settle(root);
  assert.deepEqual(changes, ["y"]);

  // A click on a label selects its option through the mark.
  world.effect("axis/y/label", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(world.compared.at(-1), ["axis/y/mark", "selected", true]);

  // Uncontrolled, the group keeps the reported selection itself.
  const own: string[] = [];
  const free = await render(
    axis({ defaultValue: "y", onChange: (value) => own.push(value) }),
  );
  await settle(free.root);
  free.world.value("axis/x/mark", "GuiButton", { selected: true });
  await settle(free.root);
  assert.deepEqual(own, ["x"]);
  assert.deepEqual(free.world.compared, []);
});

const space = (props: Partial<SegmentedControlProps> = {}) =>
  h(SegmentedControl, {
    id: "space",
    options: [
      { value: "world", label: "World" },
      { value: "local", label: "Local" },
      { value: "view", label: "View", disabled: true },
    ],
    defaultValue: "world",
    ...props,
  });

test("SegmentedControl shares one frame among segments cut only at its ends", async () => {
  const changes: string[] = [];
  const { world, root } = await render(
    space({ onChange: (value) => changes.push(value) }),
  );
  const frame = world.fields("space", "GuiLayout");
  assert.equal(frame.get("kind"), 1);
  assert.equal(frame.get("height"), 40);
  assert.equal(world.skin("space"), "frame");
  assert.equal(world.fields("space", "GuiGroup").get("axis"), 0);
  assert.equal(world.fields("space", "GuiGroup").get("selection"), 2);
  assert.deepEqual(world.children("space"), [
    "space/world",
    "space/local",
    "space/view",
  ]);
  for (const [value, theme, selected, enabled] of [
    ["world", "segmentFirst", true, true],
    ["local", "segment", false, true],
    ["view", "segmentLast", false, false],
  ] as const) {
    const segment = `space/${value}`;
    assert.equal(world.skin(segment), theme);
    assert.equal(world.fields(segment, "GuiLayout").get("flex"), 1);
    assert.equal(world.fields(segment, "GuiLayout").get("height"), 40);
    assert.equal(world.fields(segment, "GuiButton").get("selected"), selected);
    assert.equal(world.fields(segment, "GuiBehavior").get("enabled"), enabled);
  }
  assert.equal(world.fields("space/local", "GuiButton").get("label"), "Local");

  // Clear at rest; a segment after another starts with a quiet line; the
  // selected one fills solid; the frame's cut on the outer corners only.
  const base = (theme: string) => themeRow(world, theme, PART.background)!;
  assert.deepEqual(base("segmentFirst").corner_cut, [8, 0, 0, 0]);
  assert.equal(base("segmentFirst").fill_mode, 0);
  assert.deepEqual(base("segment").corner_cut, [0, 0, 0, 0]);
  assert.deepEqual(base("segmentLast").corner_cut, [0, 0, 8, 0]);
  assert.deepEqual(base("segmentOnly").corner_cut, [8, 0, 8, 0]);
  assert.deepEqual(
    {
      ...base("segment"),
      part: undefined,
    },
    {
      part: undefined,
      color: [0, 0, 0, 0],
      border_width: 0,
      corner_cut: [0, 0, 0, 0],
      fill_mode: 1,
      gradient_start: [1.25, 0],
      gradient_end: [1.26, 0],
      gradient_color0: TOKENS.line,
      gradient_color1: [0, 0, 0, 0],
    },
  );
  for (const state of ["idle", "hovered", "pressed", "disabled"] as const)
    assert.equal(
      themeRow(world, "segment", checkedKey(PART.background, state))?.fill_mode,
      0,
    );
  assert.deepEqual(
    themeRow(world, "segmentLast", PART.focusRing)?.corner_cut,
    [0, 0, 8, 0],
  );

  // Arrows select: the runtime's selection reaches onChange.
  await settle(root);
  world.value("space/local", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["local"]);
});

const navigation = (props: Partial<TabsProps> = {}) =>
  h(Tabs, {
    id: "nav",
    tabs: [
      {
        value: "overview",
        label: "Overview",
        content: h(Entity, { id: "overview-body" }),
      },
      {
        value: "signals",
        label: "Signals",
        content: h(Entity, { id: "signals-body" }),
      },
      { value: "events", label: "Events", disabled: true },
    ],
    defaultValue: "signals",
    more: h(Entity, { id: "more" }),
    ...props,
  });

/** A ScrollView's runtime-written geometry. */
const scrollGeometry = (
  offset: number,
  viewport: number,
  capacity: number,
) => ({
  offset_x: offset,
  offset_y: 0,
  viewport_x: viewport,
  viewport_y: 40,
  content_x: viewport + capacity,
  content_y: 40,
  capacity_x: capacity,
  capacity_y: 0,
});

test("Tabs: a strip whose activation selects the tab whose content shows", async () => {
  const changes: string[] = [];
  const { world, root } = await render(
    navigation({ onChange: (value) => changes.push(value) }),
  );
  assert.deepEqual(world.children("nav"), [
    "nav/strip",
    "nav/line",
    "nav/content",
  ]);
  assert.equal(world.fields("nav/strip", "GuiLayout").get("height"), 40);
  // Without overflow the strip is its scroll view and the More slot.
  assert.deepEqual(world.children("nav/strip"), ["nav/view", "more"]);
  const view = world.fields("nav/view", "GuiScrollView");
  assert.equal(view.get("axis"), 0);
  assert.equal(view.get("bar_thickness"), 0);
  assert.equal(world.skin("nav/view"), "gridBody");
  assert.equal(world.fields("nav/view", "GuiLayout").get("flex"), 1);
  // One group whose activation selects, of tabs hugging their labels.
  const group = world.fields("nav/tabs", "GuiGroup");
  assert.equal(group.get("axis"), 0);
  assert.equal(group.get("selection"), 1);
  const widths = ["Overview", "Signals", "Events"].map(
    (label) => label.length * 0.54 * 16 + 0.16 + 32,
  );
  near(
    world.fields("nav/tabs", "GuiLayout").get("width"),
    widths.reduce((sum, width) => sum + width, 0),
  );
  for (const [value, selected, enabled] of [
    ["overview", false, true],
    ["signals", true, true],
    ["events", false, false],
  ] as const) {
    const tab = `nav/tab/${value}`;
    assert.equal(world.skin(tab), "tab");
    assert.equal(world.fields(tab, "GuiButton").get("selected"), selected);
    assert.equal(world.fields(tab, "GuiBehavior").get("enabled"), enabled);
  }
  near(world.fields("nav/tab/signals", "GuiLayout").get("width"), widths[1]!);
  // Only the selected tab's content is declared.
  assert.deepEqual(world.children("nav/content"), ["signals-body"]);
  assert.equal(world.entities.has("overview-body"), false);
  assert.equal(world.skin("nav/line"), "division");

  // Selected: the accent bar along the bottom edge and a lit label.
  assert.deepEqual(
    themeRow(world, "tab", checkedKey(PART.background, "idle")),
    {
      part: checkedKey(PART.background, "idle"),
      fill_mode: 1,
      gradient_start: [0, 36],
      gradient_end: [0, 36.01],
      gradient_color0: [0, 0, 0, 0],
      gradient_color1: TOKENS.accent,
    },
  );
  assert.deepEqual(
    themeRow(world, "tab", checkedKey(PART.label, "idle"))?.color,
    TOKENS.accent,
  );

  // Enter on Overview selects it: the content follows.
  await settle(root);
  world.value("nav/tab/overview", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["overview"]);
  assert.deepEqual(world.children("nav/content"), ["overview-body"]);
  assert.equal(world.entities.has("signals-body"), false);

  // Overflowing, scroll buttons dock at the strip's ends, the one towards
  // the start disabled at the start; each scrolls half the strip.
  world.value("nav/view", "GuiScrollView", scrollGeometry(0, 200, 60));
  await settle(root);
  assert.deepEqual(world.children("nav/strip"), [
    "nav/previous",
    "nav/view",
    "nav/next",
    "more",
  ]);
  for (const [button, icon, enabled] of [
    ["nav/previous", GUI_KIT_ICONS.previous, false],
    ["nav/next", GUI_KIT_ICONS.next, true],
  ] as const) {
    assert.equal(world.skin(button), "dockedIcon");
    assert.equal(world.fields(button, "GuiButton").get("label"), icon);
    assert.equal(world.fields(button, "GuiBehavior").get("focusable"), false);
    assert.equal(world.fields(button, "GuiBehavior").get("enabled"), enabled);
    assert.equal(world.fields(button, "GuiLayout").get("width"), 32);
    assert.equal(world.fields(button, "GuiFont").get("font_size"), 24);
  }
  world.effect("nav/next", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(world.actions.at(-1), [
    "nav/view",
    { kind: "scrollBy", delta: [100, 0] },
  ]);
  world.value("nav/view", "GuiScrollView", scrollGeometry(60, 200, 60));
  await settle(root);
  assert.equal(world.fields("nav/next", "GuiBehavior").get("enabled"), false);
  assert.equal(
    world.fields("nav/previous", "GuiBehavior").get("enabled"),
    true,
  );
});

const sceneNodes: readonly TreeNode[] = [
  {
    key: "scene",
    label: "Scene",
    icon: "\u{f024b}",
    children: [
      {
        key: "camera",
        label: "Camera",
        children: [{ key: "lens", label: "Lens" }],
      },
      {
        key: "lighting",
        label: "Lighting",
        children: [
          { key: "key", label: "Key" },
          { key: "fill", label: "Fill" },
        ],
      },
      { key: "cube", label: "Cube", disabled: true },
    ],
  },
];

test("TreeView declares visible rows; unhandled Right and Left expand, collapse and move focus", async () => {
  const handle: { current: TreeViewHandle | null } = { current: null };
  const expansions: (readonly string[])[] = [];
  const selections: string[] = [];
  const tree = (props: Partial<TreeViewProps> = {}) =>
    h(TreeView, {
      id: "tree",
      nodes: sceneNodes,
      defaultExpanded: ["scene", "lighting"],
      onExpandedChange: (keys) => expansions.push(keys),
      onChange: (key) => selections.push(key),
      ref: handle,
      ...props,
    });
  const { world, root } = await render(tree());
  await settle(root);
  const rows = () => world.children("tree/rows");
  assert.deepEqual(rows(), [
    "tree/row/scene",
    "tree/row/camera",
    "tree/row/lighting",
    "tree/row/key",
    "tree/row/fill",
    "tree/row/cube",
  ]);
  // The rows are one group whose activation selects, in a list frame.
  assert.ok(world.entity("tree").components.has("GuiScrollView"));
  assert.equal(world.entity("tree").components.has("GuiSkin"), false);
  assert.equal(world.fields("tree/rows", "GuiGroup").get("axis"), 1);
  assert.equal(world.fields("tree/rows", "GuiGroup").get("selection"), 1);
  assert.equal(
    world.fields("tree/rows", "GuiLayout").get("height"),
    6 * 36 + 16,
  );
  // Indented a chevron column and a gap per level; the grid's row look.
  for (const [key, depth] of [
    ["scene", 0],
    ["camera", 1],
    ["key", 2],
  ] as const) {
    const row = `tree/row/${key}`;
    assert.equal(world.skin(row), "gridRow");
    assert.equal(world.fields(row, "GuiLayout").get("height"), 36);
    assert.equal(
      world.fields(row, "GuiLayout").get("padding_left"),
      8 + depth * 32,
    );
    assert.equal(world.fields(row, "GuiButton").get("selected"), false);
  }
  // Branches carry a pointer-only chevron Button, leaves its empty column.
  assert.deepEqual(world.children("tree/row/scene"), [
    "tree/row/scene/strut",
    "tree/row/scene/chevron",
    "tree/row/scene/icon",
    "tree/row/scene/label",
  ]);
  assert.equal(
    world.fields("tree/row/scene/chevron", "GuiButton").get("label"),
    GUI_KIT_ICONS.expanded,
  );
  assert.equal(
    world.fields("tree/row/camera/chevron", "GuiButton").get("label"),
    GUI_KIT_ICONS.collapsed,
  );
  assert.equal(world.skin("tree/row/camera/chevron"), "treeChevron");
  assert.equal(
    world.fields("tree/row/camera/chevron", "GuiBehavior").get("focusable"),
    false,
  );
  assert.deepEqual(world.children("tree/row/key"), [
    "tree/row/key/strut",
    "tree/row/key/leaf",
    "tree/row/key/label",
  ]);
  assert.equal(world.fields("tree/row/key/leaf", "GuiLayout").get("width"), 24);
  assert.equal(
    world.fields("tree/row/scene/icon", "CanvasText").get("text"),
    "\u{f024b}",
  );
  assert.equal(
    world.fields("tree/row/cube", "GuiBehavior").get("enabled"),
    false,
  );
  assert.deepEqual(world.tone("tree/row/cube/label"), TOKENS.neutral);

  // Keys reach the tree only while one of its rows holds focus.
  assert.equal(handle.current!.key("right"), false);
  const focus = (key: string, focused = true) =>
    world.effect(`tree/row/${key}`, {
      kind: "focusChanged",
      focused,
      changed: true,
      part: 0,
    });
  focus("camera");
  await settle(root);
  // Right expands a collapsed branch, then moves to its first child.
  assert.equal(handle.current!.key("right"), true);
  await settle(root);
  assert.deepEqual(expansions.at(-1), ["scene", "lighting", "camera"]);
  assert.deepEqual(rows().slice(1, 3), ["tree/row/camera", "tree/row/lens"]);
  assert.equal(
    world.fields("tree/row/lens", "GuiLayout").get("padding_left"),
    8 + 2 * 32,
  );
  assert.equal(handle.current!.key("right"), true);
  await settle(root);
  assert.deepEqual(world.actions.at(-1), ["tree/row/lens", { kind: "focus" }]);
  // Left on a leaf moves to its parent; on an expanded branch it collapses.
  focus("camera", false);
  focus("lens");
  await settle(root);
  assert.equal(handle.current!.key("right"), false);
  assert.equal(handle.current!.key("left"), true);
  await settle(root);
  assert.deepEqual(world.actions.at(-1), [
    "tree/row/camera",
    { kind: "focus" },
  ]);
  focus("lens", false);
  focus("camera");
  await settle(root);
  assert.equal(handle.current!.key("left"), true);
  await settle(root);
  assert.equal(world.entities.has("tree/row/lens"), false);
  assert.deepEqual(expansions.at(-1), ["scene", "lighting"]);
  // Left on a collapsed top-level branch has nowhere to go.
  focus("camera", false);
  focus("scene");
  await settle(root);
  assert.equal(handle.current!.key("up"), false);

  // A chevron collapses its branch without selecting; focus inside it moves
  // to the collapsed node.
  focus("scene", false);
  focus("fill");
  await settle(root);
  const actions = world.actions.length;
  world.effect("tree/row/lighting/chevron", { kind: "pressed" });
  await settle(root);
  assert.equal(world.entities.has("tree/row/fill"), false);
  assert.deepEqual(world.actions.slice(actions), [
    ["tree/row/lighting", { kind: "focus" }],
  ]);
  assert.deepEqual(world.compared, []);

  // Selection is the runtime's, reported once; it stays with a hidden row.
  world.value("tree/row/camera", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(selections, ["camera"]);
});

test("the floating surface and menu rows: frames at rest, the active row tinted under the hover edge", async () => {
  const { world } = await render(h(Entity, { id: "empty" }));
  const frame = (cut: number) => [
    [
      PART.background,
      {
        part: PART.background,
        color: TOKENS.surface,
        border_width: TOKENS.lineWidth,
        border_color: TOKENS.neutral,
        corner_cut: [cut, 0, cut, 0],
      },
    ],
  ];
  assert.deepEqual(themeRows(world, "floating").rows, frame(TOKENS.cut));
  assert.deepEqual(themeRows(world, "floatingSmall").rows, frame(4));
  // A row paints nothing at rest; active and pressed, the row tint under
  // the docked look's hover edge; selected, a data grid row's bar and tint.
  assert.deepEqual(themeRow(world, "menuRow", PART.background), {
    part: PART.background,
    color: [0, 0, 0, 0],
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
  });
  assert.deepEqual(themeRow(world, "menuRow", HOVERED), {
    part: HOVERED,
    color: TOKENS.rowTint,
  });
  assert.deepEqual(themeRow(world, "menuRow", PRESSED), {
    part: PRESSED,
    color: TOKENS.rowTint,
  });
  const bar = TOKENS.selectionGutter + TOKENS.lineWidth / 2;
  assert.deepEqual(
    themeRow(world, "menuRow", checkedKey(PART.background, "hovered")),
    {
      part: checkedKey(PART.background, "hovered"),
      fill_mode: 1,
      gradient_start: [bar, 0],
      gradient_end: [bar + 0.01, 0],
      gradient_color0: TOKENS.accent,
      gradient_color1: TOKENS.rowTint,
    },
  );
  assert.deepEqual(
    themeRow(world, "menuRow", checkedKey(PART.background, "disabled"))
      ?.gradient_color0,
    TOKENS.neutral,
  );
  // The amber row: the amber variant, square, its active row tinted amber.
  assert.deepEqual(themeRow(world, "menuRowAmber", PART.background), {
    part: PART.background,
    color: [0, 0, 0, 0],
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
    border_color: TOKENS.amber,
  });
  assert.deepEqual(themeRow(world, "menuRowAmber", HOVERED), {
    part: HOVERED,
    color: [...TOKENS.amber.slice(0, 3), TOKENS.rowTint[3]],
  });
});

const COMMANDS: readonly MenuItem[] = [
  { key: "inspect", label: "Inspect", icon: "\u{f0214}" },
  { key: "duplicate", label: "Duplicate", icon: "\u{f018f}" },
  {
    key: "delete",
    label: "Delete",
    icon: "\u{f0a7a}",
    tone: "amber",
    separator: true,
  },
  { key: "unavailable", label: "Unavailable", disabled: true },
];

/**
 * A trigger whose light overlay holds a menu, as an application composes
 * them: the trigger toggles the overlay, the overlay follows the runtime's
 * writes, and activation closes it.
 */
function MenuHarness({
  log,
  ...open
}: OverlayOpenProps & { readonly log: unknown[] }) {
  const overlay = useOverlayOpen({
    ...open,
    onOpenChange: (value) => log.push(["open", value]),
  });
  return h(
    Entity,
    { id: "trigger" },
    h(Button, { label: "Menu", onPress: overlay.toggle }),
    h(
      Children,
      null,
      h(
        Floating,
        {
          id: "trigger/overlay",
          mode: "light",
          open: overlay.open,
          onVisibleChange: overlay.onVisibleChange,
        },
        h(Menu, {
          id: "trigger/menu",
          items: COMMANDS,
          overlay,
          onSelect: (key) => log.push(["select", key]),
        }),
      ),
    ),
  );
}

test("Menu: command rows that take no focus in a group, on a floating surface", async () => {
  const log: unknown[] = [];
  const { world } = await render(h(MenuHarness, { log, defaultOpen: true }));
  // The surface: an anchored light overlay one layer up, with the kit's
  // font, its column inside the frame's line.
  const overlay = world.fields("trigger/overlay", "GuiOverlay");
  assert.equal(overlay.get("side"), 0);
  assert.equal(overlay.get("align"), 0);
  assert.equal(overlay.get("mode"), 1);
  assert.equal(
    world.fields("trigger/overlay", "CanvasStyle").get("layer"),
    GUI_KIT_LAYERS.anchored,
  );
  assert.equal(
    world.fields("trigger/overlay", "GuiBehavior").get("visible"),
    true,
  );
  assert.equal(world.fields("trigger/overlay", "GuiFont").get("font_size"), 16);
  assert.equal(world.skin("trigger/overlay"), "floating");
  const surface = world.fields("trigger/overlay", "GuiLayout");
  assert.equal(surface.get("kind"), 2);
  assert.equal(surface.get("padding_left"), TOKENS.lineWidth);
  assert.equal(surface.get("padding_top"), TOKENS.lineWidth);

  // The list: a vertical group without selection, as wide as its longest
  // label needs, rows in half-inset margins and a separator before Delete.
  const group = world.fields("trigger/menu", "GuiGroup");
  assert.equal(group.get("axis"), 1);
  assert.equal(group.get("selection"), undefined);
  const list = world.fields("trigger/menu", "GuiLayout");
  near(list.get("width"), 8 + 8 + 8 + 24 + 8 + 11 * 0.54 * 16 + 0.16 + 16);
  near(list.get("height"), 16 + 4 * 36 + TOKENS.lineWidth + 8);
  assert.equal(list.get("padding_top"), 8);
  assert.deepEqual(world.children("trigger/menu"), [
    "trigger/menu/inspect",
    "trigger/menu/duplicate",
    "trigger/menu/delete/separator",
    "trigger/menu/delete",
    "trigger/menu/unavailable",
  ]);
  const separator = world.fields("trigger/menu/delete/separator", "GuiLayout");
  assert.equal(separator.get("height"), TOKENS.lineWidth);
  assert.equal(separator.get("margin_top"), 4);
  assert.equal(separator.get("margin_left"), 8);
  near(separator.get("width"), (list.get("width") as number) - 32);
  assert.equal(world.skin("trigger/menu/delete/separator"), "division");

  for (const [key, theme, tone, enabled] of [
    ["inspect", "menuRow", TOKENS.text, true],
    ["delete", "menuRowAmber", TOKENS.amber, true],
    ["unavailable", "menuRow", TOKENS.neutral, false],
  ] as const) {
    const row = `trigger/menu/${key}`;
    assert.equal(world.skin(row), theme);
    assert.equal(world.fields(row, "GuiLayout").get("height"), 36);
    assert.equal(world.fields(row, "GuiLayout").get("padding_left"), 8);
    assert.equal(world.fields(row, "GuiButton").get("label"), "");
    const behavior = world.fields(row, "GuiBehavior");
    assert.equal(behavior.get("focusable"), false);
    assert.equal(behavior.get("enabled"), enabled);
    assert.deepEqual(world.tone(`${row}/label`), tone);
    assert.deepEqual(world.tone(`${row}/icon`), tone);
    // Every row keeps the icon column, so labels align.
    assert.equal(world.fields(`${row}/icon`, "GuiLayout").get("width"), 24);
  }
  assert.deepEqual(world.children("trigger/menu/inspect"), [
    "trigger/menu/inspect/strut",
    "trigger/menu/inspect/icon",
    "trigger/menu/inspect/label",
  ]);
  assert.equal(
    world.fields("trigger/menu/inspect/icon", "CanvasText").get("text"),
    "\u{f0214}",
  );
  assert.equal(
    world.entity("trigger/menu/unavailable/icon").components.has("CanvasText"),
    false,
  );
  // A menu without icons has no icon column.
  const { world: plain } = await render(
    h(Menu, { id: "plain", items: [{ key: "a", label: "A" }] }),
  );
  assert.deepEqual(plain.children("plain/a"), [
    "plain/a/strut",
    "plain/a/label",
  ]);
  near(plain.fields("plain", "GuiLayout").get("width"), 160);
  assert.deepEqual(log, []);
});

test("Menu: activation closes its overlay and reports the command once", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(
    h(MenuHarness, { log, defaultOpen: true }),
  );
  // Two presses before the first one's render: one command.
  world.effect("trigger/menu/duplicate", { kind: "pressed" });
  world.effect("trigger/menu/inspect", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(log, [
    ["open", false],
    ["select", "duplicate"],
  ]);
  assert.equal(
    world.fields("trigger/overlay", "GuiBehavior").get("visible"),
    false,
  );
  // The trigger opens it again, and the next activation reports again.
  world.effect("trigger", { kind: "pressed" });
  await settle(root);
  assert.equal(
    world.fields("trigger/overlay", "GuiBehavior").get("visible"),
    true,
  );
  world.effect("trigger/menu/inspect", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(log.slice(2), [
    ["open", true],
    ["open", false],
    ["select", "inspect"],
  ]);
});

test("useOverlayOpen follows the runtime closing its overlay, not an earlier report", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(h(MenuHarness, { log }));
  const visible = () =>
    world.fields("trigger/overlay", "GuiBehavior").get("visible");
  assert.equal(visible(), false);
  await settle(root);
  world.effect("trigger", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  // The field's first report arrives after the opening and still shows the
  // declared closed state: it does not close the overlay again...
  world.value("trigger/overlay", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), true);
  // ...but once the runtime reported it open, its closing is adopted, so the
  // next press on the trigger opens it rather than closing it.
  world.value("trigger/overlay", "GuiBehavior", { visible: true });
  world.value("trigger/overlay", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  world.effect("trigger", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.deepEqual(log, [
    ["open", true],
    ["open", false],
    ["open", true],
  ]);
});

/**
 * Targets that each declare their own context menu while it is open for
 * them, inside their declaration but outside its `Children`, so the menu is a
 * root of the canvas and unmounts with its target.
 */
function ContextHarness({
  log,
  targets = ["cube", "sphere"],
}: {
  readonly log: unknown[];
  readonly targets?: readonly string[];
}) {
  const menu = useContextMenu<string>();
  return h(
    Fragment,
    null,
    targets.map((target) =>
      h(
        Entity,
        { key: target, id: target },
        h(Button, { label: target, onContextMenu: menu.opener(target) }),
        menu.request?.target === target &&
          h(ContextMenu<string>, {
            id: `${target}/menu`,
            menu,
            items: COMMANDS,
            onSelect: (key, chosen) => log.push([key, chosen]),
          }),
      ),
    ),
  );
}

test("ContextMenu opens a light menu at the request's point for its target", async () => {
  const log: unknown[] = [];
  const { world, root, draw } = await render(h(ContextHarness, { log }));
  const request = (target: string, point: readonly [number, number]) =>
    world.effect(target, { kind: "contextRequested", point });
  assert.equal(world.entities.has("cube/menu"), false);
  request("cube", [40, 50]);
  await settle(root);
  // A root of the canvas at the point; the surface below it, flipping and
  // shifting to stay inside the canvas, holds the commands.
  assert.equal(world.entity("cube/menu").parent, null);
  const anchor = world.fields("cube/menu", "CanvasStyle");
  assert.equal(anchor.get("x"), 40);
  assert.equal(anchor.get("y"), 50);
  assert.equal(world.entity("cube/menu").components.has("GuiLayout"), false);
  assert.deepEqual(world.children("cube/menu"), ["cube/menu/surface"]);
  const overlay = world.fields("cube/menu/surface", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [0, 0, 1],
  );
  assert.equal(
    world.fields("cube/menu/surface", "GuiBehavior").get("visible"),
    true,
  );
  assert.deepEqual(world.children("cube/menu/surface"), ["cube/menu/items"]);
  assert.equal(world.skin("cube/menu/items/delete"), "menuRowAmber");

  // A command reports its request's target once, and the menu closes.
  world.effect("cube/menu/items/delete", { kind: "pressed" });
  world.effect("cube/menu/items/inspect", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(log, [["delete", "cube"]]);
  assert.equal(world.entities.has("cube/menu"), false);

  // The runtime closing it, on Escape or an outside press, runs nothing.
  request("cube", [10, 20]);
  await settle(root);
  assert.equal(world.fields("cube/menu", "CanvasStyle").get("x"), 10);
  world.value("cube/menu/surface", "GuiBehavior", { visible: true });
  world.value("cube/menu/surface", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(world.entities.has("cube/menu"), false);

  // Another target's request moves the menu to it; the first target's menu
  // unmounting does not close the second's.
  request("cube", [10, 20]);
  await settle(root);
  request("sphere", [70, 20]);
  await settle(root);
  assert.equal(world.entities.has("cube/menu"), false);
  assert.equal(world.fields("sphere/menu", "CanvasStyle").get("x"), 70);
  assert.equal(
    world.fields("sphere/menu/surface", "GuiBehavior").get("visible"),
    true,
  );

  // The target going away takes its menu with it and closes the request,
  // so the target's return shows no menu and nothing ran.
  await draw(h(ContextHarness, { log, targets: ["cube"] }));
  assert.equal(world.entities.has("sphere/menu"), false);
  await draw(h(ContextHarness, { log, targets: ["cube", "sphere"] }));
  assert.equal(world.entities.has("sphere/menu"), false);
  assert.deepEqual(log, [["delete", "cube"]]);
});

test("Popover: a trigger with a caret opens a titled light surface of the application's content", async () => {
  const changes: boolean[] = [];
  const popover = (open?: boolean) =>
    h(
      Popover,
      {
        id: "options",
        label: "Options",
        title: "Options",
        ...(open === undefined ? {} : { open }),
        onOpenChange: (value) => changes.push(value),
      },
      h(TextLine, { id: "options-label", text: "Label" }),
    );
  const { world, root } = await render(popover());
  // The trigger: a secondary button at the small size with a caret.
  const label = `Options ${GUI_KIT_ICONS.sortDescending}`;
  assert.equal(world.fields("options", "GuiButton").get("label"), label);
  assert.equal(world.fields("options", "GuiButton").get("selected"), false);
  assert.equal(world.skin("options"), "secondarySmall");
  near(
    world.fields("options", "GuiLayout").get("width"),
    [...label].length * 0.54 * 13 + 32,
  );
  assert.equal(world.fields("options", "GuiLayout").get("height"), 32);
  // The surface below it, a quarter inset away, its header strip with the
  // title and a close button that takes no focus, then the content.
  assert.deepEqual(world.children("options"), ["options/popover"]);
  const overlay = world.fields("options/popover", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [0, 1, 1],
  );
  assert.equal(world.fields("options/popover", "CanvasStyle").get("y"), 4);
  assert.equal(world.fields("options/popover", "GuiLayout").get("width"), 240);
  assert.equal(world.skin("options/popover"), "floating");
  assert.deepEqual(world.children("options/popover"), [
    "options/header",
    "options/header/separator",
    "options/content",
  ]);
  assert.deepEqual(world.tone("options/header/title"), TOKENS.accent);
  const close = world.fields("options/close", "GuiBehavior");
  assert.equal(close.get("focusable"), false);
  assert.equal(world.skin("options/close"), "dockedIcon");
  assert.equal(
    world.fields("options/content", "GuiLayout").get("padding_left"),
    16,
  );
  assert.deepEqual(world.children("options/content"), ["options-label"]);
  const visible = () =>
    world.fields("options/popover", "GuiBehavior").get("visible");
  assert.equal(visible(), false);

  // The trigger toggles it and takes its open look; the close button and
  // the runtime close it.
  await settle(root);
  world.effect("options", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.equal(world.fields("options", "GuiButton").get("selected"), true);
  world.effect("options/close", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), false);
  world.effect("options", { kind: "pressed" });
  await settle(root);
  world.value("options/popover", "GuiBehavior", { visible: true });
  world.value("options/popover", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  assert.equal(world.fields("options", "GuiButton").get("selected"), false);
  assert.deepEqual(changes, [true, false, true, false]);
});

test("Tooltip: a closed hint of its control on the small surface", async () => {
  const { world } = await render(
    h(
      Entity,
      { id: "help" },
      h(Button, { label: "?" }),
      h(
        Children,
        null,
        h(Tooltip, {
          id: "help/tip",
          text: ["Gain controls signal", "strength."],
        }),
      ),
    ),
  );
  const overlay = world.fields("help/tip", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [1, 1, 3],
  );
  // Declared closed and never followed: the runtime opens and closes it.
  assert.equal(world.fields("help/tip", "GuiBehavior").get("visible"), false);
  assert.equal(world.skin("help/tip"), "floatingSmall");
  assert.equal(world.fields("help/tip", "CanvasStyle").get("y"), -4);
  const layout = world.fields("help/tip", "GuiLayout");
  near(layout.get("width"), 20 * 0.54 * 13 + 0.13 + 16);
  assert.equal(layout.get("padding_top"), 4);
  assert.equal(layout.get("padding_left"), 8);
  assert.deepEqual(world.children("help/tip"), [
    "help/tip/line/0",
    "help/tip/line/1",
  ]);
  assert.equal(world.fields("help/tip/line/1", "GuiLayout").get("height"), 24);
  assert.equal(
    world.fields("help/tip/line/1/text", "CanvasText").get("font_size"),
    13,
  );
  // Beside its control, the offset follows the side.
  const { world: side } = await render(
    h(
      Entity,
      { id: "help" },
      h(Button, { label: "?" }),
      h(
        Children,
        null,
        h(Tooltip, { id: "help/tip", text: "Gain", side: "left" }),
      ),
    ),
  );
  assert.equal(side.fields("help/tip", "GuiOverlay").get("side"), 3);
  assert.equal(side.fields("help/tip", "CanvasStyle").get("x"), -4);
});

test("ConfirmationDialog: a modal centred dialog whose answer is reported once per opening", async () => {
  const answers: string[] = [];
  const dialog = (props: Partial<ConfirmationDialogProps> = {}) =>
    h(ConfirmationDialog, {
      id: "confirm",
      open: true,
      title: "Delete node?",
      body: ["Delete Cube from the scene?", "This action cannot be undone."],
      action: "Delete",
      onConfirm: () => answers.push("confirm"),
      onCancel: () => answers.push("cancel"),
      ...props,
    });
  const { world, root, draw } = await render(dialog());
  const overlay = world.fields("confirm", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [4, 1, 2],
  );
  assert.equal(
    world.fields("confirm", "CanvasStyle").get("layer"),
    GUI_KIT_LAYERS.dialog,
  );
  assert.equal(world.fields("confirm", "GuiLayout").get("width"), 368);
  assert.equal(world.fields("confirm", "GuiFont").get("font_size"), 16);
  assert.equal(world.skin("confirm"), "floating");
  assert.deepEqual(world.children("confirm"), [
    "confirm/header",
    "confirm/header/separator",
    "confirm/body",
  ]);
  // Close takes no focus, so Cancel, before the action, takes it first.
  assert.equal(
    world.fields("confirm/close", "GuiBehavior").get("focusable"),
    false,
  );
  assert.equal(
    world.fields("confirm/close", "GuiBehavior").get("semantic_label"),
    "Cancel",
  );
  assert.equal(
    world.fields("confirm/body", "GuiLayout").get("height"),
    16 + 2 * 24 + 16 + 40 + 16,
  );
  assert.deepEqual(world.children("confirm/body"), [
    "confirm/line/0",
    "confirm/line/1",
    "confirm/actions",
  ]);
  assert.deepEqual(world.children("confirm/actions"), [
    "confirm/cancel",
    "confirm/action",
  ]);
  assert.equal(world.skin("confirm/cancel"), "secondary");
  assert.equal(world.skin("confirm/action"), "amber");
  assert.equal(
    world.fields("confirm/action", "GuiButton").get("label"),
    "Delete",
  );
  for (const button of ["confirm/cancel", "confirm/action"]) {
    assert.equal(world.fields(button, "GuiLayout").get("flex"), 1);
    assert.equal(world.fields(button, "GuiLayout").get("height"), 40);
  }
  // Each flexible button gives half the gap from its own share, so the
  // two stay equal.
  assert.equal(
    world.fields("confirm/cancel", "GuiLayout").get("margin_right"),
    8,
  );
  assert.equal(
    world.fields("confirm/action", "GuiLayout").get("margin_left"),
    8,
  );
  assert.deepEqual(themeRows(world, "amber").rows[0]![1], {
    part: PART.background,
    corner_cut: [8, 0, 8, 0],
    border_color: TOKENS.amber,
  });

  // The action confirms once, however many presses arrive.
  await settle(root);
  world.effect("confirm/action", { kind: "pressed" });
  world.effect("confirm/action", { kind: "pressed" });
  world.effect("confirm/cancel", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(answers, ["confirm"]);
  await draw(dialog({ open: false }));
  assert.equal(world.fields("confirm", "GuiBehavior").get("visible"), false);

  // Opened again: the close button cancels.
  await draw(dialog());
  world.effect("confirm/close", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(answers, ["confirm", "cancel"]);
  await draw(dialog({ open: false }));

  // Opened again: Escape, the runtime closing it, cancels once; a late
  // report of the earlier closing does not.
  await draw(dialog());
  world.value("confirm", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(answers, ["confirm", "cancel"]);
  world.value("confirm", "GuiBehavior", { visible: true });
  world.value("confirm", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(answers, ["confirm", "cancel", "cancel"]);

  // Not destructive: the default primary look.
  await draw(dialog({ destructive: false }));
  assert.equal(world.entity("confirm/action").components.has("GuiSkin"), false);
});

test("Tabs: a More menu while the strip overflows selects a tab and scrolls it into view", async () => {
  const changes: string[] = [];
  const { world, root } = await render(
    navigation({
      overflowMenu: true,
      onChange: (value) => changes.push(value),
    }),
  );
  assert.equal(world.entities.has("nav/more"), false);
  await settle(root);
  world.value("nav/view", "GuiScrollView", scrollGeometry(60, 200, 60));
  await settle(root);
  assert.deepEqual(world.children("nav/strip"), [
    "nav/previous",
    "nav/view",
    "nav/next",
    "nav/more",
    "more",
  ]);
  assert.equal(world.skin("nav/more"), "dockedIcon");
  assert.equal(world.fields("nav/more", "GuiBehavior").get("focusable"), false);
  assert.equal(
    world.fields("nav/more", "GuiButton").get("label"),
    GUI_KIT_ICONS.expanded,
  );
  // Its light menu lists every tab, below it and aligned with its end.
  const overlay = world.fields("nav/more/menu", "GuiOverlay");
  assert.deepEqual([overlay.get("align"), overlay.get("mode")], [2, 1]);
  assert.deepEqual(world.children("nav/more/items"), [
    "nav/more/items/overview",
    "nav/more/items/signals",
    "nav/more/items/events",
  ]);
  assert.equal(
    world.fields("nav/more/items/events", "GuiBehavior").get("enabled"),
    false,
  );
  world.effect("nav/more", { kind: "pressed" });
  await settle(root);
  assert.equal(
    world.fields("nav/more/menu", "GuiBehavior").get("visible"),
    true,
  );
  assert.equal(world.fields("nav/more", "GuiButton").get("selected"), true);
  // Overview lies left of the scrolled strip: it is selected and scrolled to.
  world.effect("nav/more/items/overview", { kind: "pressed" });
  await settle(root);
  assert.equal(
    world.fields("nav/more/menu", "GuiBehavior").get("visible"),
    false,
  );
  assert.deepEqual(world.compared.at(-1), [
    "nav/tab/overview",
    "selected",
    true,
  ]);
  assert.deepEqual(world.actions.at(-1), [
    "nav/view",
    { kind: "scrollTo", offset: [0, 0] },
  ]);
  world.value("nav/tab/overview", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["overview"]);
});

test("selection looks: the trigger's open edge, its chevrons and the check mark of options that toggle", async () => {
  // A contract whose button look has a hover edge, which the trigger's open
  // look reuses.
  const edge = {
    part: HOVERED,
    border_width: TOKENS.litLineWidth,
    border_color: TOKENS.accent,
    glow_intensity: 0.02,
  };
  const base = contract();
  const world = new KitWorld();
  const root = createRoot(world);
  await root.render(
    h(
      GuiKit,
      {
        contract: {
          ...base,
          GUI_SKIN_LOOKS: {
            ...base.GUI_SKIN_LOOKS,
            button: { em: 16, parts: [edge] },
          },
        },
        font: FONT,
        fontSize: 16,
      },
      h(Entity, { id: "empty" }),
    ),
  );
  // Pressed, the surface stays under the hover edge; open, the checked
  // variant, the hover edge lies on the surface in every enabled state.
  assert.deepEqual(themeRow(world, "selectTrigger", PRESSED), {
    part: PRESSED,
    color: TOKENS.surface,
  });
  for (const state of ["idle", "hovered", "pressed"] as const)
    assert.deepEqual(
      themeRow(world, "selectTrigger", checkedKey(PART.background, state)),
      {
        ...edge,
        part: checkedKey(PART.background, state),
        color: TOKENS.surface,
      },
    );
  assert.deepEqual(
    themeRow(world, "selectTrigger", checkedKey(PART.background, "disabled")),
    {
      part: checkedKey(PART.background, "disabled"),
      color: TOKENS.surface,
      border_color: TOKENS.neutral,
    },
  );

  // The chevron: two strokes of the lit line meeting at its tip, each half
  // the line past it, pointing down, or up while open, or neutral.
  const past = TOKENS.litLineWidth / 2 / Math.SQRT2 / 16;
  const chevron = (name: string) =>
    themeRow(world, name, PART.background) as Record<string, number[]>;
  const down = chevron("selectChevron");
  assert.equal(down.shape, 1 as never);
  assert.equal(down.border_width, TOKENS.litLineWidth as never);
  assert.deepEqual(down.color, TOKENS.text);
  const close = (actual: number[] | undefined, expected: number[]) =>
    assert.ok(
      actual?.every(
        (value, index) => Math.abs(value - expected[index]!) < 1e-9,
      ),
      `${actual} != ${expected}`,
    );
  close(down.stroke_a, [2 / 16, 5 / 16, 0.5 + past, 11 / 16 + past]);
  close(down.stroke_b, [0.5 - past, 11 / 16 + past, 14 / 16, 5 / 16]);
  const up = chevron("selectChevronOpen");
  close(up.stroke_a, [2 / 16, 11 / 16, 0.5 + past, 5 / 16 - past]);
  close(up.stroke_b, [0.5 - past, 5 / 16 - past, 14 / 16, 11 / 16]);
  assert.deepEqual(chevron("selectChevronDisabled").color, TOKENS.neutral);

  // An option that toggles: a menu row without the selected bar, whose
  // icon is the check mark, clear unless selected, then lit, or neutral
  // while disabled.
  for (const state of ["idle", "hovered", "pressed", "disabled"] as const)
    assert.equal(
      themeRow(world, "optionCheckRow", checkedKey(PART.background, state)),
      undefined,
    );
  assert.deepEqual(themeRow(world, "optionCheckRow", HOVERED), {
    part: HOVERED,
    color: TOKENS.rowTint,
  });
  assert.deepEqual(themeRow(world, "optionCheckRow", PART.icon), {
    ...CHECK_MARK,
    color: [0, 0, 0, 0],
  });
  for (const state of ["idle", "hovered", "pressed"] as const)
    assert.deepEqual(
      themeRow(world, "optionCheckRow", checkedKey(PART.icon, state))?.color,
      TOKENS.accent,
    );
  assert.deepEqual(
    themeRow(world, "optionCheckRow", checkedKey(PART.icon, "disabled"))?.color,
    TOKENS.neutral,
  );
});

const SKINS: readonly SelectOption[] = [
  { key: "aurora", label: "Aurora" },
  { key: "ember", label: "Ember" },
  { key: "neon", label: "Neon" },
  { key: "static", label: "Static", disabled: true },
];

const dropdown = (props: Partial<DropdownProps> = {}) =>
  h(Dropdown, { id: "skin", label: "Skin", options: SKINS, ...props });

test("Dropdown: a field-like trigger and its light list of options that take no focus", async () => {
  const { world } = await render(dropdown({ defaultValue: "aurora" }));
  // The trigger: a Button in the trigger look, the control height, its
  // value at the content inset and the chevron at its end.
  assert.equal(world.skin("skin"), "selectTrigger");
  assert.equal(world.fields("skin", "GuiButton").get("label"), "");
  assert.equal(world.fields("skin", "GuiButton").get("selected"), false);
  assert.equal(
    world.fields("skin", "GuiBehavior").get("semantic_label"),
    "Skin: Aurora",
  );
  const trigger = world.fields("skin", "GuiLayout");
  assert.deepEqual(
    ["kind", "width", "height", "padding_left", "padding_right"].map((field) =>
      trigger.get(field),
    ),
    [1, 240, 40, 16, 16],
  );
  assert.equal(world.fields("skin", "GuiFont").get("font_size"), 16);
  assert.deepEqual(world.children("skin"), [
    "skin/strut",
    "skin/value",
    "skin/chevron",
    "skin/list",
  ]);
  assert.equal(world.fields("skin/value", "CanvasText").get("text"), "Aurora");
  assert.deepEqual(world.tone("skin/value"), TOKENS.text);
  assert.equal(world.skin("skin/chevron"), "selectChevron");
  const chevron = world.fields("skin/chevron", "GuiLayout");
  assert.deepEqual(
    ["width", "height", "margin_left"].map((field) => chevron.get(field)),
    [16, 16, 8],
  );

  // The list: an anchored light overlay stretched below the trigger a
  // quarter inset away, closed, on the floating surface.
  const overlay = world.fields("skin/list", "GuiOverlay");
  assert.deepEqual(
    ["side", "align", "mode"].map((field) => overlay.get(field)),
    [0, 3, 1],
  );
  assert.equal(world.fields("skin/list", "CanvasStyle").get("y"), 4);
  assert.equal(world.fields("skin/list", "GuiBehavior").get("visible"), false);
  assert.equal(world.skin("skin/list"), "floating");
  assert.deepEqual(world.children("skin/list"), ["skin/options"]);

  // The option list: a frameless scroll view as tall as its rows, a
  // vertical group without selection, rows in half-inset margins.
  assert.equal(world.skin("skin/options"), "gridBody");
  assert.equal(world.fields("skin/options", "GuiLayout").get("height"), 160);
  assert.equal(
    world.fields("skin/options", "GuiScrollView").get("bar_thickness"),
    0,
  );
  const group = world.fields("skin/options/rows", "GuiGroup");
  assert.equal(group.get("axis"), 1);
  assert.equal(group.get("selection"), undefined);
  const rows = world.fields("skin/options/rows", "GuiLayout");
  assert.deepEqual(
    ["height", "padding_top", "padding_left", "padding_right"].map((field) =>
      rows.get(field),
    ),
    [160, 8, 8, 8],
  );
  assert.deepEqual(
    world.children("skin/options/rows"),
    SKINS.map((option) => `skin/options/${option.key}`),
  );
  for (const [key, selected, enabled, tone] of [
    ["aurora", true, true, TOKENS.text],
    ["ember", false, true, TOKENS.text],
    ["static", false, false, TOKENS.neutral],
  ] as const) {
    const row = `skin/options/${key}`;
    assert.equal(world.skin(row), "menuRow");
    assert.equal(world.fields(row, "GuiButton").get("selected"), selected);
    const behavior = world.fields(row, "GuiBehavior");
    assert.equal(behavior.get("focusable"), false);
    assert.equal(behavior.get("enabled"), enabled);
    assert.equal(world.fields(row, "GuiLayout").get("height"), 36);
    assert.equal(world.fields(row, "GuiLayout").get("padding_left"), 8);
    assert.deepEqual(world.tone(`${row}/label`), tone);
  }

  // Without a selection the trigger shows its placeholder in the neutral
  // tone; disabled, its value and chevron are neutral.
  const { world: empty } = await render(dropdown({ placeholder: "Choose" }));
  assert.equal(empty.fields("skin/value", "CanvasText").get("text"), "Choose");
  assert.deepEqual(empty.tone("skin/value"), TOKENS.neutral);
  assert.equal(
    empty.children("skin/options/rows").some((row) => row.endsWith("empty")),
    false,
  );
  const { world: disabled } = await render(
    dropdown({ defaultValue: "neon", disabled: true }),
  );
  assert.equal(disabled.fields("skin", "GuiBehavior").get("enabled"), false);
  assert.deepEqual(disabled.tone("skin/value"), TOKENS.neutral);
  assert.equal(disabled.skin("skin/chevron"), "selectChevronDisabled");
});

test("Dropdown: the trigger toggles the list; a pick closes it and reports once", async () => {
  const changes: string[] = [];
  const opened: boolean[] = [];
  const { world, root } = await render(
    dropdown({
      defaultValue: "aurora",
      onChange: (key) => changes.push(key),
      onOpenChange: (open) => opened.push(open),
    }),
  );
  const visible = () => world.fields("skin/list", "GuiBehavior").get("visible");
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.equal(world.fields("skin", "GuiButton").get("selected"), true);
  assert.equal(world.skin("skin/chevron"), "selectChevronOpen");

  // Two picks before the first one's render: one change, and the list
  // closes; the trigger shows the new value.
  world.effect("skin/options/neon", { kind: "pressed" });
  world.effect("skin/options/ember", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(changes, ["neon"]);
  assert.equal(visible(), false);
  assert.equal(world.fields("skin", "GuiButton").get("selected"), false);
  assert.equal(world.fields("skin/value", "CanvasText").get("text"), "Neon");
  assert.equal(
    world.fields("skin/options/neon", "GuiButton").get("selected"),
    true,
  );
  assert.equal(
    world.fields("skin/options/aurora", "GuiButton").get("selected"),
    false,
  );

  // Picking the selected option closes the list without a change.
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  world.effect("skin/options/neon", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), false);
  assert.deepEqual(changes, ["neon"]);

  // The runtime closing it, on Escape or an outside press, changes nothing;
  // the next press on the trigger opens it again.
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  world.value("skin/list", "GuiBehavior", { visible: true });
  world.value("skin/list", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  assert.deepEqual(changes, ["neon"]);
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.deepEqual(opened, [true, false, true, false, true, false, true]);
});

const NODES: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "beta", label: "Beta Relay" },
  { key: "gamma", label: "Gamma Dock", disabled: true },
  { key: "galley", label: "Galley" },
];

const searchable = (props: Partial<SearchableDropdownProps> = {}) =>
  h(SearchableDropdown, { id: "node", options: NODES, ...props });

test("SearchableDropdown: a search field above the list filters it; Enter picks the first match", async () => {
  const changes: string[] = [];
  const queries: string[] = [];
  const { world, root } = await render(
    searchable({
      defaultValue: "alpha",
      onChange: (key) => changes.push(key),
      onQueryChange: (query) => queries.push(query),
    }),
  );
  // The surface holds the search field, a text input in a padded row with
  // a magnifier before its text, above the option list.
  assert.deepEqual(world.children("node/list"), [
    "node/search",
    "node/options",
  ]);
  const row = world.fields("node/search", "GuiLayout");
  assert.deepEqual(
    ["height", "padding_left", "padding_right", "padding_top"].map((field) =>
      row.get(field),
    ),
    [48, 8, 8, 8],
  );
  const field = "node/search/field";
  assert.equal(
    world.fields(field, "GuiTextInput").get("placeholder"),
    "Search",
  );
  const layout = world.fields(field, "GuiLayout");
  assert.deepEqual(
    ["height", "flex", "padding_left"].map((name) => layout.get(name)),
    [40, 1, 32],
  );
  assert.deepEqual(world.children(field), [`${field}/strut`, `${field}/icon`]);
  assert.equal(
    world.fields(`${field}/icon`, "CanvasText").get("text"),
    GUI_KIT_ICONS.search,
  );
  assert.equal(
    world.fields(`${field}/icon`, "GuiLayout").get("margin_left"),
    -16,
  );

  // Typing filters by label, ignoring case; the selection stays.
  world.effect("node", { kind: "pressed" });
  await settle(root);
  world.value(field, "GuiTextInput", { text: "GA" });
  await settle(root);
  assert.deepEqual(queries, ["GA"]);
  assert.deepEqual(world.children("node/options/rows"), [
    "node/options/gamma",
    "node/options/galley",
  ]);
  assert.equal(
    world.fields("node/value", "CanvasText").get("text"),
    "Alpha Station",
  );
  world.value(field, "GuiTextInput", { text: "zz" });
  await settle(root);
  assert.deepEqual(world.children("node/options/rows"), ["node/options/empty"]);
  assert.equal(
    world.fields("node/options/empty/label", "CanvasText").get("text"),
    "No results",
  );
  assert.deepEqual(world.tone("node/options/empty/label"), TOKENS.neutral);

  // Enter without an active option picks the first enabled match of the
  // submitted text, closes the list and clears the search.
  world.effect(field, { kind: "submitted", text: "ga" }, "GuiTextInput");
  await settle(root);
  assert.deepEqual(changes, ["galley"]);
  assert.equal(world.fields("node/list", "GuiBehavior").get("visible"), false);
  // The field lies in the closed list, which refuses actions: the search is
  // cleared with a write of its field, from the text it last reported.
  assert.deepEqual(world.compared.at(-1), [field, "text", ""]);
  world.value(field, "GuiTextInput", { text: "" });
  await settle(root);
  assert.deepEqual(queries, ["GA", "zz", ""]);
  assert.equal(world.children("node/options/rows").length, NODES.length);

  // A submission that matches nothing leaves the list open.
  world.effect("node", { kind: "pressed" });
  await settle(root);
  world.effect(field, { kind: "submitted", text: "zz" }, "GuiTextInput");
  await settle(root);
  assert.equal(world.fields("node/list", "GuiBehavior").get("visible"), true);
  assert.deepEqual(changes, ["galley"]);

  // Loading adds the spinner's row after the options.
  const { world: loading } = await render(searchable({ loading: "Loading…" }));
  assert.equal(
    loading.children("node/options/rows").at(-1),
    "node/options/loading",
  );
  assert.equal(
    loading
      .fields("node/options/loading/spinner/label", "CanvasText")
      .get("text"),
    "Loading…",
  );
  assert.equal(
    loading.fields("node/options", "GuiLayout").get("height"),
    (NODES.length + 1) * 36 + 16,
  );
});

const CHANNELS: readonly SelectOption[] = [
  { key: "render", label: "Render" },
  { key: "physics", label: "Physics" },
  { key: "network", label: "Network" },
];

const multi = (props: Partial<MultiSelectProps> = {}) =>
  h(MultiSelect, { id: "channels", options: CHANNELS, ...props });

test("MultiSelect: the trigger summarises the selection; options toggle with a check and the list stays open", async () => {
  // Labels while they fit, measured with the shared font, else the count.
  const advance = 0.54 * 16;
  assert.equal(
    selectionSummary(["Render", "Physics"], 15 * advance, 16),
    "Render, Physics",
  );
  assert.equal(
    selectionSummary(["Render", "Physics"], 15 * advance - 1, 16),
    "2 selected",
  );

  const changes: (readonly string[])[] = [];
  const { world, root, draw } = await render(
    multi({
      defaultValue: ["physics", "render"],
      onChange: (value) => changes.push(value),
    }),
  );
  // The default trigger leaves 240 - 2 * 16 - 16 - 8 units for its text.
  assert.equal(
    world.fields("channels/value", "CanvasText").get("text"),
    "Render, Physics",
  );
  for (const [key, selected] of [
    ["render", true],
    ["physics", true],
    ["network", false],
  ] as const) {
    const row = `channels/options/${key}`;
    assert.equal(world.skin(row), "optionCheckRow");
    assert.equal(world.fields(row, "GuiButton").get("selected"), selected);
    // The check takes the row's leading square.
    assert.equal(world.fields(row, "GuiLayout").get("padding_left"), 36);
  }

  // Toggles while open report the selection in option order, each built
  // on the one before even ahead of its render, and keep the list open.
  world.effect("channels", { kind: "pressed" });
  await settle(root);
  world.effect("channels/options/network", { kind: "pressed" });
  world.effect("channels/options/render", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(changes, [
    ["render", "physics", "network"],
    ["physics", "network"],
  ]);
  assert.equal(
    world.fields("channels/list", "GuiBehavior").get("visible"),
    true,
  );
  assert.equal(
    world.fields("channels/options/render", "GuiButton").get("selected"),
    false,
  );
  // Once the runtime closed the list, a late press toggles nothing.
  world.value("channels/list", "GuiBehavior", { visible: true });
  world.value("channels/list", "GuiBehavior", { visible: false });
  await settle(root);
  world.effect("channels/options/render", { kind: "pressed" });
  await settle(root);
  assert.equal(changes.length, 2);

  // Every channel does not fit: the count stands in; none shows the
  // placeholder.
  await draw(multi({ value: ["render", "physics", "network"] }));
  assert.equal(
    world.fields("channels/value", "CanvasText").get("text"),
    "3 selected",
  );
  await draw(multi({ value: [], placeholder: "Channels" }));
  assert.equal(
    world.fields("channels/value", "CanvasText").get("text"),
    "Channels",
  );
  assert.deepEqual(world.tone("channels/value"), TOKENS.neutral);
});

const PLACES: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "alpine", label: "Alpine Relay" },
  { key: "beta", label: "Beta Relay" },
];

/** An application suggesting the places that start with the typed text. */
function Destination({
  log,
  ...props
}: Partial<AutocompleteProps> & { readonly log: unknown[] }) {
  const [text, setText] = useState("");
  const typed = text.toLowerCase();
  return h(Autocomplete, {
    id: "dest",
    suggestions: typed
      ? PLACES.filter((place) => place.label.toLowerCase().startsWith(typed))
      : [],
    onInputChange: (value) => {
      log.push(["input", value]);
      setText(value);
    },
    onSelect: (key) => log.push(["select", key]),
    onCommit: (value) => log.push(["commit", value]),
    ...props,
  });
}

test("Autocomplete: typing opens the application's suggestions; accepting writes the field once", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(
    h(Destination, { log, placeholder: "Destination" }),
  );
  const visible = () => world.fields("dest/list", "GuiBehavior").get("visible");
  const typed = async (text: string) => {
    world.value("dest", "GuiTextInput", { text });
    await settle(root);
  };
  // The field: a text input of the control height holding the list.
  const input = world.fields("dest", "GuiTextInput");
  assert.equal(input.get("placeholder"), "Destination");
  assert.equal(input.get("text"), "");
  const layout = world.fields("dest", "GuiLayout");
  assert.deepEqual(
    ["kind", "width", "height"].map((field) => layout.get(field)),
    [0, 240, 40],
  );
  assert.deepEqual(world.children("dest"), ["dest/list"]);
  assert.equal(visible(), false);

  // Typing reports the text and shows the suggestions; none hide the list.
  await typed("al");
  assert.equal(visible(), true);
  assert.deepEqual(world.children("dest/suggestions/rows"), [
    "dest/suggestions/alpha",
    "dest/suggestions/alpine",
  ]);
  assert.equal(world.skin("dest/suggestions/alpha"), "menuRow");
  assert.equal(
    world.fields("dest/suggestions/alpha", "GuiButton").get("selected"),
    false,
  );
  world.value("dest/list", "GuiBehavior", { visible: true });
  await typed("alz");
  assert.equal(visible(), false);
  // The report of that hiding is the client's own: suggestions bring the
  // list back.
  world.value("dest/list", "GuiBehavior", { visible: false });
  await typed("alp");
  assert.equal(visible(), true);

  // Accepting a suggestion writes its label to the field, reports its key
  // and closes the list; the field's report of that label opens nothing.
  world.effect("dest/suggestions/alpine", { kind: "pressed" });
  world.effect("dest/suggestions/alpha", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(world.actions.at(-1), [
    "dest",
    { kind: "text", value: "Alpine Relay" },
  ]);
  assert.equal(visible(), false);
  await typed("Alpine Relay");
  assert.equal(visible(), false);

  // The runtime closing the list, on Escape or an outside press, keeps the
  // text; typing opens it again.
  await typed("b");
  assert.equal(visible(), true);
  world.value("dest/list", "GuiBehavior", { visible: true });
  world.value("dest/list", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  await typed("be");
  assert.equal(visible(), true);

  // Enter without an active suggestion commits the typed text.
  world.effect("dest", { kind: "submitted", text: "be" }, "GuiTextInput");
  await settle(root);
  assert.equal(visible(), false);
  assert.deepEqual(log, [
    ["input", "al"],
    ["input", "alz"],
    ["input", "alp"],
    ["select", "alpine"],
    ["input", "Alpine Relay"],
    ["input", "b"],
    ["input", "be"],
    ["commit", "be"],
  ]);

  // Loading shows its row while the application has no suggestions yet.
  const { world: loading, root: loadingRoot } = await render(
    h(Destination, { log: [], loading: "Loading…" }),
  );
  loading.value("dest", "GuiTextInput", { text: "zz" });
  await settle(loadingRoot);
  assert.equal(loading.fields("dest/list", "GuiBehavior").get("visible"), true);
  assert.deepEqual(loading.children("dest/suggestions/rows"), [
    "dest/suggestions/loading",
  ]);
});

/**
 * A popover holding a trigger whose own menu opens inside it: two overlays,
 * one nested in the other, each with its own open state.
 */
function NestedOverlays({ log }: { readonly log: unknown[] }) {
  const inner = useOverlayOpen({
    defaultOpen: true,
    onOpenChange: (open) => log.push(["inner", open]),
  });
  return h(
    Popover,
    {
      id: "outer",
      label: "Options",
      title: "Options",
      defaultOpen: true,
      onOpenChange: (open) => log.push(["outer", open]),
    },
    h(
      Entity,
      { id: "inner-trigger" },
      h(Button, { label: "More", onPress: inner.toggle }),
      h(
        Children,
        null,
        h(
          Floating,
          {
            id: "inner",
            mode: "light",
            open: inner.open,
            onVisibleChange: inner.onVisibleChange,
          },
          h(Menu, { id: "inner/menu", items: COMMANDS, overlay: inner }),
        ),
      ),
    ),
  );
}

test("closing an overlay nested in another closes only its own open state", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(h(NestedOverlays, { log }));
  await settle(root);
  const visible = (symbol: string) =>
    world.fields(symbol, "GuiBehavior").get("visible");
  assert.equal(visible("outer/popover"), true);
  assert.equal(visible("inner"), true);
  // Both are reported open; the runtime then closes the inner one, as an
  // outside press within the popover does.
  world.value("outer/popover", "GuiBehavior", { visible: true });
  world.value("inner", "GuiBehavior", { visible: true });
  await settle(root);
  world.value("inner", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible("inner"), false);
  assert.equal(visible("outer/popover"), true);
  assert.deepEqual(log, [["inner", false]]);
  // The outer one's own closing is still adopted.
  world.value("outer/popover", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible("outer/popover"), false);
  assert.deepEqual(log, [
    ["inner", false],
    ["outer", false],
  ]);
});

/** One open state wired to two overlays, as a mistaken composition would. */
function SharedOpenState({ log }: { readonly log: unknown[] }) {
  const overlay = useOverlayOpen({
    defaultOpen: true,
    onOpenChange: (open) => log.push(open),
  });
  return h(
    Fragment,
    null,
    ["first", "second"].map((id) =>
      h(Floating, {
        key: id,
        id,
        mode: "light",
        open: overlay.open,
        onVisibleChange: overlay.onVisibleChange,
      }),
    ),
  );
}

test("useOverlayOpen adopts a closing only from the entity reported open", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(h(SharedOpenState, { log }));
  await settle(root);
  world.value("first", "GuiBehavior", { visible: true });
  await settle(root);
  // Another entity's closing is ignored...
  world.value("second", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(log, []);
  // ...the reported one's is adopted.
  world.value("first", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(log, [false]);
});

/**
 * The slider composites' rail at kit body size `fontSize`: the runtime's
 * unsized depth, its thumb, and where a scale's ticks, origin mark and labels
 * start across the rail from the slider's edge.
 */
function rail(fontSize: number) {
  const k = fontSize / 16;
  const depth = (fontSize * 4) / 3;
  const thumb = 0.75 * depth;
  const tick = depth / 2 + thumb / 2 + 4 * k;
  return {
    depth,
    thumb,
    tick,
    origin: depth / 2 + 4 * k,
    end: tick + 12 * k,
    row: 24 * k,
    gap: 8 * k,
  };
}

/** Width of a line of `text` at `size`, as the kit gives it. */
const lineOf = (text: string, size: number) =>
  [...text].length * 0.54 * size + size / 100;

test("SliderScale marks values where the thumb's centre is, its origin from the rail", async () => {
  const { world } = await render(
    h(SliderScale, {
      id: "pan",
      min: -100,
      max: 100,
      origin: 0,
      vertical: true,
    }),
    { fontSize: 32 },
  );
  const g = rail(32);
  // As long as the unsized slider, reaching across to its widest label.
  const root = world.fields("pan", "GuiLayout");
  assert.equal(root.get("kind"), 3);
  assert.equal(root.get("height"), 256);
  near(root.get("width"), g.end + g.gap + lineOf("+100", 26));
  const marks = [0, 1, 2, 3, 4].map((index) => `pan/mark/${index}`);
  assert.deepEqual(world.children("pan"), marks);
  const labels = ["-100", "-50", "0", "+50", "+100"];
  marks.forEach((mark, index) => {
    // A thumb-sized box aligned by the value's fraction, minimum at the bottom.
    const box = world.fields(mark, "GuiLayout");
    assert.equal(box.get("kind"), 3);
    near(box.get("height"), g.thumb);
    near(box.get("align_y"), 1 - index / 2);
    assert.deepEqual(world.children(mark), [`${mark}/tick`, `${mark}/label`]);
    const origin = index === 2;
    assert.equal(world.skin(`${mark}/tick`), origin ? "division" : "quiet");
    const tick = world.fields(`${mark}/tick`, "GuiLayout");
    near(tick.get("margin_left"), origin ? g.origin : g.tick);
    near(tick.get("width"), g.end - (origin ? g.origin : g.tick));
    assert.equal(tick.get("height"), 2.5);
    assert.equal(tick.get("align_y"), 0);
    // The label past the tick, centred on the mark, small and neutral.
    const label = world.fields(`${mark}/label`, "GuiLayout");
    near(label.get("margin_left"), g.end + g.gap);
    near(label.get("margin_top"), (g.thumb - g.row) / 2);
    near(label.get("margin_bottom"), (g.thumb - g.row) / 2);
    assert.equal(
      world.fields(`${mark}/label/text`, "CanvasText").get("text"),
      labels[index],
    );
    assert.equal(
      world.fields(`${mark}/label/text`, "CanvasText").get("font_size"),
      26,
    );
    assert.deepEqual(world.tone(`${mark}/label/text`), TOKENS.neutral);
  });
});

test("a horizontal SliderScale hangs its labels under the ticks with the marks' decimals", async () => {
  const { world } = await render(
    h(SliderScale, { id: "volts", min: 0, max: 1, count: 3, units: "V" }),
    { fontSize: 32 },
  );
  const g = rail(32);
  // As long as its stack, reaching down to its labels' dense row.
  const root = world.fields("volts", "GuiLayout");
  near(root.get("height"), g.end + g.row);
  assert.equal(root.has("width"), false);
  const marks = [0, 1, 2].map((index) => `volts/mark/${index}`);
  assert.deepEqual(world.children("volts"), marks);
  assert.deepEqual(
    marks.map((mark) => world.fields(mark, "GuiLayout").get("align_x")),
    [-1, 0, 1],
  );
  near(world.fields(marks[0]!, "GuiLayout").get("width"), g.thumb);
  // Units follow every label; a unit of letters after a space.
  assert.deepEqual(
    marks.map((mark) =>
      world.fields(`${mark}/label/text`, "CanvasText").get("text"),
    ),
    ["0.0 V", "0.5 V", "1.0 V"],
  );
  const label = world.fields("volts/mark/1/label", "GuiLayout");
  near(label.get("margin_top"), g.end);
  near(label.get("margin_left"), (g.thumb - lineOf("0.5 V", 26)) / 2);
  near(label.get("margin_right"), (g.thumb - lineOf("0.5 V", 26)) / 2);
  const tick = world.fields("volts/mark/1/tick", "GuiLayout");
  near(tick.get("margin_top"), g.tick);
  near(tick.get("height"), g.end - g.tick);
  assert.equal(tick.get("align_x"), 0);
});

const gain = (props: Partial<LabelledSliderProps> = {}) =>
  h(LabelledSlider, {
    id: "gain",
    label: "GAIN",
    min: 0,
    max: 100,
    step: 1,
    fineStep: 0.1,
    units: "%",
    ...props,
  });

test("LabelledSlider: caption and readout above a rail between its end captions", async () => {
  const changes: number[] = [];
  const { world, root, draw } = await render(
    gain({ defaultValue: 65, onChange: (value) => changes.push(value) }),
    { fontSize: 32 },
  );
  const g = rail(32);
  const layout = world.fields("gain", "GuiLayout");
  assert.equal(layout.get("kind"), 2);
  near(layout.get("height"), g.row + g.depth);
  assert.deepEqual(world.children("gain"), ["gain/header", "gain/rail-row"]);
  assert.deepEqual(world.children("gain/header"), [
    "gain/header/strut",
    "gain/label",
    "gain/readout",
  ]);
  assert.deepEqual(world.tone("gain/label"), TOKENS.accent);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "65%");
  assert.deepEqual(world.tone("gain/readout"), TOKENS.text);
  // The rail between the range's ends, centred on it, half an inset away.
  assert.deepEqual(world.children("gain/rail-row"), [
    "gain/min",
    "gain/rail",
    "gain/max",
  ]);
  assert.equal(world.fields("gain/min/text", "CanvasText").get("text"), "0%");
  assert.deepEqual(world.tone("gain/min/text"), TOKENS.neutral);
  near(world.fields("gain/min", "GuiLayout").get("height"), g.depth);
  near(world.fields("gain/min", "GuiLayout").get("margin_right"), g.gap);
  assert.equal(world.fields("gain/max/text", "CanvasText").get("text"), "100%");
  assert.equal(world.fields("gain/rail", "GuiLayout").get("flex"), 1);
  // The runtime's slider at its unsized depth, filling the rail, unskinned.
  assert.deepEqual(world.children("gain/rail"), ["gain/slider"]);
  const slider = world.fields("gain/slider", "GuiSlider");
  assert.deepEqual(
    ["min", "max", "step", "fine_step", "axis", "value"].map((field) =>
      slider.get(field),
    ),
    [0, 100, 1, 0.1, 0, 65],
  );
  assert.equal(slider.has("origin"), false);
  assert.equal(world.fields("gain/slider", "GuiLayout").get("kind"), 3);
  near(world.fields("gain/slider", "GuiLayout").get("height"), g.depth);
  assert.equal(world.entity("gain/slider").components.has("GuiSkin"), false);
  const behavior = world.fields("gain/slider", "GuiBehavior");
  assert.equal(behavior.get("semantic_label"), "GAIN");
  assert.equal(behavior.get("enabled"), true);

  // A committed value is reported and read out at once.
  await settle(root);
  world.value("gain/slider", "GuiSlider", {
    value: 70,
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [70]);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "70%");
  assert.equal(world.fields("gain/slider", "GuiSlider").get("value"), 65);
  assert.deepEqual(world.compared, []);

  // Disabled: the value stays, every text turns neutral.
  await draw(gain({ defaultValue: 65, disabled: true }));
  assert.equal(
    world.fields("gain/slider", "GuiBehavior").get("enabled"),
    false,
  );
  for (const text of ["gain/label", "gain/readout", "gain/min/text"])
    assert.deepEqual(world.tone(text), TOKENS.neutral, text);
});

test("a vertical LabelledSlider centres its rail, with a signed scale right of it", async () => {
  const { world } = await render(
    h(LabelledSlider, {
      id: "pan",
      label: "PAN",
      min: -100,
      max: 100,
      step: 1,
      origin: 0,
      defaultValue: -30,
      units: "%",
      vertical: true,
      length: 160,
      scale: { count: 5 },
    }),
    { fontSize: 32 },
  );
  const g = rail(32);
  const reach = g.end + g.gap + lineOf("+100", 26);
  // Caption, rail and readout; the scale labels the ends, so no captions.
  near(world.fields("pan", "GuiLayout").get("height"), 2 * g.row + 320);
  assert.deepEqual(world.children("pan"), [
    "pan/label",
    "pan/track",
    "pan/readout",
  ]);
  assert.equal(world.fields("pan/label", "GuiLayout").get("align_x"), 0);
  assert.equal(
    world.fields("pan/readout/text", "CanvasText").get("text"),
    "-30%",
  );
  // The rail centred in the column; the scale reaches past it.
  const box = world.fields("pan/rail", "GuiLayout");
  near(box.get("width"), reach);
  assert.equal(box.get("height"), 320);
  assert.equal(box.get("align_x"), 0);
  near(box.get("margin_right"), g.depth - reach);
  assert.deepEqual(world.children("pan/rail"), ["pan/scale", "pan/slider"]);
  const slider = world.fields("pan/slider", "GuiSlider");
  assert.equal(slider.get("axis"), 1);
  assert.equal(slider.get("origin"), 0);
  assert.equal(slider.get("value"), -30);
  near(world.fields("pan/slider", "GuiLayout").get("width"), g.depth);
  // The scale's zero is the slider's origin, its labels signed, unitless.
  assert.equal(world.fields("pan/scale", "GuiLayout").get("height"), 320);
  assert.equal(world.skin("pan/scale/mark/2/tick"), "division");
  assert.equal(
    world.fields("pan/scale/mark/4/label/text", "CanvasText").get("text"),
    "+100",
  );
});

test("a slider composite reports committed values and writes only the application's own", async () => {
  const changes: number[] = [];
  const onChange = (value: number) => changes.push(value);
  const handle: { current: unknown } = { current: null };
  const { world, root, draw } = await render(
    gain({ value: 20, onChange, ref: handle as never }),
  );
  await settle(root);
  assert.ok(handle.current, "the composite's ref receives the handle");

  // A drag commits 30: reported, and nothing is written back when the
  // application follows.
  world.value("gain/slider", "GuiSlider", {
    value: 30,
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [30]);
  await draw(gain({ value: 30, onChange, ref: handle as never }));
  await settle(root);
  assert.deepEqual(world.compared, []);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "30%");

  // The application's own value, even the one declared at mount, is written
  // by compare-and-set from the reported one; its echo is not reported.
  await draw(gain({ value: 20, onChange, ref: handle as never }));
  await settle(root);
  assert.deepEqual(world.compared, [["gain/slider", "value", 20]]);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "20%");
  world.value("gain/slider", "GuiSlider", {
    value: 20,
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [30]);
  // Single precision: the runtime's 0.1 is the application's 0.1.
  await draw(gain({ value: 0.1, onChange }));
  await settle(root);
  world.value("gain/slider", "GuiSlider", {
    value: Math.fround(0.1),
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [30]);
});

const distance = (props: Partial<RangeSliderProps> = {}) =>
  h(RangeSlider, {
    id: "dist",
    min: 0,
    max: 100,
    step: 1,
    units: "m",
    ...props,
  });

test("RangeSlider: readouts follow their thumbs and both values report once", async () => {
  const changes: (readonly number[])[] = [];
  const { world, root } = await render(
    distance({
      label: "DISTANCE",
      defaultValue: [20, 80],
      onChange: (value) => changes.push(value),
    }),
    { fontSize: 32 },
  );
  const g = rail(32);
  near(world.fields("dist", "GuiLayout").get("height"), 2 * g.row + g.depth);
  assert.deepEqual(world.children("dist"), ["dist/header", "dist/rail-row"]);
  assert.deepEqual(world.children("dist/rail-row"), [
    "dist/min",
    "dist/rail",
    "dist/max",
  ]);
  assert.equal(
    world.fields("dist/max/text", "CanvasText").get("text"),
    "100 m",
  );
  const slider = world.fields("dist/slider", "GuiSlider");
  assert.deepEqual(
    ["range", "value", "upper", "axis"].map((field) => slider.get(field)),
    [true, 20, 80, 0],
  );
  // Each readout centred under its thumb, a dense row below the slider.
  assert.deepEqual(world.children("dist/rail"), [
    "dist/thumb/0",
    "dist/thumb/1",
    "dist/slider",
  ]);
  const readouts = () =>
    [0, 1].map((index) => [
      world.fields(`dist/thumb/${index}`, "GuiLayout").get("align_x"),
      world.fields(`dist/readout/${index}/text`, "CanvasText").get("text"),
    ]);
  const close = (pairs: unknown[][], expected: [number, string][]) =>
    pairs.forEach(([align, text], index) => {
      near(align, expected[index]![0]);
      assert.equal(text, expected[index]![1]);
    });
  close(readouts(), [
    [-0.6, "20 m"],
    [0.6, "80 m"],
  ]);
  // The lower readout hangs from its thumb's centre toward the minimum.
  const readout = world.fields("dist/readout/0", "GuiLayout");
  near(readout.get("margin_top"), g.depth);
  near(readout.get("margin_left"), g.thumb / 2 - 8 - lineOf("20 m", 26));
  assert.deepEqual(world.tone("dist/readout/0/text"), TOKENS.text);

  // A drag of the lower thumb: one event, one report of both values.
  await settle(root);
  world.value("dist/slider", "GuiSlider", {
    value: 35,
    upper: 80,
    range: true,
  });
  await settle(root);
  assert.deepEqual(changes, [[35, 80]]);
  close(readouts(), [
    [-0.3, "35 m"],
    [0.6, "80 m"],
  ]);
});

test("a range's own interval is written one field at a time, never inverted", async () => {
  const changes: (readonly number[])[] = [];
  const onChange = (value: readonly number[]) => changes.push(value);
  const { world, root, draw } = await render(
    distance({ value: [20, 80], onChange }),
  );
  await settle(root);
  // Up past the upper value: the upper value moves first.
  await draw(distance({ value: [85, 95], onChange }));
  await settle(root);
  assert.deepEqual(world.compared, [
    ["dist/slider", "upper", 95],
    ["dist/slider", "value", 85],
  ]);
  // The state between the writes, and the echo, are not reported.
  world.value("dist/slider", "GuiSlider", {
    value: 20,
    upper: 95,
    range: true,
  });
  world.value("dist/slider", "GuiSlider", {
    value: 85,
    upper: 95,
    range: true,
  });
  await settle(root);
  assert.deepEqual(changes, []);
  // Down below the lower value: the lower value moves first; a value that
  // keeps its upper end writes only the lower one.
  await draw(distance({ value: [5, 10], onChange }));
  await draw(distance({ value: [0, 10], onChange }));
  await settle(root);
  assert.deepEqual(world.compared.slice(2), [
    ["dist/slider", "value", 5],
    ["dist/slider", "upper", 10],
    ["dist/slider", "value", 0],
  ]);
});

test("a vertical RangeSlider puts each readout right of its thumb", async () => {
  const { world } = await render(
    distance({ defaultValue: [20, 80], vertical: true, length: 160 }),
    { fontSize: 32 },
  );
  const g = rail(32);
  near(world.fields("dist", "GuiLayout").get("height"), 2 * g.row + 320);
  assert.deepEqual(world.children("dist"), [
    "dist/max",
    "dist/track",
    "dist/min",
  ]);
  assert.equal(world.fields("dist/slider", "GuiSlider").get("axis"), 1);
  const offset = (g.depth + g.thumb) / 2 + g.gap;
  const box = world.fields("dist/rail", "GuiLayout");
  near(box.get("width"), offset + lineOf("100 m", 26));
  near(world.fields("dist/thumb/1", "GuiLayout").get("align_y"), -0.6);
  near(world.fields("dist/thumb/0", "GuiLayout").get("height"), g.thumb);
  near(world.fields("dist/readout/1", "GuiLayout").get("margin_left"), offset);
});

const knob = (props: Partial<KnobProps> = {}) =>
  h(Knob, {
    id: "gain",
    label: "GAIN",
    min: 0,
    max: 100,
    step: 1,
    units: "%",
    ...props,
  });

test("Knob: the dial's housing holds the readout under the dial and the ends at the sweep's", async () => {
  const changes: number[] = [];
  const { world, root, draw } = await render(
    knob({ defaultValue: 65, onChange: (value) => changes.push(value) }),
  );
  const layout = world.fields("gain", "GuiLayout");
  assert.equal(layout.get("kind"), 2);
  assert.equal(layout.get("width"), 80);
  assert.equal(layout.get("height"), 24 + 80 + 24);
  assert.deepEqual(world.children("gain"), ["gain/caption", "gain/dial"]);
  assert.deepEqual(world.tone("gain/label"), TOKENS.accent);
  // The housing is the dial-presented slider, a dense row taller than wide.
  const dial = world.fields("gain/dial", "GuiLayout");
  assert.deepEqual(
    ["kind", "width", "height", "align_x"].map((field) => dial.get(field)),
    [3, 80, 104, 0],
  );
  const slider = world.fields("gain/dial", "GuiSlider");
  assert.equal(slider.get("axis"), 2);
  assert.equal(slider.get("value"), 65);
  assert.equal(world.entity("gain/dial").components.has("GuiSkin"), false);
  assert.deepEqual(world.children("gain/dial"), [
    "gain/min",
    "gain/max",
    "gain/readout",
  ]);
  const readout = world.fields("gain/readout", "GuiLayout");
  assert.equal(readout.get("margin_top"), 80);
  assert.equal(readout.get("align_x"), 0);
  assert.equal(
    world.fields("gain/readout/text", "CanvasText").get("text"),
    "65%",
  );
  // Each end centred a small size below the tick ring's end, but half an
  // inset inside the housing: 100% would reach past it.
  const reach = (40 - 8) / Math.SQRT2;
  const min = world.fields("gain/min", "GuiLayout");
  near(min.get("margin_left"), 40 - reach - lineOf("0%", 13) / 2);
  near(min.get("margin_top"), 40 + reach + 13 - 12);
  const max = world.fields("gain/max", "GuiLayout");
  near(max.get("margin_left"), 80 - 8 - lineOf("100%", 13));
  near(max.get("margin_top"), 40 + reach + 13 - 12);
  assert.equal(world.fields("gain/max/text", "CanvasText").get("text"), "100%");

  await settle(root);
  world.value("gain/dial", "GuiSlider", { value: 80, upper: 0, range: false });
  await settle(root);
  assert.deepEqual(changes, [80]);
  assert.equal(
    world.fields("gain/readout/text", "CanvasText").get("text"),
    "80%",
  );

  // Bipolar from zero, signed ends; the paired input's row under the housing.
  await draw(
    h(Knob, {
      id: "gain",
      label: "PAN",
      min: -100,
      max: 100,
      step: 1,
      origin: 0,
      children: h(Entity, { id: "precise" }),
    }),
  );
  assert.equal(world.fields("gain/dial", "GuiSlider").get("origin"), 0);
  assert.equal(world.fields("gain/min/text", "CanvasText").get("text"), "-100");
  assert.equal(world.fields("gain/max/text", "CanvasText").get("text"), "+100");
  // The input's column: a numeric stepper's field and error line.
  assert.equal(
    world.fields("gain", "GuiLayout").get("height"),
    128 + 8 + 40 + 8 + 40,
  );
  assert.deepEqual(world.children("gain/slot"), ["precise"]);
  const input = world.fields("gain/slot", "GuiLayout");
  assert.equal(input.get("height"), 40 + 8 + 40);
  assert.equal(input.get("margin_top"), 8);

  // A larger dial gives longer end captions room.
  await draw(knob({ size: 112, defaultValue: 65 }));
  assert.equal(world.fields("gain", "GuiLayout").get("width"), 112);
  const larger = world.fields("gain/dial", "GuiLayout");
  assert.equal(larger.get("width"), 112);
  assert.equal(larger.get("height"), 112 + 24);
  near(
    world.fields("gain/max", "GuiLayout").get("margin_top"),
    56 + (56 - 8) / Math.SQRT2 + 13 - 12,
  );
});

test("range readouts hang from their thumbs away from each other, so thumbs together keep them apart", async () => {
  const { world, draw } = await render(distance({ defaultValue: [50, 50] }), {
    fontSize: 32,
  });
  const g = rail(32);
  const clear = 8;
  // Both marks at the same place; the lower text ends a quarter inset before
  // the thumbs' centre and the upper one starts a quarter inset after it.
  near(world.fields("dist/thumb/0", "GuiLayout").get("align_x"), 0);
  near(world.fields("dist/thumb/1", "GuiLayout").get("align_x"), 0);
  const width = lineOf("50 m", 26);
  const lower = world.fields("dist/readout/0", "GuiLayout");
  const upper = world.fields("dist/readout/1", "GuiLayout");
  near(lower.get("margin_left"), g.thumb / 2 - clear - width);
  near(lower.get("margin_right"), g.thumb / 2 + clear);
  near(upper.get("margin_left"), g.thumb / 2 + clear);
  near(upper.get("margin_right"), g.thumb / 2 - clear - width);
  const lowerEnd = (lower.get("margin_left") as number) + width;
  assert.ok(
    lowerEnd + 2 * clear <= (upper.get("margin_left") as number) + 1e-9,
    "the readouts overlap",
  );

  // Vertical: the lower readout's row hangs down from the centre, the upper
  // one's ends there.
  await draw(distance({ defaultValue: [50, 50], vertical: true, length: 160 }));
  const down = world.fields("dist/readout/0", "GuiLayout");
  const up = world.fields("dist/readout/1", "GuiLayout");
  near(down.get("margin_top"), g.thumb / 2);
  near(down.get("margin_bottom"), g.thumb / 2 - g.row);
  near(up.get("margin_top"), g.thumb / 2 - g.row);
  near(up.get("margin_bottom"), g.thumb / 2);
});

const exposure = (props: Partial<NumericStepperProps> = {}) =>
  h(NumericStepper, {
    id: "ev",
    label: "EXPOSURE",
    min: -4,
    max: 4,
    step: 0.25,
    fineStep: 0.05,
    precision: 2,
    units: "EV",
    defaultValue: 1.25,
    ...props,
  });

test("NumericStepper: caption and range, the numeric field with its unit, and the error line's room", async () => {
  const { world, draw } = await render(exposure(), { fontSize: 32 });
  // Caption, field and the error line reserved, at twice the design size.
  const root = world.fields("ev", "GuiLayout");
  assert.equal(root.get("kind"), 2);
  assert.equal(root.get("height"), 2 * (24 + 40 + 8 + 40));
  assert.deepEqual(world.children("ev"), ["ev/caption", "ev/row"]);
  assert.deepEqual(world.children("ev/caption"), [
    "ev/caption/strut",
    "ev/label",
    "ev/ends",
  ]);
  assert.deepEqual(world.tone("ev/label"), TOKENS.accent);
  const ends = world.fields("ev/ends", "CanvasText");
  assert.equal(ends.get("text"), "-4.00 – +4.00 EV");
  assert.equal(ends.get("font_size"), 26);
  assert.deepEqual(world.tone("ev/ends"), TOKENS.neutral);
  // The runtime's numeric input fills the row beside its unit.
  assert.deepEqual(world.children("ev/row"), [
    "ev/row/strut",
    "ev/field",
    "ev/units",
  ]);
  const field = world.fields("ev/field", "GuiTextInput");
  assert.deepEqual(
    [
      "numeric",
      "value",
      "min",
      "max",
      "step",
      "fine_step",
      "precision",
      "step_parts",
    ].map((name) => field.get(name)),
    [true, 1.25, -4, 4, 0.25, 0.05, 2, true],
  );
  const fieldLayout = world.fields("ev/field", "GuiLayout");
  assert.equal(fieldLayout.get("flex"), 1);
  assert.equal(fieldLayout.get("height"), 80);
  assert.equal(world.entity("ev/field").components.has("GuiSkin"), false);
  const behavior = world.fields("ev/field", "GuiBehavior");
  assert.equal(behavior.get("semantic_label"), "EXPOSURE");
  assert.equal(behavior.get("enabled"), true);
  assert.equal(world.fields("ev/units", "CanvasText").get("text"), "EV");
  assert.equal(world.fields("ev/units", "GuiLayout").get("margin_left"), 16);
  assert.deepEqual(world.tone("ev/units"), TOKENS.text);

  // Without a caption or range, and without step parts; disabled, neutral.
  await draw(
    h(NumericStepper, {
      id: "ev",
      units: "%",
      stepParts: false,
      disabled: true,
    }),
  );
  assert.equal(
    world.fields("ev", "GuiLayout").get("height"),
    2 * (40 + 8 + 40),
  );
  assert.deepEqual(world.children("ev"), ["ev/row"]);
  assert.equal(
    world.fields("ev/field", "GuiTextInput").get("step_parts"),
    false,
  );
  assert.equal(world.fields("ev/field", "GuiBehavior").get("enabled"), false);
  assert.deepEqual(world.tone("ev/units"), TOKENS.neutral);
});

test("a rejected entry shows its error until a later commit, submission, discard or blur", async () => {
  const changes: number[] = [];
  const { world, root } = await render(
    exposure({
      onChange: (value) => changes.push(value),
      invalidMessage: (text) => `${text} is not a number`,
    }),
  );
  await settle(root);
  const shown = () =>
    world.entities.has("ev/error")
      ? world.fields("ev/error/text", "CanvasText").get("text")
      : undefined;
  const reject = async (text: string, tick: bigint) => {
    world.effect("ev/field", { kind: "rejected", text }, "GuiTextInput", tick);
    await settle(root);
  };

  // A rejection shows the error alert under the field, which stays put.
  await reject("abc", 5n);
  assert.equal(shown(), "abc is not a number");
  assert.equal(world.skin("ev/error"), "alertError");
  assert.equal(world.fields("ev/error", "GuiLayout").get("margin_top"), 8);
  assert.deepEqual(world.children("ev"), ["ev/caption", "ev/row", "ev/error"]);
  assert.equal(world.fields("ev", "GuiLayout").get("height"), 24 + 40 + 8 + 40);

  // A commit, here at tick 10, clears it and is reported.
  world.value("ev/field", "GuiTextInput", { numeric: true, value: 2 });
  await settle(root);
  assert.equal(shown(), undefined);
  assert.deepEqual(changes, [2]);
  // A rejection from that frame or earlier, delivered after it, stays
  // superseded; a later one shows.
  await reject("x", 10n);
  assert.equal(shown(), undefined);
  await reject("y", 11n);
  assert.equal(shown(), "y is not a number");

  // Blur ends the edit, and with it the error.
  world.effect(
    "ev/field",
    { kind: "focusChanged", focused: false, changed: true, part: 0 },
    "GuiTextInput",
    12n,
  );
  await settle(root);
  assert.equal(shown(), undefined);

  // A submission commits an edit even when the number stays the same.
  await reject("z", 13n);
  assert.equal(shown(), "z is not a number");
  world.effect(
    "ev/field",
    { kind: "submitted", text: "2.00" },
    "GuiTextInput",
    14n,
  );
  await settle(root);
  assert.equal(shown(), undefined);
  assert.deepEqual(changes, [2]);

  // A discarded edit, as by Escape, clears it without a commit, and a
  // rejection from that frame or earlier stays superseded.
  await reject("w", 15n);
  assert.equal(shown(), "w is not a number");
  world.effect(
    "ev/field",
    { kind: "discarded", text: "w" },
    "GuiTextInput",
    16n,
  );
  await settle(root);
  assert.equal(shown(), undefined);
  await reject("v", 16n);
  assert.equal(shown(), undefined);
  assert.deepEqual(changes, [2]);
});

/** Each of `actual`'s leading numbers within `tolerance` of `expected`'s. */
const nearAll = (
  actual: readonly number[],
  expected: readonly number[],
  tolerance = 1e-9,
) =>
  expected.forEach((value, index) =>
    assert.ok(
      Math.abs(actual[index]! - value) <= tolerance,
      `${actual} is not ${expected}`,
    ),
  );

/** The sheet's #54F4FF as the colour control holds it. */
const CYAN = {
  hue: (4 - 160 / 171) / 6,
  saturation: 171 / 255,
  value: 1,
  alpha: 1,
};

test("hex and channels convert to and from HSVA stably, keeping hue through grey and black", () => {
  // Every byte colour on a grid reads back as itself through the control's
  // single precision, with and without coverage.
  const levels = Array.from({ length: 16 }, (_, index) => index * 17);
  for (const red of levels)
    for (const green of levels)
      for (const blue of levels) {
        const hex = `#${[red, green, blue]
          .map((level) => level.toString(16).padStart(2, "0"))
          .join("")
          .toUpperCase()}`;
        assert.equal(formatHex(parseHex(hex)!), hex);
        assert.equal(formatHex(parseHex(`${hex}80`)!), `${hex}80`);
      }
  assert.deepEqual(hsvaToRgba(CYAN), {
    red: 84,
    green: 244,
    blue: 255,
    alpha: 1,
  });
  assert.equal(formatHex(CYAN), "#54F4FF");
  // Coverage as a byte after the colour, only while not opaque or wanted.
  const half = parseHex("#54f4ff80")!;
  near(half.alpha, 128 / 255);
  assert.equal(formatHex(half), "#54F4FF80");
  assert.equal(formatHex(half, false), "#54F4FF");
  assert.equal(parseHex(" 54F4FF ", { ...CYAN, alpha: 0.25 })?.alpha, 0.25);
  for (const text of ["#54F4F", "#54F4FFF", "54F4FG", "", "#54F4FF8"])
    assert.equal(parseHex(text), undefined, text);

  // Grey, white and black keep the previous hue, black its saturation too.
  const previous = { hue: 0.4, saturation: 0.6, value: 0.8, alpha: 1 };
  const grey = parseHex("#808080", previous)!;
  assert.equal(grey.hue, 0.4);
  assert.equal(grey.saturation, 0);
  near(grey.value, 128 / 255);
  assert.deepEqual(parseHex("#FFFFFF", previous), {
    hue: 0.4,
    saturation: 0,
    value: 1,
    alpha: 1,
  });
  assert.deepEqual(parseHex("#000000", previous), {
    hue: 0.4,
    saturation: 0.6,
    value: 0,
    alpha: 1,
  });
  // Bytes are clamped to their range.
  assert.deepEqual(rgbaToHsva({ red: 300, green: -4, blue: 0, alpha: 2 }), {
    hue: 0,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
});

const MAGENTA_PRESET = parseHex("#F4449F")!;

const picker = (props: Partial<ColorPickerProps> = {}) =>
  h(ColorPicker, {
    id: "pick",
    label: "COLOR",
    defaultValue: CYAN,
    presets: [{ value: MAGENTA_PRESET, label: "Magenta" }],
    ...props,
  });

test("ColorPicker: labels over the control, channels beside it, presets, hex in sRGB and the error line's room", async () => {
  const { world, draw } = await render(picker());
  const root = world.fields("pick", "GuiLayout");
  assert.equal(root.get("kind"), 2);
  assert.equal(root.get("width"), 240 + 16 + 112);
  assert.equal(root.get("height"), 24 + 200 + (8 + 32) + 2 * (8 + 40));
  assert.deepEqual(world.children("pick"), [
    "pick/caption",
    "pick/main",
    "pick/presets",
    "pick/hex",
  ]);
  // The caption over the field, the rails' names centred over the rails.
  assert.equal(world.fields("pick/label", "GuiLayout").get("margin_left"), 8);
  assert.deepEqual(world.tone("pick/label/text"), TOKENS.accent);
  near(
    world.fields("pick/hue", "GuiLayout").get("margin_left"),
    180 - lineOf("Hue", 13) / 2,
  );
  near(
    world.fields("pick/opacity", "GuiLayout").get("margin_left"),
    220 - lineOf("Opacity", 13) / 2,
  );
  // The runtime's control at its own size, holding the colour.
  assert.deepEqual(world.children("pick/main"), [
    "pick/control",
    "pick/channels",
  ]);
  const control = world.fields("pick/control", "GuiColor");
  assert.deepEqual(
    ["hue", "saturation", "value", "alpha", "alpha_rail"].map((field) =>
      control.get(field),
    ),
    [CYAN.hue, CYAN.saturation, 1, 1, true],
  );
  assert.equal(world.fields("pick/control", "GuiLayout").get("width"), 240);
  assert.equal(world.fields("pick/control", "GuiLayout").get("height"), 200);
  // R, G and B in bytes and A in percent, numeric fields without parts.
  assert.deepEqual(world.children("pick/channels"), [
    "pick/red",
    "pick/green",
    "pick/blue",
    "pick/alpha",
  ]);
  for (const [channel, number, max] of [
    ["red", 84, 255],
    ["green", 244, 255],
    ["blue", 255, 255],
    ["alpha", 100, 100],
  ] as const) {
    const field = world.fields(`pick/${channel}/field`, "GuiTextInput");
    assert.deepEqual(
      ["numeric", "value", "min", "max", "precision", "step_parts"].map(
        (name) => field.get(name),
      ),
      [true, number, 0, max, 0, false],
      channel,
    );
  }
  assert.equal(world.fields("pick/alpha/units", "CanvasText").get("text"), "%");
  // The hex entry states its colour space.
  assert.equal(
    world.fields("pick/hex/field", "GuiTextInput").get("text"),
    "#54F4FF",
  );
  assert.equal(
    world.fields("pick/hex/space", "CanvasText").get("text"),
    "sRGB",
  );
  // A preset is a Button painted in its colour over the secondary look.
  assert.equal(world.fields("pick/presets", "GuiGroup").get("selection"), 0);
  assert.equal(world.skin("pick/preset/0"), "secondary");
  assert.equal(
    world.fields("pick/preset/0", "GuiBehavior").get("semantic_label"),
    "Magenta",
  );
  const swatch = skinRows(world, "pick/preset/0")[0]![1].color as number[];
  nearAll(swatch, [((244 / 255 + 0.055) / 1.055) ** 2.4], 1e-6);
  near(swatch[3]!, 1);

  // Without alpha: no alpha rail, A field or Opacity, a narrower control.
  await draw(h(ColorPicker, { id: "pick", alpha: false }));
  assert.equal(
    world.fields("pick/control", "GuiColor").get("alpha_rail"),
    false,
  );
  assert.equal(world.entities.has("pick/alpha"), false);
  assert.equal(world.entities.has("pick/opacity"), false);
  assert.equal(world.fields("pick", "GuiLayout").get("width"), 200 + 16 + 112);
});

test("ColorPicker entries set the one colour and report what the runtime commits", async () => {
  const changes: unknown[] = [];
  const { world, root } = await render(
    picker({ onChange: (value) => changes.push(value) }),
  );
  await settle(root);
  const colors = () =>
    world.actions
      .filter(([symbol]) => symbol === "pick/control")
      .map(([, action]) => (action as { value: number[] }).value);

  // A drag reported by the runtime is the application's, and the channels
  // and hex follow it.
  world.value("pick/control", "GuiColor", {
    hue: 0,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
  await settle(root);
  assert.deepEqual(changes, [{ hue: 0, saturation: 1, value: 1, alpha: 1 }]);
  assert.equal(
    world.fields("pick/red/field", "GuiTextInput").get("value"),
    255,
  );
  assert.equal(
    world.fields("pick/green/field", "GuiTextInput").get("value"),
    0,
  );
  assert.equal(
    world.fields("pick/hex/field", "GuiTextInput").get("text"),
    "#FF0000",
  );
  assert.deepEqual(colors(), []);

  // A submitted hex sets the colour with the control's action; a grey keeps
  // the hue.
  const submit = (text: string, tick: bigint) =>
    world.effect(
      "pick/hex/field",
      { kind: "submitted", text },
      "GuiTextInput",
      tick,
    );
  submit("#00ff00", 20n);
  await settle(root);
  nearAll(colors().at(-1)!, [1 / 3, 1, 1, 1]);
  submit("#808080", 21n);
  await settle(root);
  const grey = colors().at(-1)!;
  assert.equal(grey[0], 0);
  assert.equal(grey[1], 0);

  // Text that is no hex shows the error and sets nothing.
  const before = colors().length;
  submit("#12345", 22n);
  await settle(root);
  assert.equal(colors().length, before);
  assert.equal(
    world.fields("pick/error/text", "CanvasText").get("text"),
    "Use #RRGGBB or #RRGGBBAA",
  );
  assert.equal(world.skin("pick/error"), "alertError");
  // The next committed colour clears it.
  world.value("pick/control", "GuiColor", {
    hue: 0.5,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
  await settle(root);
  assert.equal(world.entities.has("pick/error"), false);

  // A channel's number is an entry only while its field holds focus: the
  // field's own update from a drag sets nothing.
  world.value("pick/blue/field", "GuiTextInput", { numeric: true, value: 9 });
  await settle(root);
  assert.equal(colors().length, before);
  world.effect(
    "pick/green/field",
    { kind: "focusChanged", focused: true, changed: true, part: 0 },
    "GuiTextInput",
    1n,
  );
  world.value("pick/green/field", "GuiTextInput", {
    numeric: true,
    value: 128,
  });
  await settle(root);
  const typed = colors().at(-1)!;
  assert.deepEqual(
    hsvaToRgba({
      hue: typed[0]!,
      saturation: typed[1]!,
      value: typed[2]!,
      alpha: typed[3]!,
    }),
    { red: 0, green: 128, blue: 255, alpha: 1 },
  );
  // A percent of alpha changes the alpha alone.
  world.effect(
    "pick/alpha/field",
    { kind: "focusChanged", focused: true, changed: true, part: 0 },
    "GuiTextInput",
    2n,
  );
  world.value("pick/alpha/field", "GuiTextInput", { numeric: true, value: 50 });
  await settle(root);
  nearAll(colors().at(-1)!, [0.5, 1, 1, 0.5]);

  // A channel entry that is no number shows its error until that field
  // discards the edit.
  world.effect(
    "pick/green/field",
    { kind: "rejected", text: "x" },
    "GuiTextInput",
    30n,
  );
  await settle(root);
  assert.equal(
    world.fields("pick/error/text", "CanvasText").get("text"),
    "G: Not a number",
  );
  world.effect(
    "pick/green/field",
    { kind: "discarded", text: "x" },
    "GuiTextInput",
    31n,
  );
  await settle(root);
  assert.equal(world.entities.has("pick/error"), false);

  // Text typed in the hex field is entered on blur; once the next colour
  // has replaced it, a later blur enters nothing, so a drag's colour is
  // never written over by the text the field held before it.
  const focusHex = (focused: boolean, tick: bigint) =>
    world.effect(
      "pick/hex/field",
      { kind: "focusChanged", focused, changed: true, part: 0 },
      "GuiTextInput",
      tick,
    );
  focusHex(true, 3n);
  world.value("pick/hex/field", "GuiTextInput", { text: "#0000ff" });
  focusHex(false, 1000n);
  await settle(root);
  nearAll(colors().at(-1)!, [2 / 3, 1, 1, 1]);
  const entries = colors().length;
  world.value("pick/control", "GuiColor", {
    hue: 0.25,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
  focusHex(true, 1001n);
  focusHex(false, 1002n);
  await settle(root);
  assert.equal(colors().length, entries);

  // A preset is an explicit choice.
  world.effect("pick/preset/0", { kind: "pressed" });
  await settle(root);
  const preset = colors().at(-1)!;
  near(preset[0]!, MAGENTA_PRESET.hue);
  near(preset[2]!, MAGENTA_PRESET.value);

  // An application's own colour is set, and its echo not reported back.
  const own: unknown[] = [];
  const controlled = await render(
    picker({ value: CYAN, onChange: (value) => own.push(value) }),
  );
  await settle(controlled.root);
  await controlled.draw(
    picker({ value: MAGENTA_PRESET, onChange: (value) => own.push(value) }),
  );
  await settle(controlled.root);
  const written = controlled.world.actions.at(-1)!;
  assert.equal(written[0], "pick/control");
  controlled.world.value("pick/control", "GuiColor", { ...MAGENTA_PRESET });
  await settle(controlled.root);
  assert.deepEqual(own, []);
});
