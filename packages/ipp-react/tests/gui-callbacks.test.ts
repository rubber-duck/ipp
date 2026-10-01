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
  invokeControlValue,
  type GuiActionListeners,
  type GuiControlEvent,
  type GuiControlListeners,
} from "../src/gui/callbacks.js";
import { Checkbox } from "../src/gui/controls.js";
import { Entity, createRoot, type ReactWorldClient } from "../src/index.js";

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

test("feedback and invalid pinned paths do not dispatch application callbacks", () => {
  let errors = 0;
  for (const effect of [
    {
      ...observed,
      effect: { kind: "focusChanged" as const, focused: true, changed: true },
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

const valueComponents: ReactWorldClient["components"] = {
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
  const slider = controlValueLayout(valueComponents, 43)!;
  assert.deepEqual(slider.fields, [12]);
  assert.deepEqual(controlValues(slider, [{ offset: 12, value: 0.5 }]), {
    value: { kind: "scalar", value: 0.5 },
  });
  assert.throws(
    () => controlValues(slider, [{ offset: 12, value: "0.5" }]),
    /GuiSlider.value is not a number/,
  );
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
  const received: unknown[] = [];
  const target = observed.target;
  const listeners: GuiControlListeners = {
    onToggle: (event) => received.push(["toggle", event.value]),
    onScalarCommit: (event) => received.push(["scalar", event.value]),
    onTextCommit: (event) => received.push(["text", event.value]),
    onScroll: (event) => received.push(["scroll", event.value.offset]),
  };
  for (const value of [
    { kind: "checked", value: false },
    { kind: "scalar", value: 2 },
    { kind: "text", value: "typed" },
    {
      kind: "scroll",
      value: { offset: [0, 3], anchorIndex: 0, anchorOffset: 0 },
    },
  ] as const)
    invokeControlValue(listeners, { target, tick: 1n, value });
  assert.deepEqual(received, [
    ["toggle", false],
    ["scalar", 2],
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
  const deliver = (tick: bigint, checked: boolean | null) =>
    listener!({
      kind: "value",
      world: { id: 1n, incarnation: 1n },
      output: 5n,
      member: { output: 5n, generation: 3n },
      tick,
      values: checked === null ? null : [{ offset: 16, value: checked }],
    });
  return { client, watches, deliver };
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
