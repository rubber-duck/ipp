import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate as turn } from "node:timers/promises";
import { createElement } from "react";
import {
  FieldKind,
  type BatchOutcome,
  type Command,
  type GuiObservedEffect,
  type LifecycleFieldValue,
  type LifecycleTargetWatch,
  type LifecycleWatchEvent,
} from "@ipp/client";
import {
  controlValueLayout,
  controlValues,
  dispatchGuiEffect,
  dispatchGuiFeedback,
  invokeControlValue,
  type GuiActionEvent,
  type GuiActionListeners,
  type GuiControlEvent,
  type GuiControlListeners,
  type GuiHsva,
} from "../src/gui/callbacks.js";
import { Behavior } from "../src/gui/components.js";
import { Button, Checkbox, Color } from "../src/gui/controls.js";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
} from "../src/index.js";

const unexpected = () => assert.fail("Unexpected callback");

const observed: GuiObservedEffect = {
  id: { world: { id: 1n, incarnation: 2n }, ordinal: 3n },
  target: {
    world: { id: 1n, incarnation: 2n },
    entity: 12n,
    component: 49,
    incarnation: 4n,
  },
  source: "semantic",
  tick: 5n,
  ancestry: [10n, 11n, 12n],
  effect: { kind: "pressed" },
};

test("control callback precedes pinned capture and bubble; exceptions are isolated", () => {
  const order: string[] = [];
  const errors: unknown[] = [];
  dispatchGuiEffect(
    observed,
    () => [
      {
        onPress: (event) => {
          assert.equal(event.target, observed.target);
          order.push("press");
          throw new Error("application");
        },
      },
    ],
    (entity) => [
      {
        onActionCapture: (event) => {
          assert.equal(event.currentTarget, entity);
          order.push(`c${entity}`);
        },
        onAction: () => order.push(`b${entity}`),
      },
    ],
    () => true,
    (error) => errors.push(error),
  );
  assert.deepEqual(order, ["press", "c10", "c11", "c12", "b12", "b11", "b10"]);
  assert.equal(errors.length, 1);
});

test("stopPropagation only affects JS propagation, while teardown fences reentrant delivery", () => {
  const order: string[] = [];
  const controls: GuiControlListeners[] = [
    { onPress: () => order.push("press") },
  ];
  const ancestors = (entity: bigint): GuiActionListeners[] => [
    {
      onActionCapture: (event) => {
        order.push(`c${entity}`);
        event.stopPropagation();
      },
    },
  ];
  dispatchGuiEffect(
    observed,
    () => controls,
    ancestors,
    () => true,
    unexpected,
  );
  assert.deepEqual(order, ["press", "c10"]);
  assert.equal(observed.effect.kind, "pressed");
  let live = true;
  dispatchGuiEffect(
    observed,
    () => [
      {
        onPress: () => {
          live = false;
        },
      },
    ],
    ancestors,
    () => live,
    unexpected,
  );
  assert.deepEqual(order, ["press", "c10"]);
});

test("submission carries the effect text and never dispatches text commit", () => {
  const submitted = {
    ...observed,
    effect: { kind: "submitted" as const, text: "committed" },
  };
  const values: unknown[] = [];
  dispatchGuiEffect(
    submitted,
    () => [
      {
        onSubmit: (event) => values.push(event.value, event.tick),
        onTextCommit: unexpected,
      },
    ],
    () => [],
    () => true,
    unexpected,
  );
  assert.deepEqual(values, ["committed", observed.tick]);
});

test("a rejection carries the refused text to onReject, then bubbles", () => {
  const refused = {
    ...observed,
    effect: { kind: "rejected" as const, text: "1,5" },
  };
  const order: unknown[] = [];
  dispatchGuiEffect(
    refused,
    () => [
      {
        onReject: (event) => order.push(event.value, event.tick),
        onSubmit: unexpected,
        onScalarCommit: unexpected,
      },
    ],
    (entity) => [
      {
        onAction: (event) => {
          if (event.kind === "effect")
            assert.equal(event.effect.kind, "rejected");
          order.push(entity);
        },
      },
    ],
    () => true,
    unexpected,
  );
  assert.deepEqual(order, ["1,5", observed.tick, 12n, 11n, 10n]);
});

test("a discard carries the discarded text to onDiscard, then bubbles", () => {
  const discarded = {
    ...observed,
    effect: { kind: "discarded" as const, text: "1,5" },
  };
  const order: unknown[] = [];
  dispatchGuiEffect(
    discarded,
    () => [
      {
        onDiscard: (event) => order.push(event.value, event.tick),
        onReject: unexpected,
        onSubmit: unexpected,
      },
    ],
    (entity) => [
      {
        onAction: (event) => {
          if (event.kind === "effect")
            assert.equal(event.effect.kind, "discarded");
          order.push(entity);
        },
      },
    ],
    () => true,
    unexpected,
  );
  assert.deepEqual(order, ["1,5", observed.tick, 12n, 11n, 10n]);
});

test("context request reaches onContextMenu with its point, then bubbles", () => {
  const requested = {
    ...observed,
    effect: { kind: "contextRequested" as const, point: [1.5, 2.5] as const },
  };
  const order: unknown[] = [];
  dispatchGuiEffect(
    requested,
    () => [
      {
        onContextMenu: (event) => order.push(event.point, event.target),
        onPress: unexpected,
      },
    ],
    (entity) => [
      {
        onAction: (event) => {
          assert.equal(event.kind, "effect");
          if (event.kind === "effect")
            assert.equal(event.effect.kind, "contextRequested");
          order.push(entity);
        },
      },
    ],
    () => true,
    unexpected,
  );
  assert.deepEqual(order, [[1.5, 2.5], observed.target, 12n, 11n, 10n]);
});

test("feedback and invalid pinned paths do not dispatch application callbacks", () => {
  let errors = 0;
  for (const effect of [
    {
      ...observed,
      effect: {
        kind: "focusChanged" as const,
        focused: true,
        changed: true,
        part: 0,
      },
    },
    { ...observed, ancestry: [10n] },
  ])
    dispatchGuiEffect(
      effect,
      () => [{ onPress: unexpected }],
      () => [{ onAction: unexpected }],
      () => true,
      () => errors++,
    );
  assert.equal(errors, 1);
});

test("feedback reaches only the target control's focus and interaction callbacks", () => {
  const received: unknown[] = [];
  const errors: unknown[] = [];
  const listeners: GuiControlListeners[] = [
    {
      onFocusChange: (event) => {
        received.push(["focus", event.focused, event.part, event.target]);
        throw new Error("application");
      },
      onInteractionChange: (event) =>
        received.push([
          "pointer",
          event.pointer,
          event.hovered,
          event.pressed,
          event.captured,
        ]),
      onPress: unexpected,
    },
  ];
  dispatchGuiFeedback(
    {
      ...observed,
      effect: { kind: "focusChanged", focused: true, changed: true, part: 1 },
    },
    () => listeners,
    () => true,
    (error) => errors.push(error),
  );
  dispatchGuiFeedback(
    {
      ...observed,
      effect: {
        kind: "interactionChanged",
        pointer: 7n,
        state: { hovered: true, pressed: false, captured: false },
        changed: true,
      },
    },
    () => listeners,
    () => true,
    unexpected,
  );
  // Application effects and a fenced root deliver nothing here.
  dispatchGuiFeedback(
    observed,
    () => listeners,
    () => true,
    unexpected,
  );
  dispatchGuiFeedback(
    {
      ...observed,
      effect: { kind: "focusChanged", focused: false, changed: true, part: 0 },
    },
    () => listeners,
    () => false,
    unexpected,
  );
  assert.deepEqual(received, [
    ["focus", true, 1, observed.target],
    ["pointer", 7n, true, false, false],
  ]);
  assert.equal(errors.length, 1);
});

const valueComponents: ReactWorldClient["components"] = {
  GuiButton: {
    id: 41,
    fields: {
      label: { offset: 0, kind: FieldKind.String },
      selected: { offset: 8, kind: FieldKind.Bool },
    },
  },
  GuiCheckbox: {
    id: 42,
    fields: {
      label: { offset: 0, kind: FieldKind.String },
      checked: { offset: 16, kind: FieldKind.Bool },
    },
  },
  GuiSlider: {
    id: 43,
    fields: {
      min: { offset: 0, kind: FieldKind.F32 },
      max: { offset: 4, kind: FieldKind.F32 },
      step: { offset: 8, kind: FieldKind.F32 },
      value: { offset: 12, kind: FieldKind.F32 },
      range: { offset: 16, kind: FieldKind.Bool },
      upper: { offset: 20, kind: FieldKind.F32 },
    },
  },
  GuiBehavior: {
    id: 45,
    fields: {
      enabled: { offset: 0, kind: FieldKind.Bool },
      visible: { offset: 1, kind: FieldKind.Bool },
    },
  },
  GuiTextInput: {
    id: 46,
    fields: {
      placeholder: { offset: 0, kind: FieldKind.String },
      text: { offset: 8, kind: FieldKind.String },
      numeric: { offset: 16, kind: FieldKind.Bool },
      value: { offset: 20, kind: FieldKind.F32 },
      min: { offset: 24, kind: FieldKind.F32 },
    },
  },
  GuiColor: {
    id: 47,
    fields: {
      hue: { offset: 0, kind: FieldKind.F32 },
      saturation: { offset: 4, kind: FieldKind.F32 },
      value: { offset: 8, kind: FieldKind.F32 },
      alpha: { offset: 12, kind: FieldKind.F32 },
      alpha_rail: { offset: 16, kind: FieldKind.Bool },
    },
  },
  GuiVirtualList: {
    id: 44,
    fields: Object.fromEntries(
      [
        ["item_count", FieldKind.U32],
        ["item_extent", FieldKind.F32],
        ["overscan", FieldKind.U32],
        ["axis", FieldKind.U32],
        ["offset_x", FieldKind.F32],
        ["offset_y", FieldKind.F32],
        ["anchor_index", FieldKind.U32],
        ["anchor_offset", FieldKind.F32],
        ["viewport_x", FieldKind.F32],
        ["viewport_y", FieldKind.F32],
        ["content_x", FieldKind.F32],
        ["content_y", FieldKind.F32],
        ["capacity_x", FieldKind.F32],
        ["capacity_y", FieldKind.F32],
        ["range_first", FieldKind.U32],
        ["range_last", FieldKind.U32],
      ].map(([name, kind], index) => [
        name,
        { offset: index * 4, kind: kind as FieldKind },
      ]),
    ),
  },
};

test("value layouts watch each control's value fields and decode them for its callback", () => {
  assert.equal(controlValueLayout(valueComponents, 99), undefined);
  const checkbox = controlValueLayout(valueComponents, 42)!;
  assert.deepEqual(checkbox.fields, [16]);
  assert.deepEqual(controlValues(checkbox, [{ offset: 16, value: true }]), {
    value: { kind: "checked", value: true },
  });
  // A Button's value is its selection; its label is not watched.
  const button = controlValueLayout(valueComponents, 41)!;
  assert.deepEqual(button.fields, [8]);
  assert.deepEqual(controlValues(button, [{ offset: 8, value: true }]), {
    value: { kind: "selected", value: true },
  });
  // A Behavior's value is its open state; enabled is not watched.
  const behavior = controlValueLayout(valueComponents, 45)!;
  assert.deepEqual(behavior.fields, [1]);
  assert.deepEqual(controlValues(behavior, [{ offset: 1, value: false }]), {
    value: { kind: "visible", value: false },
  });
  // A slider's value; a range's two values come in one record.
  const slider = controlValueLayout(valueComponents, 43)!;
  assert.deepEqual(slider.fields, [12, 16, 20]);
  const sliderValues = (value: unknown, range: boolean) => [
    { offset: 12, value: value as number },
    { offset: 16, value: range },
    { offset: 20, value: 0.75 },
  ];
  assert.deepEqual(controlValues(slider, sliderValues(0.5, false)), {
    value: { kind: "scalar", value: 0.5 },
  });
  assert.deepEqual(controlValues(slider, sliderValues(0.25, true)), {
    value: { kind: "scalar", value: 0.25, upper: 0.75 },
  });
  assert.throws(
    () => controlValues(slider, sliderValues("0.5", false)),
    /GuiSlider.value is not a number/,
  );
  // A text input's value is its text, or a numeric input's number, which
  // its scalar callback observes; its range is not watched.
  const input = controlValueLayout(valueComponents, 46)!;
  assert.deepEqual(input.fields, [8, 16, 20]);
  const inputValues = (numeric: boolean) => [
    { offset: 8, value: "typed" },
    { offset: 16, value: numeric },
    { offset: 20, value: 1.25 },
  ];
  assert.deepEqual(controlValues(input, inputValues(false)), {
    value: { kind: "text", value: "typed" },
  });
  assert.deepEqual(controlValues(input, inputValues(true)), {
    value: { kind: "scalar", value: 1.25 },
  });
  const list = controlValueLayout(valueComponents, 44)!;
  // Configuration fields other than the item count are not watched.
  assert.deepEqual(
    list.fields,
    [0, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60],
  );
  const values: LifecycleFieldValue[] = list.fields.map((offset) => ({
    offset,
    value: offset / 4,
  }));
  assert.deepEqual(controlValues(list, values), {
    value: {
      kind: "scroll",
      value: { offset: [4, 5], anchorIndex: 6, anchorOffset: 7 },
    },
    range: {
      viewport: [8, 9],
      content: [10, 11],
      capacity: [12, 13],
      itemCount: 0,
      first: 14,
      last: 15,
    },
  });
  // A colour's four channels come in one record, without its rail flag.
  const color = controlValueLayout(valueComponents, 47)!;
  assert.deepEqual(color.fields, [0, 4, 8, 12]);
  const hsva = { hue: 0.5, saturation: 0.25, value: 1, alpha: 0.75 };
  assert.deepEqual(
    controlValues(
      color,
      color.fields.map((offset, index) => ({
        offset,
        value: Object.values(hsva)[index]!,
      })),
    ),
    { value: { kind: "color", value: hsva } },
  );
  const received: unknown[] = [];
  const target = observed.target;
  const listeners: GuiControlListeners = {
    onToggle: (event) => received.push(["toggle", event.value]),
    onSelectedChange: (event) => received.push(["selected", event.value]),
    onVisibleChange: (event) => received.push(["visible", event.value]),
    onScalarCommit: (event) =>
      received.push(["scalar", event.value, event.upper]),
    onColorCommit: (event) => received.push(["color", event.value]),
    onTextCommit: (event) => received.push(["text", event.value]),
    onScroll: (event) => received.push(["scroll", event.value.offset]),
  };
  for (const value of [
    { kind: "checked", value: false },
    { kind: "selected", value: true },
    { kind: "visible", value: false },
    { kind: "scalar", value: 2 },
    { kind: "scalar", value: 2, upper: 4 },
    { kind: "color", value: hsva },
    { kind: "text", value: "typed" },
    {
      kind: "scroll",
      value: { offset: [0, 3], anchorIndex: 0, anchorOffset: 0 },
    },
  ] as const)
    invokeControlValue(listeners, { target, tick: 1n, value });
  assert.deepEqual(received, [
    ["toggle", false],
    ["selected", true],
    ["visible", false],
    ["scalar", 2, undefined],
    ["scalar", 2, 4],
    ["color", hsva],
    ["text", "typed"],
    ["scroll", [0, 3]],
  ]);
});

/**
 * A World with one lifecycle watch: batches apply at once, and the test
 * delivers value records of the watched checkbox.
 */
function valueWorld() {
  let listener: ((event: LifecycleWatchEvent) => void) | undefined;
  const watches: { targets: unknown[] }[] = [];
  let nextEntity = 10n;
  const client: ReactWorldClient = {
    session: 1n,
    schemaHash: 1n,
    worldReference: { id: 1n, incarnation: 1n },
    components: valueComponents,
    async batch(operations: Command[]): Promise<BatchOutcome> {
      return {
        ok: true,
        batchId: 1n,
        tick: 1n,
        aliases: operations.flatMap((operation) =>
          operation.kind === "create"
            ? [{ alias: operation.alias, id: nextEntity++ }]
            : [],
        ),
        symbols: [],
        effects: [],
      };
    },
    async watchLifecycle(targets, next) {
      watches.push({ targets: [...targets] });
      listener = next;
      const watch: LifecycleTargetWatch = {
        world: { id: 1n, incarnation: 1n },
        baselines: targets.map((selection, index) => ({
          member: { output: 5n, generation: BigInt(index + 1) },
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
      };
      return watch;
    },
  };
  /** A record of the watched control's value fields. */
  const record = (tick: bigint, values: LifecycleFieldValue[] | null) =>
    listener!({
      kind: "value",
      world: { id: 1n, incarnation: 1n },
      output: 5n,
      member: { output: 5n, generation: 3n },
      tick,
      values,
    });
  /** A record of the watched control's value field at `offset`. */
  const deliver = (tick: bigint, checked: boolean | null, offset = 16) =>
    record(tick, checked === null ? null : [{ offset, value: checked }]);
  return { client, watches, deliver, record };
}

async function settled(): Promise<void> {
  for (let index = 0; index < 10; index++) await turn();
}

test("value callbacks fire from value records, current value first, only when the value changes", async () => {
  const world = valueWorld();
  const toggles: GuiControlEvent<boolean>[] = [];
  const root = createRoot(world.client, {
    onError: (error) => assert.fail(error),
  });
  const scene = (onToggle?: (event: GuiControlEvent<boolean>) => void) =>
    createElement(
      Entity,
      { id: "row" },
      createElement(Checkbox, {
        checked: false,
        ...(onToggle ? { onToggle } : {}),
      }),
    );
  try {
    await root.render(scene((event) => toggles.push(event)));
    await settled();
    // One watch: entity, component and the checkbox's value field.
    assert.deepEqual(world.watches[0]!.targets, [
      { target: { kind: "entity", entity: 10n }, kinds: 4 },
      { target: { kind: "component", entity: 10n, component: 42 }, kinds: 104 },
      {
        target: { kind: "value", entity: 10n, component: 42, fields: [16] },
        kinds: 128,
      },
    ]);
    world.deliver(5n, false);
    await settled();
    world.deliver(6n, true);
    await settled();
    world.deliver(7n, true);
    await settled();
    assert.deepEqual(
      toggles.map((event) => [event.tick, event.value]),
      [
        [5n, false],
        [6n, true],
      ],
      "the first record reports the current value; an equal value does not fire",
    );
    assert.deepEqual(toggles[0]!.target, {
      world: { id: 1n, incarnation: 1n },
      entity: 10n,
      component: 42,
      incarnation: 3n,
    });
    // Absence reports nothing and clears the delivered state, so the value
    // fires again when the component returns.
    world.deliver(8n, null);
    await settled();
    assert.equal(toggles.length, 2);
    world.deliver(9n, true);
    await settled();
    assert.deepEqual(toggles.map((event) => [event.tick, event.value]).at(-1), [
      9n,
      true,
    ]);
    // Without value callbacks the control is not observed; registering one
    // again observes anew and receives the current value first.
    const later: GuiControlEvent<boolean>[] = [];
    await root.render(scene());
    await root.render(scene((event) => later.push(event)));
    await settled();
    assert.equal(world.watches.length, 2);
    world.deliver(10n, true);
    await settled();
    assert.deepEqual(
      later.map((event) => [event.tick, event.value]),
      [[10n, true]],
    );
    assert.equal(toggles.length, 3, "the removed listener is not called");
  } finally {
    await root.unmount();
  }
});

test("a render that only replaces a value callback sends nothing, and the new callback receives values", async () => {
  const world = valueWorld();
  const batch = world.client.batch.bind(world.client);
  let batches = 0;
  world.client.batch = (operations) => {
    batches++;
    return batch(operations);
  };
  const received: [string, boolean][] = [];
  const root = createRoot(world.client, {
    onError: (error) => assert.fail(error),
  });
  const scene = (name: string) =>
    createElement(
      Entity,
      { id: "row" },
      createElement(Checkbox, {
        checked: false,
        onToggle: (event: GuiControlEvent<boolean>) =>
          received.push([name, event.value]),
      }),
    );
  try {
    await root.render(scene("first"));
    await settled();
    world.deliver(5n, false);
    await settled();
    const sent = batches;
    await root.render(scene("second"));
    await settled();
    assert.equal(batches, sent, "a callback is not a declared value");
    assert.equal(world.watches.length, 1, "the control stays observed");
    world.deliver(6n, true);
    await settled();
    assert.deepEqual(received, [
      ["first", false],
      ["second", true],
    ]);
  } finally {
    await root.unmount();
  }
});

test("a colour control delivers its four channels in one event per changed record", async () => {
  const world = valueWorld();
  const colors: GuiControlEvent<GuiHsva>[] = [];
  const root = createRoot(world.client, {
    onError: (error) => assert.fail(error),
  });
  try {
    await root.render(
      createElement(
        Entity,
        { id: "picker" },
        createElement(Color, {
          hue: 0.5,
          saturation: 0.25,
          value: 1,
          alpha: 1,
          alpha_rail: true,
          onColorCommit: (event) => colors.push(event),
        }),
      ),
    );
    await settled();
    // The watch observes the four channels, not the rail flag.
    assert.deepEqual(world.watches[0]!.targets.at(-1), {
      target: {
        kind: "value",
        entity: 10n,
        component: 47,
        fields: [0, 4, 8, 12],
      },
      kinds: 128,
    });
    const channels = (hue: number, saturation: number) => [
      { offset: 0, value: hue },
      { offset: 4, value: saturation },
      { offset: 8, value: 1 },
      { offset: 12, value: 0.5 },
    ];
    world.record(5n, channels(0.5, 0.25));
    await settled();
    world.record(6n, channels(0.5, 0.25));
    await settled();
    // A drag that moved two channels in one frame is one event.
    world.record(7n, channels(0.75, 0.5));
    await settled();
    assert.deepEqual(
      colors.map((event) => [event.tick, event.value]),
      [
        [5n, { hue: 0.5, saturation: 0.25, value: 1, alpha: 0.5 }],
        [7n, { hue: 0.75, saturation: 0.5, value: 1, alpha: 0.5 }],
      ],
    );
  } finally {
    await root.unmount();
  }
});

test("a Button's selection reaches onSelectedChange whoever writes it", async () => {
  const world = valueWorld();
  const selections: [bigint, boolean][] = [];
  const root = createRoot(world.client, {
    onError: (error) => assert.fail(error),
  });
  try {
    await root.render(
      createElement(
        Entity,
        { id: "tab" },
        createElement(Button, {
          label: "Signals",
          selected: false,
          onSelectedChange: (event) =>
            selections.push([event.tick, event.value]),
        }),
      ),
    );
    await settled();
    // The watch covers the Button's selection only.
    assert.deepEqual(world.watches[0]!.targets.at(-1), {
      target: { kind: "value", entity: 10n, component: 41, fields: [8] },
      kinds: 128,
    });
    // The current value first, then a runtime write, as a selecting group
    // makes when its item is activated, and the write clearing it again.
    world.deliver(5n, false, 8);
    await settled();
    world.deliver(6n, true, 8);
    await settled();
    world.deliver(7n, false, 8);
    await settled();
    assert.deepEqual(selections, [
      [5n, false],
      [6n, true],
      [7n, false],
    ]);
  } finally {
    await root.unmount();
  }
});

test("a Behavior's open state reaches onVisibleChange, then bubbles to action listeners", async () => {
  const world = valueWorld();
  // Action listeners observe effects too; none arrive here.
  world.client.subscribeGuiEffects = async () => ({
    id: 1n as never,
    world: { id: 1n, incarnation: 1n },
    start: { tick: 0n } as never,
    closed: new Promise(() => {}),
    unsubscribe: async () => ({ tick: 0n }) as never,
  });
  const order: unknown[] = [];
  const root = createRoot(world.client, {
    onError: (error) => assert.fail(error),
  });
  // An outer Entity listening for actions, around the overlay's Entity.
  const scene = (observe: boolean, listen: boolean) =>
    createElement(
      Entity,
      {
        id: "panel",
        ...(listen
          ? {
              onAction: (event: GuiActionEvent) =>
                order.push([
                  "action",
                  event.kind === "value" ? event.value : event.kind,
                  event.target.entity,
                  event.currentTarget,
                ]),
            }
          : {}),
      },
      createElement(
        Children,
        null,
        createElement(
          Entity,
          { id: "menu" },
          createElement(Behavior, {
            visible: true,
            ...(observe
              ? {
                  onVisibleChange: (event: GuiControlEvent<boolean>) =>
                    order.push(["own", event.value, event.target.entity]),
                }
              : {}),
          }),
        ),
      ),
    );
  try {
    // Neither a callback nor an action listener: the Behavior is not
    // observed.
    await root.render(scene(false, false));
    await settled();
    assert.equal(world.watches.length, 0);
    // An action listener alone observes it, as it observes every control.
    await root.render(scene(false, true));
    await settled();
    assert.deepEqual(world.watches[0]!.targets.at(-1), {
      target: { kind: "value", entity: 11n, component: 45, fields: [1] },
      kinds: 128,
    });
    world.deliver(5n, true, 1);
    await settled();
    // With its callback too: the callback first, then the listener on the
    // enclosing Entity, the event naming the overlay's entity as its target.
    await root.render(scene(true, true));
    await settled();
    world.deliver(6n, false, 1);
    await settled();
    world.deliver(7n, false, 1);
    await settled();
    const value = (visible: boolean) => ({ kind: "visible", value: visible });
    assert.deepEqual(order, [
      ["action", value(true), 11n, 10n],
      ["own", true, 11n],
      ["own", false, 11n],
      ["action", value(false), 11n, 10n],
    ]);
  } finally {
    await root.unmount();
  }
});
