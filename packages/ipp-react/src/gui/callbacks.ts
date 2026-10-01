import type {
  ComponentDescriptor,
  ComponentFieldValue,
  GuiObservedEffect,
  GuiTarget,
  LifecycleFieldValue,
} from "@ipp/client";

/** A momentary press effect of one control. */
export interface GuiPressEvent {
  readonly id: GuiObservedEffect["id"];
  readonly target: GuiTarget;
  readonly source: GuiObservedEffect["source"];
  readonly tick: bigint;
  readonly ancestry: readonly bigint[];
}

/** A momentary submission of a text input, carrying the submitted text. */
export interface GuiSubmitEvent extends GuiPressEvent {
  readonly value: string;
}

/** A control's field values at the end of the evaluated frame `tick`. */
export interface GuiControlEvent<Value> {
  readonly target: GuiTarget;
  readonly tick: bigint;
  readonly value: Value;
}

/** Scroll position of a ScrollView or VirtualList; anchors are 0 for a ScrollView. */
export interface GuiScrollPosition {
  readonly offset: readonly [number, number];
  readonly anchorIndex: number;
  readonly anchorOffset: number;
}

/** A control value that propagates to `onAction` listeners. */
export type GuiControlValue =
  | { readonly kind: "checked"; readonly value: boolean }
  | { readonly kind: "scalar"; readonly value: number }
  | { readonly kind: "text"; readonly value: string }
  | { readonly kind: "scroll"; readonly value: GuiScrollPosition };

export interface GuiPropagation {
  /** The entity whose declaration's listener runs. */
  readonly currentTarget: bigint;
  readonly phase: "capture" | "bubble";
  stopPropagation(): void;
}

/** What an action listener observes, before propagation details. */
export type GuiActionDetail =
  | (GuiPressEvent & {
      readonly kind: "effect";
      readonly effect: GuiObservedEffect["effect"];
    })
  | (GuiControlEvent<GuiControlValue> & { readonly kind: "value" });

export type GuiActionEvent = GuiPropagation & GuiActionDetail;

/**
 * Scroll geometry and visible range of a ScrollView or VirtualList. A
 * ScrollView has no item count and an empty range.
 */
export interface GuiVirtualRange {
  readonly target: GuiTarget;
  readonly tick: bigint;
  readonly viewport: readonly [number, number];
  readonly content: readonly [number, number];
  readonly capacity: readonly [number, number];
  readonly itemCount: number | null;
  /** First visible item, inclusive. */
  readonly first: number;
  /** End of the visible items, exclusive. */
  readonly last: number;
}

export type GuiPressListener = (event: GuiPressEvent) => void;
export type GuiToggleListener = (event: GuiControlEvent<boolean>) => void;
export type GuiScalarCommitListener = (event: GuiControlEvent<number>) => void;
export type GuiTextCommitListener = (event: GuiControlEvent<string>) => void;
export type GuiTextSubmitListener = (event: GuiSubmitEvent) => void;
export type GuiActionListener = (event: GuiActionEvent) => void;
export type GuiRangeChangeListener = (range: GuiVirtualRange) => void;
export type GuiScrollListener = (
  event: GuiControlEvent<GuiScrollPosition>,
) => void;

export interface GuiActionListeners {
  onAction?: GuiActionListener | undefined;
  onActionCapture?: GuiActionListener | undefined;
}

export interface GuiControlListeners {
  onRangeChange?: GuiRangeChangeListener | undefined;
  onScroll?: GuiScrollListener | undefined;
  onPress?: GuiPressListener | undefined;
  onToggle?: GuiToggleListener | undefined;
  onScalarCommit?: GuiScalarCommitListener | undefined;
  onTextCommit?: GuiTextCommitListener | undefined;
  onSubmit?: GuiTextSubmitListener | undefined;
}

export const controlCallbackNames = [
  "onRangeChange",
  "onScroll",
  "onPress",
  "onToggle",
  "onScalarCommit",
  "onTextCommit",
  "onSubmit",
] as const;

/** Callbacks fed by field observations rather than momentary effects. */
export const controlValueCallbackNames = [
  "onRangeChange",
  "onScroll",
  "onToggle",
  "onScalarCommit",
  "onTextCommit",
] as const;

/** Watched fields of each control kind that has values, by component name. */
const controlValueFields = {
  GuiCheckbox: ["checked"],
  GuiSlider: ["value"],
  GuiTextInput: ["text"],
  GuiScrollView: [
    "offset_x",
    "offset_y",
    "viewport_x",
    "viewport_y",
    "content_x",
    "content_y",
    "capacity_x",
    "capacity_y",
  ],
  GuiVirtualList: [
    "item_count",
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
  ],
} as const;

/** Where one control component's watched values live. */
export interface GuiControlValueLayout {
  readonly control: keyof typeof controlValueFields;
  /** Watched field offsets, ascending. */
  readonly fields: readonly number[];
  /** The field name of each watched offset. */
  readonly names: ReadonlyMap<number, string>;
}

/**
 * The watched fields of the control component `component`, or undefined when
 * the component is not a control with values.
 */
export function controlValueLayout(
  components: Readonly<Record<string, ComponentDescriptor>>,
  component: number,
): GuiControlValueLayout | undefined {
  for (const [name, descriptor] of Object.entries(components)) {
    if (descriptor.id !== component) continue;
    if (!Object.hasOwn(controlValueFields, name)) return undefined;
    const control = name as keyof typeof controlValueFields;
    const names = new Map<number, string>();
    for (const field of controlValueFields[control]) {
      const offset = descriptor.fields[field]?.offset;
      if (offset === undefined)
        throw new Error(`${name} has no field ${field}`);
      names.set(offset, field);
    }
    return {
      control,
      fields: Object.freeze(
        [...names.keys()].sort((left, right) => left - right),
      ),
      names,
    };
  }
  return undefined;
}

/** What one value record of a control carries for its callbacks. */
export interface GuiControlValues {
  /** The control's value, observed by its value callback and by actions. */
  readonly value: GuiControlValue;
  /** Scroll geometry and range; ScrollView and VirtualList only. */
  readonly range?: Omit<GuiVirtualRange, "target" | "tick">;
}

/** Derive a control's callback values from its watched field values. */
export function controlValues(
  layout: GuiControlValueLayout,
  values: readonly LifecycleFieldValue[],
): GuiControlValues {
  const fields = new Map<string, ComponentFieldValue>();
  for (const { offset, value } of values) {
    const name = layout.names.get(offset);
    if (name !== undefined) fields.set(name, value);
  }
  const field = (name: string, kind: "boolean" | "number" | "string") => {
    const value = fields.get(name);
    if (typeof value !== kind)
      throw new Error(`${layout.control}.${name} is not a ${kind}`);
    return value;
  };
  const number = (name: string) => field(name, "number") as number;
  switch (layout.control) {
    case "GuiCheckbox":
      return {
        value: {
          kind: "checked",
          value: field("checked", "boolean") as boolean,
        },
      };
    case "GuiSlider":
      return { value: { kind: "scalar", value: number("value") } };
    case "GuiTextInput":
      return {
        value: { kind: "text", value: field("text", "string") as string },
      };
    case "GuiScrollView":
    case "GuiVirtualList": {
      const list = layout.control === "GuiVirtualList";
      const pair = (name: string) =>
        [number(`${name}_x`), number(`${name}_y`)] as const;
      return {
        value: {
          kind: "scroll",
          value: {
            offset: pair("offset"),
            anchorIndex: list ? number("anchor_index") : 0,
            anchorOffset: list ? number("anchor_offset") : 0,
          },
        },
        range: {
          viewport: pair("viewport"),
          content: pair("content"),
          capacity: pair("capacity"),
          itemCount: list ? number("item_count") : null,
          first: list ? number("range_first") : 0,
          last: list ? number("range_last") : 0,
        },
      };
    }
  }
}

/** The control callback that observes each kind of control value. */
export const controlValueCallback = {
  checked: "onToggle",
  scalar: "onScalarCommit",
  text: "onTextCommit",
  scroll: "onScroll",
} as const satisfies Record<
  GuiControlValue["kind"],
  (typeof controlValueCallbackNames)[number]
>;

/** Invoke the control listener that observes `event.value`. */
export function invokeControlValue(
  listeners: GuiControlListeners | undefined,
  event: GuiControlEvent<GuiControlValue>,
): void {
  const { value } = event;
  const base = { target: event.target, tick: event.tick };
  switch (value.kind) {
    case "checked":
      return listeners?.onToggle?.({ ...base, value: value.value });
    case "scalar":
      return listeners?.onScalarCommit?.({ ...base, value: value.value });
    case "text":
      return listeners?.onTextCommit?.({ ...base, value: value.value });
    case "scroll":
      return listeners?.onScroll?.({ ...base, value: value.value });
  }
}

/** One entity on an action's propagation path. */
export interface GuiPropagationStep {
  readonly currentTarget: bigint;
  /** The action listeners declared for this entity, read when dispatch reaches it. */
  listeners(): readonly GuiActionListeners[];
}

/**
 * Run the target control's own listeners, then `onActionCapture` from the
 * root-most step inward and `onAction` from the target outward. A listener's
 * `stopPropagation()` skips the remaining steps; exceptions are reported and
 * never interrupt dispatch.
 */
export function dispatchGuiAction(
  detail: GuiActionDetail,
  controls: readonly (() => void)[],
  path: readonly GuiPropagationStep[],
  live: () => boolean,
  report: (error: unknown) => unknown,
): void {
  const invoke = (callback: (() => void) | undefined) => {
    if (!live()) return;
    try {
      callback?.();
    } catch (error) {
      report(error);
    }
  };
  for (const control of controls) invoke(control);
  let stopped = false;
  for (const phase of ["capture", "bubble"] as const) {
    for (const step of phase === "capture" ? path : path.toReversed()) {
      for (const listener of step.listeners()) {
        if (stopped || !live()) return;
        const callback =
          phase === "capture" ? listener.onActionCapture : listener.onAction;
        if (!callback) continue;
        invoke(() =>
          callback(
            Object.freeze({
              ...detail,
              currentTarget: step.currentTarget,
              phase,
              stopPropagation: () => {
                stopped = true;
              },
            }),
          ),
        );
      }
    }
  }
}

/**
 * Dispatch a press or submission to the target control's listeners, then along
 * the effect's runtime ancestry. Other effects have no application callbacks.
 */
export function dispatchGuiEffect(
  observed: GuiObservedEffect,
  controls: () => readonly GuiControlListeners[],
  ancestors: (entity: bigint) => readonly GuiActionListeners[],
  live: () => boolean,
  report: (error: unknown) => unknown,
): void {
  if (!live()) return;
  const { effect, ...base } = observed;
  if (effect.kind !== "pressed" && effect.kind !== "submitted") return;
  if (observed.ancestry.at(-1) !== observed.target.entity) {
    report(new Error("GUI effect omitted its pinned target ancestry"));
    return;
  }
  dispatchGuiAction(
    { ...base, kind: "effect", effect },
    controls().map((listener) =>
      effect.kind === "pressed"
        ? () => listener.onPress?.(base)
        : () => listener.onSubmit?.({ ...base, value: effect.text }),
    ),
    observed.ancestry.map((entity) => ({
      currentTarget: entity,
      listeners: () => ancestors(entity),
    })),
    live,
    report,
  );
}
