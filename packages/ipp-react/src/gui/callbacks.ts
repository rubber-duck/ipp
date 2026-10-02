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

/**
 * A momentary submission of a text input, carrying the submitted text: a
 * numeric input's committed number as it shows it.
 */
export interface GuiSubmitEvent extends GuiPressEvent {
  readonly value: string;
}

/**
 * Text a numeric text input refused to commit, on Enter or blur, because it
 * does not parse; its number is unchanged. `value` is the refused text, for
 * an error shown beside the field.
 */
export type GuiRejectEvent = GuiSubmitEvent;

/**
 * A numeric text input's pending edit ended without being committed or
 * refused, such as by Escape, which shows the formatted number again; its
 * number is unchanged. `value` is the discarded text. A client clears what it
 * showed for the edit, such as a rejection's error.
 */
export type GuiDiscardEvent = GuiSubmitEvent;

/**
 * A momentary context request on a control: a secondary press, the Menu key
 * or Shift+F10. The runtime has focused the control; `point` is a logical
 * point of its canvas, the press point or, for a key, the bottom-left corner
 * of the control's visible box, where a client may open its menu.
 */
export interface GuiContextMenuEvent extends GuiPressEvent {
  readonly point: readonly [number, number];
}

/**
 * A change of a control's logical focus, from the GUI feedback stream:
 * whether the control holds focus after the change, and the part focus names
 * (a range slider's lower thumb 0 or upper thumb 1), or the part it left. A
 * control with one part reports part 0; focus moving between the parts of one
 * control reports the new part with `focused` true.
 */
export interface GuiFocusChangeEvent extends GuiPressEvent {
  readonly focused: boolean;
  readonly part: number;
}

/**
 * A change of one pointer's interaction with a control, from the GUI feedback
 * stream: that pointer's flags on the control after the change. Several
 * pointers may hover a control at once.
 */
export interface GuiInteractionEvent extends GuiPressEvent {
  readonly pointer: bigint;
  readonly hovered: boolean;
  readonly pressed: boolean;
  readonly captured: boolean;
}

/** A control's field values at the end of the evaluated frame `tick`. */
export interface GuiControlEvent<Value> {
  readonly target: GuiTarget;
  readonly tick: bigint;
  readonly value: Value;
}

/**
 * A slider's committed values: `value`, and for a range also `upper`, read
 * from one field observation, so a change of either or both is one event.
 */
export interface GuiScalarEvent extends GuiControlEvent<number> {
  /** A range's upper value; absent for a slider with one value. */
  readonly upper?: number;
}

/**
 * A colour control's colour: hue in turns from red, saturation, value and
 * alpha, each in 0..1. Hue, saturation and value are the HSV model on
 * sRGB-encoded values; alpha is linear coverage.
 */
export interface GuiHsva {
  readonly hue: number;
  readonly saturation: number;
  readonly value: number;
  readonly alpha: number;
}

/** Scroll position of a ScrollView or VirtualList; anchors are 0 for a ScrollView. */
export interface GuiScrollPosition {
  readonly offset: readonly [number, number];
  readonly anchorIndex: number;
  readonly anchorOffset: number;
}

/**
 * A value a value callback observes, which then propagates to `onAction`
 * listeners: a control's value, or `visible`, a Behavior's open state.
 */
export type GuiControlValue =
  | { readonly kind: "checked"; readonly value: boolean }
  | { readonly kind: "selected"; readonly value: boolean }
  | { readonly kind: "visible"; readonly value: boolean }
  | {
      readonly kind: "scalar";
      readonly value: number;
      /** A range slider's upper value. */
      readonly upper?: number;
    }
  | { readonly kind: "text"; readonly value: string }
  | { readonly kind: "scroll"; readonly value: GuiScrollPosition }
  | { readonly kind: "color"; readonly value: GuiHsva };

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
export type GuiContextMenuListener = (event: GuiContextMenuEvent) => void;
export type GuiFocusChangeListener = (event: GuiFocusChangeEvent) => void;
export type GuiInteractionListener = (event: GuiInteractionEvent) => void;
export type GuiToggleListener = (event: GuiControlEvent<boolean>) => void;
/**
 * A Button's `selected` field changed, whoever wrote it: the application, or
 * the runtime selecting a group item.
 */
export type GuiSelectedChangeListener = (
  event: GuiControlEvent<boolean>,
) => void;
/**
 * A Behavior's `visible` field changed, whoever wrote it: the application,
 * or the runtime opening or closing an overlay as its mode decides.
 */
export type GuiVisibleChangeListener = (
  event: GuiControlEvent<boolean>,
) => void;
export type GuiScalarCommitListener = (event: GuiScalarEvent) => void;
/**
 * A colour control's colour changed, whoever wrote it: one event per frame
 * that changed any of its channels, carrying all four.
 */
export type GuiColorCommitListener = (event: GuiControlEvent<GuiHsva>) => void;
export type GuiTextCommitListener = (event: GuiControlEvent<string>) => void;
export type GuiTextSubmitListener = (event: GuiSubmitEvent) => void;
export type GuiTextRejectListener = (event: GuiRejectEvent) => void;
export type GuiTextDiscardListener = (event: GuiDiscardEvent) => void;
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
  onContextMenu?: GuiContextMenuListener | undefined;
  onFocusChange?: GuiFocusChangeListener | undefined;
  onInteractionChange?: GuiInteractionListener | undefined;
  onToggle?: GuiToggleListener | undefined;
  onSelectedChange?: GuiSelectedChangeListener | undefined;
  onVisibleChange?: GuiVisibleChangeListener | undefined;
  onScalarCommit?: GuiScalarCommitListener | undefined;
  onColorCommit?: GuiColorCommitListener | undefined;
  onTextCommit?: GuiTextCommitListener | undefined;
  onSubmit?: GuiTextSubmitListener | undefined;
  onReject?: GuiTextRejectListener | undefined;
  onDiscard?: GuiTextDiscardListener | undefined;
}

/** Callbacks of every control fed by the GUI feedback stream. */
export interface GuiFeedbackListeners {
  onFocusChange?: GuiFocusChangeListener;
  onInteractionChange?: GuiInteractionListener;
}

export const controlCallbackNames = [
  "onRangeChange",
  "onScroll",
  "onPress",
  "onContextMenu",
  "onFocusChange",
  "onInteractionChange",
  "onToggle",
  "onSelectedChange",
  "onVisibleChange",
  "onScalarCommit",
  "onColorCommit",
  "onTextCommit",
  "onSubmit",
  "onReject",
  "onDiscard",
] as const;

/** Callbacks fed by feedback effects: focus and pointer interaction changes. */
export const controlFeedbackCallbackNames = [
  "onFocusChange",
  "onInteractionChange",
] as const;

/** Callbacks fed by field observations rather than momentary effects. */
export const controlValueCallbackNames = [
  "onRangeChange",
  "onScroll",
  "onToggle",
  "onSelectedChange",
  "onVisibleChange",
  "onScalarCommit",
  "onColorCommit",
  "onTextCommit",
] as const;

/**
 * Watched fields of each control kind that has values, and of the Behavior
 * whose open state a callback observes, by component name.
 */
const controlValueFields = {
  GuiBehavior: ["visible"],
  GuiButton: ["selected"],
  GuiCheckbox: ["checked"],
  GuiSlider: ["value", "upper", "range"],
  GuiTextInput: ["text", "numeric", "value"],
  GuiColor: ["hue", "saturation", "value", "alpha"],
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
 * The watched fields of the control component `component`, or of a
 * Behavior, or undefined when the component has no observed values.
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
  /** The value, observed by its value callback and by actions. */
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
    case "GuiBehavior":
      return {
        value: {
          kind: "visible",
          value: field("visible", "boolean") as boolean,
        },
      };
    case "GuiButton":
      return {
        value: {
          kind: "selected",
          value: field("selected", "boolean") as boolean,
        },
      };
    case "GuiCheckbox":
      return {
        value: {
          kind: "checked",
          value: field("checked", "boolean") as boolean,
        },
      };
    case "GuiSlider":
      return {
        value: field("range", "boolean")
          ? { kind: "scalar", value: number("value"), upper: number("upper") }
          : { kind: "scalar", value: number("value") },
      };
    case "GuiTextInput":
      return {
        value: field("numeric", "boolean")
          ? { kind: "scalar", value: number("value") }
          : { kind: "text", value: field("text", "string") as string },
      };
    case "GuiColor":
      return {
        value: {
          kind: "color",
          value: {
            hue: number("hue"),
            saturation: number("saturation"),
            value: number("value"),
            alpha: number("alpha"),
          },
        },
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

/** The callback that observes each kind of observed value. */
export const controlValueCallback = {
  checked: "onToggle",
  selected: "onSelectedChange",
  visible: "onVisibleChange",
  scalar: "onScalarCommit",
  text: "onTextCommit",
  scroll: "onScroll",
  color: "onColorCommit",
} as const satisfies Record<
  GuiControlValue["kind"],
  (typeof controlValueCallbackNames)[number]
>;

/** Invoke the listener that observes `event.value`. */
export function invokeControlValue(
  listeners: GuiControlListeners | undefined,
  event: GuiControlEvent<GuiControlValue>,
): void {
  const { value } = event;
  const base = { target: event.target, tick: event.tick };
  switch (value.kind) {
    case "checked":
      return listeners?.onToggle?.({ ...base, value: value.value });
    case "selected":
      return listeners?.onSelectedChange?.({ ...base, value: value.value });
    case "visible":
      return listeners?.onVisibleChange?.({ ...base, value: value.value });
    case "scalar":
      return listeners?.onScalarCommit?.(
        value.upper === undefined
          ? { ...base, value: value.value }
          : { ...base, value: value.value, upper: value.upper },
      );
    case "text":
      return listeners?.onTextCommit?.({ ...base, value: value.value });
    case "scroll":
      return listeners?.onScroll?.({ ...base, value: value.value });
    case "color":
      return listeners?.onColorCommit?.({ ...base, value: value.value });
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
 * Dispatch a feedback effect, a focus or pointer interaction change, to the
 * target control's own `onFocusChange` or `onInteractionChange`. Feedback
 * reaches no action listener and does not propagate.
 */
export function dispatchGuiFeedback(
  observed: GuiObservedEffect,
  controls: () => readonly GuiControlListeners[],
  live: () => boolean,
  report: (error: unknown) => unknown,
): void {
  const { effect, ...base } = observed;
  const deliveries =
    effect.kind === "focusChanged"
      ? controls().map(
          (listener) => () =>
            listener.onFocusChange?.(
              Object.freeze({
                ...base,
                focused: effect.focused,
                part: effect.part,
              }),
            ),
        )
      : effect.kind === "interactionChanged"
        ? controls().map(
            (listener) => () =>
              listener.onInteractionChange?.(
                Object.freeze({
                  ...base,
                  pointer: effect.pointer,
                  ...effect.state,
                }),
              ),
          )
        : [];
  for (const deliver of deliveries) {
    if (!live()) return;
    try {
      deliver();
    } catch (error) {
      report(error);
    }
  }
}

/**
 * Dispatch a press, submission, rejection, discard or context request to the
 * target control's listeners, then along the effect's runtime ancestry.
 * Feedback effects reach only `dispatchGuiFeedback`.
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
  if (
    effect.kind !== "pressed" &&
    effect.kind !== "submitted" &&
    effect.kind !== "rejected" &&
    effect.kind !== "discarded" &&
    effect.kind !== "contextRequested"
  )
    return;
  if (observed.ancestry.at(-1) !== observed.target.entity) {
    report(new Error("GUI effect omitted its pinned target ancestry"));
    return;
  }
  dispatchGuiAction(
    { ...base, kind: "effect", effect },
    controls().map((listener) =>
      effect.kind === "pressed"
        ? () => listener.onPress?.(base)
        : effect.kind === "submitted"
          ? () => listener.onSubmit?.({ ...base, value: effect.text })
          : effect.kind === "rejected"
            ? () => listener.onReject?.({ ...base, value: effect.text })
            : effect.kind === "discarded"
              ? () => listener.onDiscard?.({ ...base, value: effect.text })
              : () =>
                  listener.onContextMenu?.({ ...base, point: effect.point }),
    ),
    observed.ancestry.map((entity) => ({
      currentTarget: entity,
      listeners: () => ancestors(entity),
    })),
    live,
    report,
  );
}
