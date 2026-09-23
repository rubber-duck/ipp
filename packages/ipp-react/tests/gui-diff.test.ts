/** Placement edits of the pure GUI diff against an independent model of the
 * runtime's child-list semantics. */
import assert from "node:assert/strict";
import test from "node:test";
import type { GuiEdit, GuiNodeContent, GuiNodeHandle } from "@ipp/client";
import {
  GUI_COLUMN_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_HOST_TYPE,
} from "../src/gui/components.js";
import {
  normalizeGuiStyle,
  type GuiDescribedNode,
} from "../src/gui/description.js";
import { diffGuiTree, type GuiAcknowledgedNode } from "../src/gui/diff.js";

const NEXT_ID = 100;

function handle(nodeId: number, lifetime: number): GuiNodeHandle {
  return {
    session: 1n,
    entity: 100n,
    rootIncarnation: 1n,
    nodeId,
    nodeLifetime: lifetime,
  };
}

function column(identity: number, parent?: number): GuiDescribedNode {
  return node(identity, parent, {
    kind: "container",
    containerKind: "column",
  });
}

function text(identity: number, parent: number, value = `line ${identity}`) {
  return node(identity, parent, { kind: "text", text: value });
}

function node(
  identity: number,
  parent: number | undefined,
  content: GuiNodeContent,
): GuiDescribedNode {
  return {
    identity,
    parent,
    type:
      content.kind === "container"
        ? GUI_COLUMN_HOST_TYPE
        : content.kind === "slider"
          ? GUI_SLIDER_HOST_TYPE
          : GUI_TEXT_HOST_TYPE,
    content,
    style: normalizeGuiStyle({}),
    nodeRef: null,
    onAction: undefined,
    onActionCapture: undefined,
  };
}

/** Acknowledged state of `nodes`, with node IDs equal to identities. */
function acknowledge(nodes: readonly GuiDescribedNode[], withOrder = true) {
  const acked = new Map<number, GuiAcknowledgedNode>(
    nodes.map((entry) => [
      entry.identity,
      {
        nodeId: entry.identity,
        parent: entry.parent,
        parentId: entry.parent,
        lifetime: 1,
        content: entry.content,
        style: entry.style,
      },
    ]),
  );
  const ids = new Map(nodes.map((entry) => [entry.identity, entry.identity]));
  const order = new Map<number | undefined, number[]>();
  if (withOrder)
    for (const entry of nodes) {
      const list = order.get(entry.parent);
      if (list) list.push(entry.identity);
      else order.set(entry.parent, [entry.identity]);
    }
  return { acked, ids, order };
}

function diff(
  before: readonly GuiDescribedNode[],
  after: readonly GuiDescribedNode[],
  withOrder = true,
) {
  const { acked, ids, order } = acknowledge(before, withOrder);
  return diffGuiTree(after, acked, ids, NEXT_ID, order, handle);
}

/**
 * Child lists as the runtime applies edits: removal cascades, and insert and
 * move clamp their index after detaching a moved node from its parent.
 */
class RuntimeModel {
  private readonly children = new Map<number | undefined, number[]>();
  private readonly parents = new Map<number, number | undefined>();

  constructor(nodes: readonly GuiDescribedNode[]) {
    for (const entry of nodes) this.attach(entry.identity, entry.parent);
  }

  apply(edits: readonly GuiEdit[], ids: ReadonlyMap<number, number>): void {
    const identity = new Map([...ids].map(([key, id]) => [id, key]));
    const resolve = (id: number | undefined) =>
      id === undefined ? undefined : identity.get(id)!;
    for (const edit of edits) {
      switch (edit.action) {
        case "remove":
          this.remove(resolve(edit.handle.nodeId)!);
          break;
        case "insert":
          this.attach(resolve(edit.id)!, resolve(edit.parent), edit.index);
          break;
        case "move": {
          const moved = resolve(edit.handle.nodeId)!;
          const from = this.list(this.parents.get(moved));
          from.splice(from.indexOf(moved), 1);
          this.attach(moved, resolve(edit.parent), edit.index);
          break;
        }
        default:
          break;
      }
    }
  }

  /** Every parent's child list, with empty lists omitted. */
  lists(): Map<number | undefined, number[]> {
    return new Map([...this.children].filter(([, list]) => list.length > 0));
  }

  private list(parent: number | undefined): number[] {
    let list = this.children.get(parent);
    if (!list) this.children.set(parent, (list = []));
    return list;
  }

  private attach(
    identity: number,
    parent: number | undefined,
    index = Number.MAX_SAFE_INTEGER,
  ): void {
    const list = this.list(parent);
    list.splice(Math.min(index, list.length), 0, identity);
    this.parents.set(identity, parent);
  }

  private remove(identity: number): void {
    const list = this.list(this.parents.get(identity));
    list.splice(list.indexOf(identity), 1);
    for (const child of this.children.get(identity) ?? []) this.remove(child);
    this.children.delete(identity);
    this.parents.delete(identity);
  }
}

function expectedLists(nodes: readonly GuiDescribedNode[]) {
  const lists = new Map<number | undefined, number[]>();
  for (const entry of nodes) {
    const list = lists.get(entry.parent);
    if (list) list.push(entry.identity);
    else lists.set(entry.parent, [entry.identity]);
  }
  return lists;
}

function sortedLists(lists: Map<number | undefined, number[]>) {
  return [...lists].sort(([a], [b]) => (a ?? -1) - (b ?? -1));
}

/** Apply the diff to the model and require the desired arrangement. */
function converge(
  before: readonly GuiDescribedNode[],
  after: readonly GuiDescribedNode[],
  withOrder = true,
) {
  const plan = diff(before, after, withOrder);
  const model = new RuntimeModel(before);
  model.apply(plan.edits, plan.ids);
  assert.deepEqual(
    sortedLists(model.lists()),
    sortedLists(expectedLists(after)),
  );
  return plan;
}

const log = (lines: readonly number[]) => [
  column(1),
  ...lines.map((identity) => text(identity, 1)),
];

const insert = (id: number, index: number, identity: number) => ({
  action: "insert",
  entity: 0n,
  rootIncarnation: 0n,
  id,
  parent: 1,
  index,
  content: { kind: "text", text: `line ${identity}` },
  style: normalizeGuiStyle({}),
});

test("an unchanged list emits no edits", () => {
  assert.deepEqual(converge(log([2, 3, 4]), log([2, 3, 4])).edits, []);
});

test("inserts at the head, middle and tail emit one insert and no moves", () => {
  assert.deepEqual(converge(log([2, 3, 4]), log([5, 2, 3, 4])).edits, [
    insert(NEXT_ID, 0, 5),
  ]);
  assert.deepEqual(converge(log([2, 3, 4]), log([2, 5, 3, 4])).edits, [
    insert(NEXT_ID, 1, 5),
  ]);
  assert.deepEqual(converge(log([2, 3, 4]), log([2, 3, 4, 5])).edits, [
    insert(NEXT_ID, 3, 5),
  ]);
});

test("a removal emits one remove and no moves", () => {
  assert.deepEqual(converge(log([2, 3, 4]), log([2, 4])).edits, [
    { action: "remove", handle: handle(3, 1) },
  ]);
});

test("a prepended log line with the oldest dropped emits no moves", () => {
  assert.deepEqual(converge(log([2, 3, 4, 5]), log([6, 2, 3, 4])).edits, [
    { action: "remove", handle: handle(5, 1) },
    insert(NEXT_ID, 0, 6),
  ]);
});

test("a reorder moves only the children leaving their relative order", () => {
  assert.deepEqual(converge(log([2, 3, 4, 5]), log([5, 2, 3, 4])).edits, [
    { action: "move", handle: handle(5, 1), parent: 1, index: 0 },
  ]);
  assert.deepEqual(converge(log([2, 3, 4, 5]), log([3, 2, 5, 4])).edits, [
    { action: "move", handle: handle(2, 1), parent: 1, index: 1 },
    { action: "move", handle: handle(4, 1), parent: 1, index: 3 },
  ]);
});

test("a slider drag frame emits one value edit and no moves", () => {
  // The slider value itself is a committed control value, not structure; the
  // frame's only structural change is the readout it drives.
  const panel = (value: number) => [
    column(1),
    text(2, 1, `GAIN ${Math.round(value * 100)} PERCENT`),
    node(3, 1, { kind: "slider", value, min: 0, max: 1, step: 0 }),
    text(4, 1, "footer"),
  ];
  assert.deepEqual(converge(panel(0.25), panel(0.5)).edits, [
    {
      action: "update",
      handle: handle(2, 1),
      patch: { content: { kind: "text", text: "GAIN 50 PERCENT" } },
    },
  ]);
});

test("a list of unknown acknowledged order converges", () => {
  const plan = converge(log([2, 3, 4]), log([4, 5, 2]), false);
  assert.equal(plan.edits.filter(({ action }) => action === "move").length, 2);
});

/** Deterministic xorshift generator so a failure reproduces from its seed. */
function random(seed: number) {
  let state = seed >>> 0 || 1;
  return (limit: number) => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) % limit;
  };
}

function shuffle<T>(values: T[], next: (limit: number) => number): T[] {
  for (let index = values.length - 1; index > 0; index -= 1) {
    const other = next(index + 1);
    [values[index], values[other]] = [values[other]!, values[index]!];
  }
  return values;
}

test("random permutations, inserts, removals and reparents converge", () => {
  for (let seed = 1; seed <= 500; seed += 1) {
    const next = random(seed);
    // Root column 1 with child columns 2 and 3; leaves start at 10.
    const containers = [1, 2, 3];
    const leaves = new Map<number, number>();
    let identity = 10;
    for (const parent of containers)
      for (let count = next(7); count > 0; count -= 1)
        leaves.set(identity++, parent);
    const arrange = (placement: Map<number, number>, order: number[]) => [
      column(1),
      column(2, 1),
      column(3, 1),
      ...order.map((leaf) => text(leaf, placement.get(leaf)!)),
    ];
    const before = arrange(leaves, [...leaves.keys()]);

    const kept = new Map([...leaves].filter(() => next(4) !== 0));
    const shuffled = next(3) !== 0;
    for (const leaf of kept.keys())
      if (next(8) === 0) kept.set(leaf, containers[next(3)]!);
    for (let count = next(4); count > 0; count -= 1)
      kept.set(identity++, containers[next(3)]!);
    const order = [...kept.keys()];
    if (shuffled) shuffle(order, next);
    const after = arrange(kept, order);
    const withOrder = next(5) !== 0;
    const plan = converge(before, after, withOrder);

    const moves = plan.edits.filter(({ action }) => action === "move").length;
    const persisting = [...kept.keys()].filter((leaf) => leaves.has(leaf));
    assert.ok(
      moves <= persisting.length + (withOrder ? 0 : containers.length),
      `seed ${seed}: at most one move per persisting node`,
    );
    const reparented = persisting.some(
      (leaf) => kept.get(leaf) !== leaves.get(leaf),
    );
    if (withOrder && !shuffled && !reparented)
      assert.equal(moves, 0, `seed ${seed}: order kept without moves`);
  }
});
