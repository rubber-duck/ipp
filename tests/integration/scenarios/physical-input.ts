import type {
  Client,
  AssetWorldClient,
  Command,
  GuiObservedEffect,
  GuiWorldClient,
  HostClientBase,
  LifecycleFieldValue,
  LifecycleTargetWatch,
  PresentationView,
  RowsInput,
  GuiPhysicalContext,
} from "@ipp/client";
import { canvasOutput } from "../../../packages/ipp-client/src/references.js";
import { attachCanvasGuiInput } from "../../../packages/ipp-react/src/gui/input.js";
import { createGuiUnhandledInputGate } from "../../../packages/ipp-react/src/gui/scene-input.js";
import { preparePhysicalBlockers } from "./physical-input-blockers.js";
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
  GUI,
  selectSystems,
} from "../system-selections.js";
export { nativePresentationTransport } from "../../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../../packages/ipp-client/src/worker.js";

/** Logical units one browser wheel notch scrolls on these panels. */
const WHEEL_STEP = 100;

interface Contract {
  encodeBoundingShape(shape: {
    type: "box";
    min: [number, number, number];
    max: [number, number, number];
  }): Uint8Array<ArrayBuffer>;
  GuiTheme: { encodeParts(input: RowsInput): Uint8Array<ArrayBuffer> };
  guiPaintPartIndex(input: {
    part: "background";
    state?: "hovered" | "pressed";
  }): number;
}

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/** Fields of component `name` on `entity`, from its one component list. */
async function fieldsOf(client: Client, entity: bigint, name: string) {
  const component = client.components[name];
  check(component, `Target does not expose ${name}`);
  const page = await client.inspectPage({
    collection: "entities",
    target: entity,
    limit: 1,
  });
  return page.entities
    .find((item) => item.id === entity)
    ?.components.find((entry) => entry.component === component.id)?.fields;
}

/** One value record of the watched control: its field values at `tick`. */
interface ValueRecord {
  readonly tick: bigint;
  readonly values: readonly LifecycleFieldValue[] | null;
}

/** The value field a scenario step reads from each control. */
const VALUE_FIELD: Readonly<Record<string, string>> = {
  GuiSlider: "value",
  GuiTextInput: "text",
  GuiVirtualList: "offset_y",
};

export async function preparePhysicalInput(
  host: HostClientBase<Client>,
  contract: Contract,
  canvas: HTMLCanvasElement,
  fontBytes: ArrayBuffer,
) {
  const parent = (
    await host.createWorld({
      selectedSystems: selectSystems(ATTACHMENTS, GUI, SURFACE, LIFECYCLE),
      symbolicId: "physical-parent",
    })
  ).reference;
  const child = (
    await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "physical-child",
      canvas: { extent: [96, 64], unitsPerMetre: 1 },
    })
  ).reference;
  const parentClient = await host.openWorld(parent);
  const childClient = await host.openWorld(child);
  check("subscribeGuiEffects" in childClient, "GUI capability missing");
  const gui = childClient as Client & GuiWorldClient;
  check("createAsset" in childClient, "Asset capability missing");
  const font = await (childClient as unknown as AssetWorldClient).createAsset(
    17,
    fontBytes,
  );
  const effects: GuiObservedEffect[] = [];
  const failures: string[] = [];
  const unhandledWheel: (readonly [number, number])[] = [];
  const subscription = await gui.subscribeGuiEffects(
    (effect) => effects.push(effect),
    { classes: "all" },
  );
  const root = { kind: "alias", alias: 1 } as const;
  const button = { kind: "alias", alias: 2 } as const;
  const theme = successfulBatch(
    await childClient.batch([
      createEntity(3, "physical-theme"),
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 3 },
        component: childClient.components.GuiTheme!.id,
        fields: [
          {
            offset: childClient.components.GuiTheme!.fields.parts!.offset,
            value: {
              kind: "rows",
              value: contract.GuiTheme.encodeParts({
                nextSlot: 3,
                rows: new Map([
                  [
                    0,
                    {
                      part: contract.guiPaintPartIndex({ part: "background" }),
                      color: [1, 0, 0, 1],
                      corner_radius: [0, 0],
                      border_width: 0,
                    },
                  ],
                  [
                    1,
                    {
                      part: contract.guiPaintPartIndex({
                        part: "background",
                        state: "hovered",
                      }),
                      color: [0, 0, 1, 1],
                    },
                  ],
                  [
                    2,
                    {
                      part: contract.guiPaintPartIndex({
                        part: "background",
                        state: "pressed",
                      }),
                      color: [0, 1, 0, 1],
                    },
                  ],
                ]),
              }),
            },
          },
        ],
      },
    ]),
  );
  const authored = successfulBatch(
    await childClient.batch([
      createEntity(1, "physical-canvas"),
      insertComponent(childClient, "GuiFont", root, {
        source: font.source,
        font_size: 16,
      }),
      createEntity(2, "physical-button"),
      insertComponent(childClient, "GuiButton", button),
      insertComponent(childClient, "GuiLayout", button, {
        width: 96,
        height: 64,
      }),
      insertComponent(childClient, "GuiSkin", button, {
        theme: aliasId(theme, 3),
      }),
      {
        kind: "placeEntity",
        entity: button,
        placement: { parent: root, before: null },
      },
    ]),
  );
  const attachment = parentClient.components.WorldAttachment!;
  const anchor = { kind: "alias", alias: 2 } as const;
  // The anchor is a slot of the parent World's canvas under a top-level
  // root that a nested review turns into a VirtualList.
  const parentCommands: Command[] = [
    createEntity(1, "physical-root"),
    createEntity(2, "physical-anchor"),
    insertComponent(parentClient, "FlatSurface", anchor, {
      width: 96,
      height: 64,
    }),
    {
      kind: "insertComponent",
      entity: anchor,
      component: attachment.id,
      fields: [
        {
          offset: attachment.fields.child!.offset,
          value: { kind: "world", value: child },
        },
        {
          offset: attachment.fields.mode!.offset,
          value: { kind: "u32", value: 1 },
        },
      ],
    },
    {
      kind: "placeEntity",
      entity: anchor,
      placement: { parent: root, before: null },
    },
  ];
  const parentOutcome = successfulBatch(
    await parentClient.batch(parentCommands),
  );
  const output = canvasOutput(parent);
  const viewport = { width: 96, height: 64, devicePixelRatio: 1 };
  const binding = await host.setRootOutput(output, viewport);
  let view: PresentationView = await host.presentation.select(
    await host.presentation.surface(),
    binding,
  );
  const initialFrame = await host.presentation.frame(view);
  const input = await host.input.open(view);
  // One 100 px wheel notch scrolls 100 logical units, so these unit-density
  // panels scroll one logical unit per wheel pixel.
  const detach = attachCanvasGuiInput(canvas, input, {
    wheelStep: WHEEL_STEP,
    onError: (error) => failures.push(error.message),
    onUnhandled: (input) => {
      if (input.kind === "wheel") unhandledWheel.push(input.delta);
    },
  });
  await parentClient.close();
  let control = "GuiButton";
  const entity = { kind: "handle", id: aliasId(authored, 2) } as const;
  let dragTarget: bigint | undefined;
  let dragCut = 0;
  const capture = async () => {
    const capture = await host.presentation.capture(view);
    return {
      width: 96,
      height: 64,
      pixels: [...new Uint8Array(capture.pixels)],
      sequence: capture.sequence,
      publication: capture.publication,
    };
  };
  const reviewGate = createGuiUnhandledInputGate();
  let reviewInput: GuiPhysicalContext | undefined;
  let reviewDetach: (() => void) | undefined;
  let reviewParent: (Client & GuiWorldClient) | undefined;
  let reviewGateResult: Promise<boolean> | undefined;
  const reviewRemainders: number[] = [];
  // Value records of the current control's value field: the first reports
  // the current value, later ones each frame-end change. Each `value` step
  // consumes the records up to the one that reached its expected value.
  const values: ValueRecord[] = [];
  let valueCut = 0;
  let valueWatch: LifecycleTargetWatch | undefined;
  const watchValue = async (name: string) => {
    await valueWatch?.remove();
    values.length = 0;
    valueCut = 0;
    const descriptor = childClient.components[name]!;
    valueWatch = await childClient.watchLifecycle(
      [
        {
          target: {
            kind: "value",
            entity: entity.id,
            component: descriptor.id,
            fields: [descriptor.fields[VALUE_FIELD[name]!]!.offset],
          },
          kinds: 128,
        },
      ],
      (event) => {
        if (event.kind === "value")
          values.push({ tick: event.tick, values: event.values });
      },
    );
  };
  const fieldValue = async (name: string, entityId = entity.id) =>
    (await fieldsOf(childClient, entityId, name))?.[VALUE_FIELD[name]!];
  return {
    async blockers() {
      reviewDetach?.();
      await reviewInput?.close();
      return preparePhysicalBlockers(host, contract, canvas);
    },
    async prepareNested(capacity: number) {
      detach();
      await input.close();
      reviewDetach?.();
      await reviewInput?.close();
      reviewParent ??= (await host.openWorld(parent)) as Client &
        GuiWorldClient;
      successfulBatch(
        await reviewParent.batch([
          insertComponent(
            reviewParent,
            "GuiLayout",
            { kind: "handle", id: aliasId(parentOutcome, 1) },
            { width: 96, height: 64 },
          ),
          insertComponent(
            reviewParent,
            "GuiVirtualList",
            { kind: "handle", id: aliasId(parentOutcome, 1) },
            { item_count: 3, item_extent: 64, axis: 1, overscan: 0 },
          ),
          insertComponent(
            reviewParent,
            "GuiVirtualItem",
            { kind: "handle", id: aliasId(parentOutcome, 2) },
            { index: 0 },
          ),
        ]),
      );
      successfulBatch(
        await childClient.batch([
          {
            kind: "removeComponent",
            entity,
            component: childClient.components[control]!.id,
          },
          insertComponent(childClient, "GuiVirtualList", entity, {
            item_count: 1,
            item_extent: 64 + capacity,
            axis: 1,
            overscan: 0,
          }),
        ]),
      );
      control = "GuiVirtualList";
      await host.presentation.frame(view);
      reviewInput = await host.input.open(view);
      reviewRemainders.length = 0;
      reviewDetach = attachCanvasGuiInput(canvas, reviewInput, {
        wheelStep: WHEEL_STEP,
        unhandledInputGate: reviewGate,
        onError: (error) => failures.push(error.message),
        onUnhandled: (input) => {
          if (input.kind === "wheel") reviewRemainders.push(input.delta[1]);
        },
      });
    },
    async nestedValue(inner: number, outer: number) {
      const deadline = performance.now() + 10_000;
      for (;;) {
        const innerState = await fieldsOf(
          childClient,
          entity.id,
          "GuiVirtualList",
        );
        const outerState = await fieldsOf(
          reviewParent!,
          aliasId(parentOutcome, 1),
          "GuiVirtualList",
        );
        if (
          typeof innerState?.offset_y === "number" &&
          typeof outerState?.offset_y === "number" &&
          Math.abs(innerState.offset_y - inner) < 0.001 &&
          Math.abs(outerState.offset_y - outer) < 0.001
        ) {
          check(failures.length === 0, `Physical errors: ${failures}`);
          return {
            inner: innerState,
            outer: outerState,
            image: await capture(),
          };
        }
        check(
          performance.now() < deadline,
          `Nested scroll expected ${inner}/${outer}, got ${JSON.stringify([innerState?.offset_y, outerState?.offset_y])}; failures=${failures}`,
        );
        await host.presentation.frame(view);
      }
    },
    async armSaturatedWheel() {
      await reviewInput!.send({
        kind: "wheel",
        point: [0.5, 0.5],
        delta: [0, 10000],
      });
      reviewGateResult = reviewGate.scroll(new AbortController().signal);
    },
    armButton(button: "secondary" | "auxiliary") {
      reviewGateResult = reviewGate.pointerDown(
        1,
        button,
        new AbortController().signal,
      );
    },
    async sceneGate(expected: boolean) {
      check(reviewGateResult !== undefined, "Gate was not armed");
      check(
        (await reviewGateResult) === expected,
        `Wrong scene gate result, expected ${expected}`,
      );
      reviewGateResult = undefined;
      return [...reviewRemainders];
    },
    async prepareDragTarget() {
      const target = { kind: "alias", alias: 4 } as const;
      const outcome = successfulBatch(
        await childClient.batch([
          createEntity(4, "physical-drag-button"),
          insertComponent(childClient, "GuiButton", target),
          insertComponent(childClient, "GuiVirtualItem", target, { index: 15 }),
          insertComponent(childClient, "GuiLayout", target, {
            width: 96,
            height: 10,
          }),
          {
            kind: "placeEntity",
            entity: target,
            placement: { parent: entity, before: null },
          },
        ]),
      );
      dragTarget = aliasId(outcome, 4);
      await host.presentation.frame(view);
      dragCut = effects.length;
    },
    /** One stationary tap pressed the child button exactly once without
     * scrolling its container; later drag checks start after it. */
    async tappedDragTarget(offset: number) {
      check(dragTarget !== undefined, "Drag target was not installed");
      const deadline = performance.now() + 10_000;
      for (;;) {
        const pressed = effects
          .slice(dragCut)
          .filter(
            (effect) =>
              effect.target.entity === dragTarget &&
              effect.effect.kind === "pressed",
          );
        check(pressed.length <= 1, "One tap pressed the child twice");
        if (pressed.length === 1) {
          check(
            typeof pressed[0]!.source === "object",
            "Tap press lost its physical source",
          );
          const list = await fieldValue("GuiVirtualList");
          check(
            typeof list === "number" && Math.abs(list - offset) < 0.001,
            `Tap scrolled its container: ${JSON.stringify(list)}`,
          );
          check(failures.length === 0, `Physical errors: ${failures}`);
          dragCut = effects.length;
          return { pressed: pressed.length, image: await capture() };
        }
        check(
          performance.now() < deadline,
          `Tap did not press the child; failures=${failures}`,
        );
        await host.presentation.frame(view);
      }
    },
    async finishDragTarget() {
      check(dragTarget !== undefined, "Drag target was not installed");
      const relevant = effects
        .slice(dragCut)
        .filter((effect) => effect.target.entity === dragTarget);
      check(
        relevant.some((effect) => effect.effect.kind === "interactionChanged"),
        "Drag did not enter the child control",
      );
      check(
        !relevant.some((effect) => effect.effect.kind === "pressed"),
        "Scroll drag replayed the child's tap",
      );
      successfulBatch(
        await childClient.batch([
          { kind: "delete", entity: { kind: "handle", id: dragTarget } },
        ]),
      );
      dragTarget = undefined;
    },
    async text(expected: string, composing = false) {
      const deadline = performance.now() + 10_000;
      while (
        input.nativeText?.text !== expected ||
        Boolean(input.nativeText?.composition) !== composing
      ) {
        check(
          performance.now() < deadline,
          `Native text ${JSON.stringify(input.nativeText, (_, value) => (typeof value === "bigint" ? value.toString() : value))}, expected ${expected}/${composing}; failures=${failures}`,
        );
        await host.presentation.frame(view);
      }
      const snapshot = await fieldValue("GuiTextInput");
      check(
        snapshot === expected,
        "DOM/provisional text became committed authority",
      );
      return { native: input.nativeText, snapshot, image: await capture() };
    },
    /** Wait for the runtime's committed selection, in UTF-8 bytes. */
    async selection(start: number, end: number) {
      const deadline = performance.now() + 10_000;
      while (
        input.nativeText?.selectionStart !== start ||
        input.nativeText.selectionEnd !== end
      ) {
        check(
          performance.now() < deadline,
          `Native selection ${input.nativeText?.selectionStart}..${input.nativeText?.selectionEnd}, expected ${start}..${end}; failures=${failures}`,
        );
        await host.presentation.frame(view);
      }
      return [start, end] as const;
    },
    async replaceText(text: string) {
      const before = input.nativeText!;
      const component = childClient.components.GuiTextInput!;
      const [write] = componentFields(childClient, "GuiTextInput", { text });
      const [expected] = componentFields(childClient, "GuiTextInput", {
        text: before.text,
      });
      // Compare-and-set on the committed text the native buffer shows.
      successfulBatch(
        await childClient.batch([
          {
            kind: "setFieldIf",
            entity,
            component: component.id,
            field: write!,
            expected: expected!.value,
          },
        ]),
      );
      await host.presentation.frame(view);
      const outcome = await input.editText(before.fence, {
        kind: "text",
        text: "stale",
      });
      check(
        outcome.rejected === 1 && outcome.applied === 0,
        "Stale native edit overwrote replacement",
      );
      return outcome;
    },
    async blurredText() {
      const deadline = performance.now() + 10_000;
      while (input.nativeText !== null) {
        check(
          performance.now() < deadline,
          "Escape did not release native text",
        );
        await host.presentation.frame(view);
      }
      check(
        document.activeElement === canvas,
        "Native buffer did not return keyboard ownership to Canvas",
      );
    },
    async remainder(expected: number) {
      const deadline = performance.now() + 10_000;
      while (unhandledWheel.length === 0) {
        check(
          performance.now() < deadline,
          "Missing unconsumed physical wheel remainder",
        );
        await host.presentation.frame(view);
      }
      check(
        unhandledWheel.length === 1 &&
          unhandledWheel[0]![0] === 0 &&
          unhandledWheel[0]![1] === expected,
        `Wrong physical wheel fallback: ${unhandledWheel}`,
      );
      return unhandledWheel;
    },
    async role(next: "GuiSlider" | "GuiVirtualList" | "GuiTextInput") {
      const fields =
        next === "GuiSlider"
          ? { min: 0, max: 10, step: 1, value: 0 }
          : next === "GuiTextInput"
            ? { text: "ab" }
            : { item_count: 20, item_extent: 10, axis: 1, overscan: 0 };
      successfulBatch(
        await childClient.batch([
          {
            kind: "removeComponent",
            entity,
            component: childClient.components[control]!.id,
          },
          insertComponent(childClient, next, entity, fields),
        ]),
      );
      control = next;
      await watchValue(next);
      await host.presentation.frame(view);
      return capture();
    },
    async value(expected: number) {
      const deadline = performance.now() + 10_000;
      for (;;) {
        const snapshot = await fieldsOf(childClient, entity.id, control);
        const value = snapshot?.[VALUE_FIELD[control]!];
        // Only physical input writes the value here, so a value record that
        // reached `expected` after the previous step observed its commit.
        const index = values.findIndex(
          (record, index) =>
            index >= valueCut &&
            typeof record.values?.[0]?.value === "number" &&
            Math.abs(record.values[0].value - expected) < 0.001,
        );
        if (
          typeof value === "number" &&
          Math.abs(value - expected) < 0.001 &&
          index >= 0
        ) {
          valueCut = index + 1;
          check(failures.length === 0, `Physical errors: ${failures}`);
          return { snapshot, effect: values[index]!, image: await capture() };
        }
        check(
          performance.now() < deadline,
          `Physical value ${value}, expected ${expected}; records=${values.map((record) => `${record.tick}/${record.values?.[0]?.value}`)}; cut=${valueCut}; failures=${failures}`,
        );
        await host.presentation.frame(view);
      }
    },
    async image(expected: readonly number[]) {
      const deadline = performance.now() + 10_000;
      let previous = 0n;
      for (;;) {
        const capture = await host.presentation.capture(view, {
          afterSequence: previous,
        });
        previous = capture.sequence;
        const pixels = new Uint8Array(capture.pixels);
        const color = [
          ...pixels.slice((32 * 96 + 48) * 4, (32 * 96 + 48) * 4 + 4),
        ];
        if (
          expected.every(
            (value, channel) => Math.abs(value - color[channel]!) <= 8,
          )
        )
          return {
            width: 96,
            height: 64,
            pixels: [...pixels],
            sequence: capture.sequence,
            publication: capture.publication,
          };
        check(
          performance.now() < deadline,
          `Physical control pixels ${color}, expected ${expected}; failures=${failures}`,
        );
      }
    },
    async observed(expected: number) {
      const page = await fieldsOf(childClient, aliasId(authored, 2), control);
      check(
        effects.filter((effect) => effect.effect.kind === "pressed").length ===
          expected,
        `Wrong routed press count: ${effects.map((effect) => effect.effect.kind)}`,
      );
      check(
        effects.every(
          (effect) =>
            effect.target.world.id === child.id &&
            typeof effect.source === "object" &&
            effect.source.publication.host === initialFrame.publication.host &&
            effect.source.publication.revision >=
              initialFrame.publication.revision,
        ),
        "Physical effects lost routed publication or child identity",
      );
      check(failures.length === 0, `Physical errors: ${failures}`);
      return { effects, page };
    },
    async rebind() {
      const rebound = await host.setRootOutput(output, viewport);
      view = await host.presentation.select(view.surface, rebound);
      await host.presentation.frame(view);
      check(
        input.isClosed,
        "Equal root rebind did not revoke old physical context",
      );
      await input.send({ kind: "key", key: "enter" }).then(
        () => {
          throw new Error("Revoked physical context transmitted input");
        },
        () => {},
      );
    },
    async close() {
      await valueWatch?.remove();
      reviewDetach?.();
      await reviewInput?.close();
      await reviewParent?.close();
      detach();
      await input.close();
      await subscription.unsubscribe();
      await host.presentation.clear(view);
      await childClient.close();
      await host.destroyWorld(parent);
      await host.destroyWorld(child);
    },
  };
}
