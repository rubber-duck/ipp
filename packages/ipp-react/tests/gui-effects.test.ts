/** P04 effect subscription coverage: real React controls through callback
 * retention into the lifetime-fenced listener registry, then through
 * GuiCommits acknowledgement wiring fed by client-shaped observation batches.
 *
 * Headless and node-runnable: control factories describe nodes, the
 * reconciler tree retains their JS-only listeners, GuiCommits subscribes on
 * acknowledgement, and committed observations dispatch exactly once with
 * capture/bubble paths, conflicts, cancellations and scene-bound unhandled
 * inputs. Byte-exact wire proof lives in the protocol client suite; the host
 * publication emission is the integration owner's follow-up.
 */
import assert from "node:assert/strict";
import test from "node:test";
import type {
  BatchOutcome,
  Command,
  GuiEdit,
  GuiInputCommand,
  GuiInspectedNode,
  GuiInspectResponse,
  GuiNodeHandle,
  GuiNodeStyle,
} from "@ipp/client";
import { ENTITY_HOST_TYPE } from "../src/components.js";
import type { ReactWorldClient } from "../src/contract.js";
import { GuiCommits } from "../src/gui/commits.js";
import { TestGuiCommits } from "./gui-test-commits.js";
import {
  Button,
  Checkbox,
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  Slider,
  TextInput,
} from "../src/gui/controls.js";
import {
  GUI_COLUMN_HOST_TYPE,
  GUI_ROOT_HOST_TYPE,
} from "../src/gui/components.js";
import {
  dispatchGuiObservations,
  GuiEffectSubscriptions,
  isCommittedEffect,
  type GuiCallbackResolution,
  type GuiCommittedEffect,
  type GuiControlListenerRecord,
  type GuiObservationBatch,
  type GuiObservationSummary,
} from "../src/gui/callbacks.js";
import {
  ReactWorldTree,
  retainedNodeCallbacks,
  type ReactWorldElementProps,
} from "../src/tree.js";

const EFFECT_ENTITY = 100n;
const EFFECT_ROOT = 3n;

function stubClient(): ReactWorldClient {
  return {
    components: { GuiRoot: {} },
    session: 7n,
  } as unknown as ReactWorldClient;
}

/** Describe one column holding a button and a checkbox from real factories. */
function describedControls() {
  const presses: unknown[] = [];
  const toggles: unknown[] = [];
  const scalars: unknown[] = [];
  const texts: unknown[] = [];
  const button = Button({
    label: "Go",
    onPress: (event) => void presses.push(event),
  });
  const checkbox = Checkbox({
    checked: false,
    onToggle: (event) => void toggles.push(event),
  });
  const slider = Slider({
    value: 0.5,
    onScalarCommit: (event) => void scalars.push(event),
  });
  const textInput = TextInput({
    text: "a",
    onTextCommit: (event) => void texts.push(event),
  });
  assert.equal(button.type, GUI_BUTTON_HOST_TYPE);
  assert.equal(checkbox.type, GUI_CHECKBOX_HOST_TYPE);
  assert.equal(slider.type, GUI_SLIDER_HOST_TYPE);
  assert.equal(textInput.type, GUI_TEXT_INPUT_HOST_TYPE);
  const tree = new ReactWorldTree(stubClient());
  const entity = tree.instance(ENTITY_HOST_TYPE, { id: "panel" });
  const root = tree.instance(GUI_ROOT_HOST_TYPE, {});
  const column = tree.instance(GUI_COLUMN_HOST_TYPE, {});
  const buttonNode = tree.instance(
    GUI_BUTTON_HOST_TYPE,
    button.props as unknown as ReactWorldElementProps,
  );
  const checkboxNode = tree.instance(
    GUI_CHECKBOX_HOST_TYPE,
    checkbox.props as unknown as ReactWorldElementProps,
  );
  const sliderNode = tree.instance(
    GUI_SLIDER_HOST_TYPE,
    slider.props as unknown as ReactWorldElementProps,
  );
  const textNode = tree.instance(
    GUI_TEXT_INPUT_HOST_TYPE,
    textInput.props as unknown as ReactWorldElementProps,
  );
  entity.children.push(root);
  root.children.push(column);
  column.children.push(buttonNode, checkboxNode, sliderNode, textNode);
  tree.children.push(entity);
  const described = tree.describe();
  assert.equal(described.gui.length, 1);
  return {
    tree,
    buttonNode,
    nodes: described.gui[0]!.nodes,
    signature: described.gui[0]!.signature,
    presses,
    toggles,
    scalars,
    texts,
  };
}

test("real control listeners are retained JS-only on described nodes", () => {
  const { nodes, presses, toggles, scalars, texts } = describedControls();
  const byType = (type: string) => nodes.find((node) => node.type === type)!;
  const button = retainedNodeCallbacks(byType(GUI_BUTTON_HOST_TYPE));
  assert.equal(typeof button.onPress, "function");
  assert.equal(button.onToggle, undefined);
  const checkbox = retainedNodeCallbacks(byType(GUI_CHECKBOX_HOST_TYPE));
  assert.equal(typeof checkbox.onToggle, "function");
  assert.equal(checkbox.onPress, undefined);
  assert.equal(
    typeof retainedNodeCallbacks(byType(GUI_SLIDER_HOST_TYPE)).onScalarCommit,
    "function",
  );
  assert.equal(
    typeof retainedNodeCallbacks(byType(GUI_TEXT_INPUT_HOST_TYPE)).onTextCommit,
    "function",
  );
  assert.equal(
    retainedNodeCallbacks(byType(GUI_COLUMN_HOST_TYPE)).onPress,
    undefined,
  );
  // Listeners observe without transport side effects.
  assert.deepEqual([presses, toggles, scalars, texts], [[], [], [], []]);
});

test("callback-only changes resubmit nothing but refresh retention", () => {
  const first = describedControls();
  const before = first.signature;
  const replacement = () => {};
  first.buttonNode.props = {
    ...(first.buttonNode.props as Record<string, unknown>),
    onPress: replacement,
  };
  const again = first.tree.describe();
  assert.equal(again.gui[0]!.signature, before);
  const node = again.gui[0]!.nodes.find(
    (entry) => entry.type === GUI_BUTTON_HOST_TYPE,
  )!;
  assert.equal(retainedNodeCallbacks(node).onPress, replacement);
});

test("guards accept pinned paths and ticks, fail closed otherwise", () => {
  assert.equal(
    isCommittedEffect({
      kind: "buttonPressed",
      entity: 100n,
      rootIncarnation: EFFECT_ROOT,
      node: 20,
      lifetime: 1,
      path: [10, 20],
      sourceTick: 11n,
      effectTick: 12n,
    }),
    true,
  );
  assert.equal(
    isCommittedEffect({
      kind: "controlCommitted",
      entity: 100n,
      rootIncarnation: EFFECT_ROOT,
      node: 30,
      lifetime: 1,
      value: { kind: "bool", value: true },
      revision: 2,
      path: [10, 30],
      sourceTick: 11n,
      effectTick: 12n,
    }),
    true,
  );
  assert.equal(
    isCommittedEffect({
      kind: "buttonPressed",
      entity: 100n,
      rootIncarnation: EFFECT_ROOT,
      node: 20,
      lifetime: 1,
      path: [10, 20.5],
    }),
    false,
  );
  assert.equal(
    isCommittedEffect({
      kind: "buttonPressed",
      entity: 100n,
      rootIncarnation: EFFECT_ROOT,
      node: 20,
      lifetime: 1,
      sourceTick: 11,
    }),
    false,
  );
});

interface FeedHarness {
  subs: GuiEffectSubscriptions;
  parentOf: GuiCallbackResolution["parentOf"];
  seen: string[];
  errors: Error[];
}

function feedHarness(): FeedHarness {
  const subs = new GuiEffectSubscriptions();
  const seen: string[] = [];
  const errors: Error[] = [];
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 10, {
    lifetime: 1,
    kind: "container",
    onActionCapture: () => void seen.push("capture:10"),
    onAction: () => void seen.push("bubble:10"),
  });
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 20, {
    lifetime: 1,
    kind: "button",
    name: "Go",
    onPress: (event) =>
      void seen.push(`press:20@${event.sourceTick}/${event.effectTick}`),
    onAction: () => void seen.push("bubble:20"),
  });
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 30, {
    lifetime: 1,
    kind: "checkbox",
    onToggle: (event) =>
      void seen.push(`toggle:${event.value}@${event.revision}`),
  });
  assert.equal(subs.size, 3);
  return {
    subs,
    parentOf: () =>
      new Map([
        [20, 10],
        [30, 10],
        [10, undefined],
      ]),
    seen,
    errors,
  };
}

test("exactly-once press follows the pinned runtime path with ticks", () => {
  const { subs, parentOf, seen, errors } = feedHarness();
  const summary = subs.feed(
    [
      {
        kind: "buttonPressed",
        entity: 100n,
        rootIncarnation: EFFECT_ROOT,
        node: 20,
        lifetime: 1,
        path: [10, 20],
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
    parentOf,
    (error) => void errors.push(error),
  );
  assert.deepEqual(summary, { delivered: 1, skipped: 0 });
  assert.deepEqual(errors, []);
  assert.deepEqual(seen, [
    "press:20@11/12",
    "capture:10",
    "bubble:20",
    "bubble:10",
  ]);
});

test("change commits deliver values and revisions through the registry", () => {
  const { subs, parentOf, seen, errors } = feedHarness();
  const summary = subs.feed(
    [
      {
        kind: "controlCommitted",
        entity: 100n,
        rootIncarnation: EFFECT_ROOT,
        node: 30,
        lifetime: 1,
        value: { kind: "bool", value: true },
        revision: 2,
        path: [10, 30],
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
    parentOf,
    (error) => void errors.push(error),
  );
  assert.deepEqual(summary, { delivered: 1, skipped: 0 });
  assert.deepEqual(errors, []);
  assert.deepEqual(seen, ["toggle:true@2", "capture:10", "bubble:10"]);
});

test("same entity and node identities stay isolated across root incarnations", () => {
  const subs = new GuiEffectSubscriptions();
  const seen: bigint[] = [];
  for (const rootIncarnation of [3n, 4n]) {
    subs.subscribe(EFFECT_ENTITY, rootIncarnation, 20, {
      lifetime: 1,
      kind: "button",
      onPress: (event) => void seen.push(event.rootIncarnation),
    });
  }
  const summary = subs.feed(
    [
      {
        kind: "buttonPressed",
        entity: EFFECT_ENTITY,
        rootIncarnation: 4n,
        node: 20,
        lifetime: 1,
      },
    ],
    () => new Map([[20, undefined]]),
  );
  assert.deepEqual(summary, { delivered: 1, skipped: 0 });
  assert.deepEqual(seen, [4n]);
  assert.equal(subs.unsubscribe(EFFECT_ENTITY, 3n, 20), true);
  assert.equal(subs.size, 1);
});

test("pinned path wins over stale acknowledged ancestry", () => {
  const { subs, seen, errors } = feedHarness();
  // The acknowledged tree moved on (20 now under 99), but the effect pins
  // the committed ancestry: capture runs on 10, never on 99.
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 99, {
    lifetime: 1,
    kind: "container",
    onActionCapture: () => void seen.push("capture:99"),
  });
  const summary = subs.feed(
    [
      {
        kind: "buttonPressed",
        entity: 100n,
        rootIncarnation: EFFECT_ROOT,
        node: 20,
        lifetime: 1,
        path: [10, 20],
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
    () =>
      new Map([
        [20, 99],
        [99, undefined],
        [10, undefined],
      ]),
    (error) => void errors.push(error),
  );
  assert.deepEqual(summary, { delivered: 1, skipped: 0 });
  assert.deepEqual(errors, []);
  assert.ok(seen.includes("capture:10"), `saw ${seen}`);
  assert.ok(!seen.includes("capture:99"), `saw ${seen}`);
});

test("stale pinned path falls back to acknowledged ancestry with a report", () => {
  const { subs, parentOf, seen, errors } = feedHarness();
  const summary = subs.feed(
    [
      {
        kind: "buttonPressed",
        entity: 100n,
        rootIncarnation: EFFECT_ROOT,
        node: 20,
        lifetime: 1,
        // Names a retired tree: the last entry is not the target.
        path: [10, 30],
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
    parentOf,
    (error) => void errors.push(error),
  );
  assert.deepEqual(summary, { delivered: 1, skipped: 0 });
  assert.equal(errors.length, 1);
  assert.match(errors[0]!.message, /stale runtime path/);
  assert.ok(seen.includes("press:20@11/12"), `saw ${seen}`);
  assert.ok(seen.includes("capture:10"), `saw ${seen}`);
});

test("delayed effects dispatch; teardown skips without redelivery", () => {
  const { subs, parentOf, seen, errors } = feedHarness();
  const effect: GuiCommittedEffect = {
    kind: "buttonPressed",
    entity: 100n,
    rootIncarnation: EFFECT_ROOT,
    node: 20,
    lifetime: 1,
    path: [10, 20],
    sourceTick: 11n,
    effectTick: 12n,
  };
  // A later frame still dispatches the delayed press exactly once.
  assert.deepEqual(
    subs.feed([effect], parentOf, (error) => void errors.push(error)),
    { delivered: 1, skipped: 0 },
  );
  // Removal teardown drops the record: later arrivals stay silent.
  assert.equal(subs.unsubscribe(EFFECT_ENTITY, EFFECT_ROOT, 20), true);
  assert.equal(subs.size, 3 - 1);
  assert.deepEqual(
    subs.feed([effect], parentOf, (error) => void errors.push(error)),
    { delivered: 0, skipped: 1 },
  );
  // Re-acknowledgement under a new lifetime retires the old one: delayed
  // effects naming it report stale, current ones deliver.
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 20, {
    lifetime: 2,
    kind: "button",
    onPress: () => void seen.push("press:20v2"),
  });
  assert.deepEqual(
    subs.feed([effect], parentOf, (error) => void errors.push(error)),
    { delivered: 0, skipped: 1 },
  );
  assert.equal(errors.length, 1);
  assert.match(errors[0]!.message, /Stale GUI control effect for node 20/);
  assert.deepEqual(
    subs.feed(
      [{ ...effect, lifetime: 2 }],
      parentOf,
      (error) => void errors.push(error),
    ),
    { delivered: 1, skipped: 0 },
  );
  assert.ok(seen.includes("press:20v2"), `saw ${seen}`);
  // Unmount teardown drops everything.
  subs.clear();
  assert.equal(subs.size, 0);
  assert.deepEqual(
    subs.feed([effect], parentOf, (error) => void errors.push(error)),
    { delivered: 0, skipped: 1 },
  );
});

test("stopPropagation still controls callbacks only; commits stand", () => {
  const { subs, parentOf, seen, errors } = feedHarness();
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 10, {
    lifetime: 1,
    kind: "container",
    onActionCapture: (event) => {
      seen.push("capture:10");
      event.stopPropagation();
    },
    onAction: () => void seen.push("bubble:10"),
  });
  const summary = subs.feed(
    [
      {
        kind: "buttonPressed",
        entity: 100n,
        rootIncarnation: EFFECT_ROOT,
        node: 20,
        lifetime: 1,
        path: [10, 20],
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
    parentOf,
    (error) => void errors.push(error),
  );
  assert.deepEqual(summary, { delivered: 1, skipped: 0 });
  assert.deepEqual(errors, []);
  assert.ok(seen.includes("press:20@11/12"), `saw ${seen}`);
  assert.ok(seen.includes("capture:10"), `saw ${seen}`);
  assert.ok(!seen.includes("bubble:20"), `saw ${seen}`);
  assert.ok(!seen.includes("bubble:10"), `saw ${seen}`);
});

test("throwing control and action listeners do not abort later effects", () => {
  const { subs, parentOf, seen, errors } = feedHarness();
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 10, {
    lifetime: 1,
    kind: "container",
    onActionCapture: () => {
      throw new Error("capture failed");
    },
    onAction: () => void seen.push("bubble:10"),
  });
  subs.subscribe(EFFECT_ENTITY, EFFECT_ROOT, 20, {
    lifetime: 1,
    kind: "button",
    onPress: () => {
      throw new Error("press failed");
    },
  });
  const summary = subs.feed(
    [
      {
        kind: "buttonPressed",
        entity: EFFECT_ENTITY,
        rootIncarnation: EFFECT_ROOT,
        node: 20,
        lifetime: 1,
      },
      {
        kind: "controlCommitted",
        entity: EFFECT_ENTITY,
        rootIncarnation: EFFECT_ROOT,
        node: 30,
        lifetime: 1,
        value: { kind: "bool", value: false },
        revision: 3,
      },
    ],
    parentOf,
    (error) => void errors.push(error),
  );
  assert.deepEqual(summary, { delivered: 2, skipped: 0 });
  assert.ok(seen.includes("toggle:false@3"), `saw ${seen}`);
  assert.ok(seen.includes("bubble:10"), `saw ${seen}`);
  assert.equal(
    errors.filter((error) => /press failed/.test(error.message)).length,
    1,
  );
  assert.equal(
    errors.filter((error) => /capture failed/.test(error.message)).length,
    2,
  );
});

test("conflicts and unhandled scene input report once, never as effects", () => {
  const { subs, parentOf, seen, errors } = feedHarness();
  const conflicts: unknown[] = [];
  const cancelled: unknown[] = [];
  const unhandled: unknown[] = [];
  const missed: GuiInputCommand = {
    kind: "pointerDown",
    pointer: 1,
    position: [50, 50],
    button: "primary",
  };
  const batch: GuiObservationBatch = {
    effects: [
      {
        kind: "controlCommitted",
        entity: 100n,
        rootIncarnation: EFFECT_ROOT,
        node: 30,
        lifetime: 1,
        value: { kind: "bool", value: true },
        revision: 2,
        path: [10, 30],
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
    conflicts: [
      {
        session: 7n,
        sourceTick: 11n,
        effectTick: 12n,
        target: {
          entity: 100n,
          rootIncarnation: EFFECT_ROOT,
          node: 30,
          lifetime: 1,
        },
        reason: { kind: "revisionMismatch", expected: 1, found: 2 },
      },
    ],
    cancellations: [
      {
        session: 7n,
        sourceTick: 11n,
        effectTick: 12n,
        target: {
          entity: 100n,
          rootIncarnation: EFFECT_ROOT,
          node: 30,
          lifetime: 1,
        },
        reason: "gestureCancelled",
      },
    ],
    unhandled: [
      {
        session: 7n,
        tick: 11n,
        input: missed,
        reason: { kind: "noPanelHit" },
      },
    ],
  };
  const summary = subs.feedObservations(batch, parentOf, {
    onConflict: (conflict) => void conflicts.push(conflict),
    onCancelled: (cancellation) => void cancelled.push(cancellation),
    onUnhandled: (observation) => void unhandled.push(observation),
    onError: (error) => void errors.push(error),
  });
  assert.deepEqual(summary, {
    delivered: 1,
    skipped: 0,
    conflicts: 1,
    cancelled: 1,
    unhandled: 1,
  });
  assert.equal(errors.length, 0);
  // The toggle delivered once through control callbacks.
  assert.deepEqual(
    seen.filter((entry) => entry.startsWith("toggle:")),
    ["toggle:true@2"],
  );
  // Each non-effect record arrived exactly once with its identity.
  assert.equal(conflicts.length, 1);
  assert.equal(cancelled.length, 1);
  assert.equal(unhandled.length, 1);
  assert.deepEqual((unhandled[0] as { input: GuiInputCommand }).input, missed);
  // A throwing scene listener is isolated without losing the counts.
  const throwing = subs.feedObservations(batch, parentOf, {
    onUnhandled: () => {
      throw new Error("scene blew up");
    },
    onError: (error) => void errors.push(error),
  });
  assert.deepEqual(throwing.unhandled, 1);
  assert.equal(errors.length, 1);
  assert.match(errors[0]!.message, /scene blew up/);
});

test("free dispatch routes observations without a registry", () => {
  const seen: string[] = [];
  const resolution: GuiCallbackResolution = {
    parentOf: () =>
      new Map([
        [20, 10],
        [10, undefined],
      ]),
    listeners: (_entity, _rootIncarnation, node) =>
      new Map<number, GuiControlListenerRecord>([
        [
          10,
          {
            lifetime: 1,
            kind: "container",
            onActionCapture: () => void seen.push("capture:10"),
          },
        ],
        [
          20,
          {
            lifetime: 1,
            kind: "button",
            onPress: () => void seen.push("press:20"),
          },
        ],
      ]).get(node),
  };
  const summary = dispatchGuiObservations(
    {
      effects: [
        {
          kind: "buttonPressed",
          entity: 100n,
          rootIncarnation: EFFECT_ROOT,
          node: 20,
          lifetime: 1,
          path: [10, 20],
          sourceTick: 11n,
          effectTick: 12n,
        },
      ],
      unhandled: [
        {
          session: 7n,
          tick: 11n,
          input: { kind: "blur" },
          reason: { kind: "noFocus" },
        },
      ],
    },
    resolution,
  );
  assert.deepEqual(summary, {
    delivered: 1,
    skipped: 0,
    conflicts: 0,
    cancelled: 0,
    unhandled: 1,
  });
  assert.deepEqual(seen, ["press:20", "capture:10"]);
});

/** GuiCommits acknowledgement wiring over real React controls.
 *
 * A minimal producer runtime applies GUI edits and answers inspections; the
 * observation batches below mirror core report fields (sessions, ticks,
 * pinned paths, lifetimes, revisions). The byte-exact wire proof lives in
 * the protocol client suite.
 */

function commitsRejected(): Error {
  return Object.assign(new Error("rejected"), {
    code: "IPP_REQUEST_REJECTED",
  });
}

interface CommitsStoredNode {
  id: number;
  parent: number | undefined;
  content: unknown;
  style: GuiNodeStyle;
  lifetime: number;
}

class CommitsProducer {
  readonly entity = 100n;
  rootIncarnation = 1n;
  producerLive = false;
  readonly nodes = new Map<number, CommitsStoredNode>();
  observationListener: ((batch: GuiObservationBatch) => void) | null = null;
  observationDetaches = 0;

  async editGui(edit: GuiEdit): Promise<void> {
    if (edit.action === "insert") {
      if (this.nodes.has(edit.id)) throw commitsRejected();
      this.nodes.set(edit.id, {
        id: edit.id,
        parent: edit.parent,
        content: edit.content,
        style: { ...(edit.style ?? {}) },
        lifetime: 1,
      });
      return;
    }
    if (edit.action === "remove") {
      const doomed = [edit.handle.nodeId];
      for (let index = 0; index < doomed.length; index += 1) {
        const current = doomed[index]!;
        for (const node of this.nodes.values())
          if (node.parent === current) doomed.push(node.id);
      }
      for (const gone of doomed) this.nodes.delete(gone);
      return;
    }
    const node = this.nodes.get(edit.handle.nodeId);
    if (!node) throw commitsRejected();
    if (edit.action === "update") {
      if (edit.patch.content !== undefined) node.content = edit.patch.content;
      if (edit.patch.style !== undefined)
        node.style = {
          ...node.style,
          ...edit.patch.style,
        } as GuiNodeStyle;
      return;
    }
    if (edit.action === "move" && edit.parent !== undefined)
      node.parent = edit.parent;
  }

  async editGuiBatch(
    edits: readonly GuiEdit[],
  ): Promise<import("@ipp/client").GuiEditBatchOutcome> {
    let applied = 0;
    for (const edit of edits) {
      try {
        await this.editGui(edit);
        applied += 1;
      } catch {
        return {
          ok: false,
          applied,
          requests: 1,
          error: { kind: "runtime", reason: "rejected" },
        };
      }
    }
    return { ok: true, applied, requests: 1 };
  }

  async inspectGui(query: { entity: bigint }): Promise<GuiInspectResponse> {
    assert.equal(query.entity, this.entity);
    if (!this.producerLive) throw commitsRejected();
    const nodes = [...this.nodes.values()]
      .sort((a, b) => a.id - b.id)
      .map(
        (node) =>
          ({
            id: node.id,
            parent: node.parent,
            children: [...this.nodes.values()]
              .filter((child) => child.parent === node.id)
              .map((child) => child.id),
            content: node.content,
            style: { ...node.style },
            controlValue: { kind: "none" },
            controlRevision: 0,
            lifetime: node.lifetime,
          }) as unknown as GuiInspectedNode,
      );
    return {
      rootEntity: this.entity,
      rootIncarnation: this.rootIncarnation,
      nodes,
    } as unknown as GuiInspectResponse;
  }

  async batch(commands: Command[]): Promise<BatchOutcome> {
    for (const command of commands) {
      if (command.kind === "insertComponent") this.producerLive = true;
      else if (command.kind === "removeComponent") this.producerLive = false;
    }
    return { ok: true } as unknown as BatchOutcome;
  }

  client(): ReactWorldClient {
    return {
      session: 7n,
      schemaHash: "test",
      capabilities: [],
      components: { GuiRoot: { id: 26 } },
      batch: (commands: Command[]) => this.batch(commands),
      onDiagnostic: () => () => {},
      editGui: (edit: GuiEdit) => this.editGui(edit),
      editGuiBatch: (edits: readonly GuiEdit[]) => this.editGuiBatch(edits),
      inspectGui: (query: { entity: bigint }) => this.inspectGui(query),
      createGuiNodeHandle: (
        entity: bigint,
        rootIncarnation: bigint,
        nodeId: number,
        nodeLifetime: number,
      ): GuiNodeHandle =>
        ({
          session: 7n,
          entity,
          rootIncarnation,
          nodeId,
          nodeLifetime,
        }) as GuiNodeHandle,
      subscribeGuiObservations: (
        listener: (batch: GuiObservationBatch) => void,
      ) => {
        this.observationListener = listener;
        return () => {
          this.observationDetaches += 1;
          if (this.observationListener === listener)
            this.observationListener = null;
        };
      },
    } as unknown as ReactWorldClient;
  }
}

interface WiredControls {
  commits: GuiCommits;
  producer: CommitsProducer;
  presses: unknown[];
  toggles: unknown[];
  actions: string[];
  conflicts: unknown[];
  cancelled: unknown[];
  unhandled: unknown[];
  errors: Error[];
  columnId: number;
  buttonId: number;
  checkboxId: number;
  emit(batch: GuiObservationBatch): GuiObservationSummary;
  removeButton(): Promise<void>;
  captureButton(callback: () => void): void;
  replayOlderSnapshot(): Promise<void>;
  captureRemovedRoot(): void;
  buttonRef: { current: GuiNodeHandle | null };
}

/** Mount a real column holding a real button and checkbox, then acknowledge
 * each node one at a time so runtime identities stay deterministic. */
async function wiredControls(): Promise<WiredControls> {
  const producer = new CommitsProducer();
  const presses: unknown[] = [];
  const toggles: unknown[] = [];
  const actions: string[] = [];
  const conflicts: unknown[] = [];
  const cancelled: unknown[] = [];
  const unhandled: unknown[] = [];
  const errors: Error[] = [];
  const commits = new TestGuiCommits(producer.client(), {
    checkSession: () => {},
    report: (error: unknown) =>
      error instanceof Error ? error : new Error(String(error)),
  });
  const columnRef: { current: GuiNodeHandle | null } = { current: null };
  const buttonRef: { current: GuiNodeHandle | null } = { current: null };
  const checkRef: { current: GuiNodeHandle | null } = { current: null };
  const tree = new ReactWorldTree(stubClient());
  const entity = tree.instance(ENTITY_HOST_TYPE, { id: "panel" });
  const root = tree.instance(GUI_ROOT_HOST_TYPE, {});
  const column = tree.instance(GUI_COLUMN_HOST_TYPE, {
    nodeRef: columnRef,
    onActionCapture: () => void actions.push("capture:column"),
    onAction: () => void actions.push("bubble:column"),
  });
  const button = Button({
    label: "Go",
    nodeRef: buttonRef,
    onPress: (event) => void presses.push(event),
    onAction: () => void actions.push("bubble:button"),
  });
  const checkbox = Checkbox({
    checked: false,
    nodeRef: checkRef,
    onToggle: (event) => void toggles.push(event),
  });
  const buttonNode = tree.instance(
    GUI_BUTTON_HOST_TYPE,
    button.props as unknown as ReactWorldElementProps,
  );
  const checkboxNode = tree.instance(
    GUI_CHECKBOX_HOST_TYPE,
    checkbox.props as unknown as ReactWorldElementProps,
  );
  entity.children.push(root);
  root.children.push(column);
  column.children.push(buttonNode, checkboxNode);
  tree.children.push(entity);
  const described = tree.describe();
  assert.equal(described.gui.length, 1);
  const decl = described.gui[0]!;
  const resolveEntity = () => producer.entity;
  const snapshot = (count: number) => ({
    identity: decl.identity,
    entity: decl.entity,
    nodeRef: decl.nodeRef,
    nodes: decl.nodes.slice(0, count),
    signature: `${count}`,
  });
  await commits.apply([snapshot(1)], resolveEntity);
  await commits.apply([snapshot(2)], resolveEntity);
  await commits.apply([snapshot(3)], resolveEntity);
  const columnId = columnRef.current?.nodeId;
  const buttonId = buttonRef.current?.nodeId;
  const checkboxId = checkRef.current?.nodeId;
  assert.equal(columnId, 1);
  assert.equal(buttonId, 2);
  assert.equal(checkboxId, 3);
  const removeButton = async (): Promise<void> => {
    buttonNode.hidden = true;
    try {
      await commits.apply(tree.describe().gui, resolveEntity);
    } finally {
      buttonNode.hidden = false;
    }
  };
  return {
    commits,
    producer,
    presses,
    toggles,
    actions,
    conflicts,
    cancelled,
    unhandled,
    errors,
    columnId: columnId!,
    buttonId: buttonId!,
    checkboxId: checkboxId!,
    emit: (batch: GuiObservationBatch) =>
      commits.feedObservations(batch, {
        onConflict: (observation) => void conflicts.push(observation),
        onCancelled: (observation) => void cancelled.push(observation),
        onUnhandled: (observation) => void unhandled.push(observation),
        onError: (error) => void errors.push(error),
      }),
    removeButton,
    captureButton: (callback) => {
      buttonNode.props = { ...buttonNode.props, onPress: callback };
      commits.reconcileLocal(tree.describe().gui);
    },
    replayOlderSnapshot: () =>
      GuiCommits.prototype.apply.call(commits, [decl], resolveEntity),
    captureRemovedRoot: () => commits.reconcileLocal([]),
    buttonRef,
  };
}

function pressBatch(
  wired: WiredControls,
  node: number,
  path: number[],
  lifetime = 1,
): GuiObservationBatch {
  return {
    effects: [
      {
        kind: "buttonPressed",
        entity: 100n,
        rootIncarnation: wired.producer.rootIncarnation,
        node,
        lifetime,
        path,
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
  };
}

test("GuiCommits dispatches committed observations into real control callbacks", async () => {
  const wired = await wiredControls();
  const summary = wired.emit({
    effects: [
      {
        kind: "buttonPressed",
        entity: 100n,
        rootIncarnation: wired.producer.rootIncarnation,
        node: wired.buttonId,
        lifetime: 1,
        path: [wired.columnId, wired.buttonId],
        sourceTick: 11n,
        effectTick: 12n,
      },
      {
        kind: "controlCommitted",
        entity: 100n,
        rootIncarnation: wired.producer.rootIncarnation,
        node: wired.checkboxId,
        lifetime: 1,
        value: { kind: "bool", value: true },
        revision: 2,
        path: [wired.columnId, wired.checkboxId],
        sourceTick: 11n,
        effectTick: 12n,
      },
    ],
  });
  assert.deepEqual(summary, {
    delivered: 2,
    skipped: 0,
    conflicts: 0,
    cancelled: 0,
    unhandled: 0,
  });
  assert.equal(wired.presses.length, 1);
  assert.deepEqual(wired.presses[0], {
    entity: 100n,
    rootIncarnation: wired.producer.rootIncarnation,
    node: wired.buttonId,
    lifetime: 1,
    name: "Go",
    sourceTick: 11n,
    effectTick: 12n,
  });
  assert.deepEqual(wired.toggles[0], {
    entity: 100n,
    rootIncarnation: wired.producer.rootIncarnation,
    node: wired.checkboxId,
    lifetime: 1,
    revision: 2,
    value: true,
    sourceTick: 11n,
    effectTick: 12n,
  });
  assert.deepEqual(wired.actions, [
    "capture:column",
    "bubble:button",
    "bubble:column",
    "capture:column",
    "bubble:column",
  ]);
  assert.deepEqual(wired.errors, []);
});

test("GuiCommits keeps delayed callbacks until diff removals tear them down", async () => {
  const wired = await wiredControls();
  // A delayed press still dispatches while the node stays acknowledged.
  assert.deepEqual(
    wired.emit(
      pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
    ),
    { delivered: 1, skipped: 0, conflicts: 0, cancelled: 0, unhandled: 0 },
  );
  assert.equal(wired.presses.length, 1);
  // A stale lifetime reports without dispatching while subscribed.
  assert.deepEqual(
    wired.emit(
      pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId], 9),
    ),
    { delivered: 0, skipped: 1, conflicts: 0, cancelled: 0, unhandled: 0 },
  );
  assert.equal(wired.errors.length, 1);
  assert.match(wired.errors[0]!.message, /Stale GUI control effect/);
});

test("older acknowledged GUI work cannot restore superseded callbacks or refs", async () => {
  const wired = await wiredControls();
  let latestPresses = 0;
  wired.captureButton(() => {
    latestPresses += 1;
  });
  await wired.replayOlderSnapshot();
  wired.emit(
    pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
  );
  assert.equal(latestPresses, 1);
  assert.equal(wired.presses.length, 0);

  wired.captureRemovedRoot();
  assert.equal(wired.buttonRef.current, null);
  await wired.replayOlderSnapshot();
  wired.emit(
    pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
  );
  assert.equal(latestPresses, 1);
  assert.equal(wired.buttonRef.current, null);
});

test("GuiCommits tears subscriptions down on diff removals", async () => {
  const wired = await wiredControls();
  assert.deepEqual(
    wired.emit(
      pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
    ),
    { delivered: 1, skipped: 0, conflicts: 0, cancelled: 0, unhandled: 0 },
  );
  await wired.removeButton();
  assert.deepEqual(
    wired.emit(
      pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
    ),
    { delivered: 0, skipped: 1, conflicts: 0, cancelled: 0, unhandled: 0 },
  );
  assert.equal(wired.presses.length, 1);
  assert.deepEqual(wired.errors, []);
});

test("GuiCommits reports conflicts and cancellations once, never as effects", async () => {
  const wired = await wiredControls();
  const summary = wired.emit({
    effects: [],
    conflicts: [
      {
        session: 7n,
        sourceTick: 11n,
        effectTick: 12n,
        target: {
          entity: 100n,
          rootIncarnation: wired.producer.rootIncarnation,
          node: wired.checkboxId,
          lifetime: 1,
        },
        reason: { kind: "revisionMismatch", expected: 1, found: 2 },
      },
    ],
    cancellations: [
      {
        session: 7n,
        sourceTick: 11n,
        effectTick: 12n,
        reason: "gestureCancelled",
      },
    ],
  });
  assert.deepEqual(summary, {
    delivered: 0,
    skipped: 0,
    conflicts: 1,
    cancelled: 1,
    unhandled: 0,
  });
  assert.equal(wired.conflicts.length, 1);
  assert.equal(wired.cancelled.length, 1);
  assert.deepEqual(wired.presses, []);
  assert.deepEqual(wired.toggles, []);
});

test("GuiCommits routes unhandled scene input to the fallback without duplicates", async () => {
  const wired = await wiredControls();
  const missed: GuiInputCommand = {
    kind: "pointerDown",
    pointer: 1,
    position: [50, 50],
    button: "primary",
  };
  const summary = wired.emit({
    effects: [],
    unhandled: [
      { session: 7n, tick: 11n, input: missed, reason: { kind: "noPanelHit" } },
    ],
  });
  assert.deepEqual(summary, {
    delivered: 0,
    skipped: 0,
    conflicts: 0,
    cancelled: 0,
    unhandled: 1,
  });
  assert.equal(wired.unhandled.length, 1);
  assert.deepEqual(
    (wired.unhandled[0] as { input: GuiInputCommand }).input,
    missed,
  );
  assert.deepEqual(wired.presses, []);
  // The same pointer completing on the button dispatches once, not twice.
  assert.deepEqual(
    wired.emit(
      pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
    ),
    { delivered: 1, skipped: 0, conflicts: 0, cancelled: 0, unhandled: 0 },
  );
  assert.equal(wired.presses.length, 1);
  assert.equal(wired.unhandled.length, 1);
});

test("GuiCommits clears subscriptions on reset", async () => {
  const wired = await wiredControls();
  wired.commits.reset();
  assert.deepEqual(
    wired.emit(
      pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
    ),
    { delivered: 0, skipped: 1, conflicts: 0, cancelled: 0, unhandled: 0 },
  );
  assert.deepEqual(wired.presses, []);
});

test("GuiCommits bridges live client batches on acknowledgement", async () => {
  const wired = await wiredControls();
  assert.ok(wired.producer.observationListener !== null);
  wired.producer.observationListener!(
    pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
  );
  assert.equal(wired.presses.length, 1);
  await wired.commits.dispose();
  assert.equal(wired.producer.observationDetaches, 1);
  assert.deepEqual(
    wired.emit(
      pressBatch(wired, wired.buttonId, [wired.columnId, wired.buttonId]),
    ),
    { delivered: 0, skipped: 1, conflicts: 0, cancelled: 0, unhandled: 0 },
  );
});
