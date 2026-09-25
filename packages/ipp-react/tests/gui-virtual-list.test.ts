/** VirtualList declarations: range-keyed item description, index placement
 * in the GUI diff, and wanted-range delivery through the subscription
 * registry. Real runtime scrolling and frames live in the browser GUI suite. */
import assert from "node:assert/strict";
import test from "node:test";
import type {
  GuiEdit,
  GuiNodeHandle,
  GuiVirtualRangeChangedEffect,
} from "@ipp/client";
import { ENTITY_HOST_TYPE } from "../src/components.js";
import type { ReactWorldClient } from "../src/contract.js";
import {
  GUI_COLUMN_HOST_TYPE,
  GUI_ROOT_HOST_TYPE,
  GUI_TEXT_HOST_TYPE,
  GUI_VIRTUAL_ITEM_HOST_TYPE,
  GUI_VIRTUAL_LIST_HOST_TYPE,
  virtualListNode,
  type GuiVirtualRange,
} from "../src/gui/components.js";
import { GuiEffectSubscriptions } from "../src/gui/callbacks.js";
import type { GuiDescribedNode } from "../src/gui/description.js";
import { diffGuiTree, type GuiAcknowledgedNode } from "../src/gui/diff.js";
import {
  ReactWorldTree,
  retainedNodeCallbacks,
  type ReactWorldElementProps,
} from "../src/tree.js";

function stubClient(): ReactWorldClient {
  return {
    components: { GuiRoot: {} },
    session: 7n,
  } as unknown as ReactWorldClient;
}

const LIST_PROPS = {
  itemCount: 100_000,
  itemExtent: 0.5,
  overscan: 2,
  width: 4,
  height: 3,
};

/** Describe a list declaring `indices`, each item holding one text leaf. */
function describeList(
  indices: readonly number[],
  onRangeChange?: (range: GuiVirtualRange) => void,
) {
  const tree = new ReactWorldTree(stubClient());
  const entity = tree.instance(ENTITY_HOST_TYPE, { id: "panel" });
  const root = tree.instance(GUI_ROOT_HOST_TYPE, {});
  const list = tree.instance(GUI_VIRTUAL_LIST_HOST_TYPE, {
    ...LIST_PROPS,
    onRangeChange,
  } as unknown as ReactWorldElementProps);
  for (const index of indices) {
    const item = tree.instance(GUI_VIRTUAL_ITEM_HOST_TYPE, {
      itemIndex: index,
    } as unknown as ReactWorldElementProps);
    item.children.push(
      tree.instance(GUI_TEXT_HOST_TYPE, {
        text: `item ${index}`,
      } as unknown as ReactWorldElementProps),
    );
    list.children.push(item);
  }
  entity.children.push(root);
  root.children.push(list);
  tree.children.push(entity);
  return { tree, list, root, nodes: tree.describe().gui[0]!.nodes };
}

test("a VirtualList describes its items and item wrappers by index", () => {
  const ranges: GuiVirtualRange[] = [];
  const { nodes } = describeList([40, 41, 42], (range) => ranges.push(range));
  const [list, ...rest] = nodes;
  assert.deepEqual(list!.data, {
    kind: "container",
    containerKind: "virtualList",
  });
  assert.deepEqual(list!.values, {
    itemCount: 100_000,
    itemExtent: 0.5,
    overscan: 2,
    axis: 1,
    anchorIndex: 0,
    anchorOffset: 0,
  });
  const items = rest.filter((node) => node.parent === list!.identity);
  assert.deepEqual(
    items.map((node) => [node.itemIndex, node.data]),
    [40, 41, 42].map((index) => [
      index,
      { kind: "container", containerKind: "stack" },
    ]),
  );
  retainedNodeCallbacks(list!).onRangeChange?.({ first: 1, last: 2 });
  assert.deepEqual(ranges, [{ first: 1, last: 2 }]);
});

test("VirtualList declarations validate their items and nesting", () => {
  assert.throws(
    () => virtualListNode({ itemCount: 1.5, itemExtent: 1 }),
    /itemCount/,
  );
  assert.throws(
    () => virtualListNode({ itemCount: 2 ** 24 + 1, itemExtent: 1 }),
    /itemCount/,
  );
  assert.throws(
    () => virtualListNode({ itemCount: 4, itemExtent: 0 }),
    /itemExtent/,
  );
  assert.throws(
    () => virtualListNode({ itemCount: 4, itemExtent: 1, overscan: -1 }),
    /overscan/,
  );
  assert.deepEqual(
    virtualListNode({ itemCount: 4, itemExtent: 1, axis: "horizontal" }).values
      .axis,
    0,
  );

  // Only item wrappers sit directly in a list, and only in a list.
  const { tree, list } = describeList([]);
  list.children.push(
    tree.instance(GUI_COLUMN_HOST_TYPE, {} as ReactWorldElementProps),
  );
  assert.throws(() => tree.describe(), /renderItem/);
  const stray = new ReactWorldTree(stubClient());
  const entity = stray.instance(ENTITY_HOST_TYPE, { id: "panel" });
  const root = stray.instance(GUI_ROOT_HOST_TYPE, {});
  const column = stray.instance(
    GUI_COLUMN_HOST_TYPE,
    {} as ReactWorldElementProps,
  );
  column.children.push(
    stray.instance(GUI_VIRTUAL_ITEM_HOST_TYPE, {
      itemIndex: 0,
    } as unknown as ReactWorldElementProps),
  );
  entity.children.push(root);
  root.children.push(column);
  stray.children.push(entity);
  assert.throws(() => stray.describe(), /renderItem/);
  assert.throws(
    () =>
      stray.instance(GUI_VIRTUAL_ITEM_HOST_TYPE, {
        itemIndex: -1,
      } as unknown as ReactWorldElementProps),
    /item index/,
  );
});

function handle(nodeId: number): GuiNodeHandle {
  return { session: 7n, entity: 100n, rootIncarnation: 1n, nodeId };
}

/** Acknowledged state of `nodes`, node IDs equal to identities. */
function acknowledge(nodes: readonly GuiDescribedNode[]) {
  const acked = new Map<number, GuiAcknowledgedNode>(
    nodes.map((node) => [
      node.identity,
      {
        nodeId: node.identity,
        parent: node.parent,
        parentId: node.parent,
        data: node.data,
        values: node.values,
        style: node.style,
      },
    ]),
  );
  const order = new Map<number | undefined, number[]>();
  for (const node of nodes) {
    const list = order.get(node.parent) ?? [];
    list.push(node.identity);
    order.set(node.parent, list);
  }
  const ids = new Map(nodes.map((node) => [node.identity, node.identity]));
  return { acked, ids, order };
}

/** A list at identity 1 with item wrappers whose identity is 1000 + index. */
function window(first: number, last: number, count = 100_000) {
  const list: GuiDescribedNode = {
    identity: 1,
    parent: undefined,
    type: GUI_VIRTUAL_LIST_HOST_TYPE,
    ...virtualListNode({ itemCount: count, itemExtent: 0.5 }),
    style: {},
    nodeRef: null,
    onAction: undefined,
    onActionCapture: undefined,
  };
  const items: GuiDescribedNode[] = [];
  for (let index = first; index < last; index += 1)
    items.push({
      identity: 1000 + index,
      parent: 1,
      type: GUI_VIRTUAL_ITEM_HOST_TYPE,
      data: { kind: "container", containerKind: "stack" },
      values: {},
      style: {},
      nodeRef: null,
      onAction: undefined,
      onActionCapture: undefined,
      itemIndex: index,
    });
  return [list, ...items];
}

test("a moving range removes and inserts items by index without moves", () => {
  const before = window(40, 52);
  const { acked, ids, order } = acknowledge(before);
  const plan = diffGuiTree(window(44, 56), acked, ids, 5000, order, handle);
  const kinds = plan.edits.map((edit) => edit.action);
  assert.deepEqual(kinds, [
    ...Array(4).fill("remove"),
    ...Array(4).fill("insert"),
  ]);
  assert.deepEqual(
    plan.edits
      .filter(
        (edit): edit is Extract<GuiEdit, { action: "remove" }> =>
          edit.action === "remove",
      )
      .map((edit) => edit.handle.nodeId),
    [1040, 1041, 1042, 1043],
  );
  // Each insert names its item index, the runtime order key, whatever the
  // wrapper's position among the declared children.
  assert.deepEqual(
    plan.edits
      .filter(
        (edit): edit is Extract<GuiEdit, { action: "insert" }> =>
          edit.action === "insert",
      )
      .map((edit) => [edit.id, edit.parent, edit.index]),
    [
      [5000, 1, 52],
      [5001, 1, 53],
      [5002, 1, 54],
      [5003, 1, 55],
    ],
  );

  // Scrolling back prepends items at their indices, again without moves.
  const back = diffGuiTree(window(38, 50), acked, ids, 5000, order, handle);
  assert.deepEqual(
    back.edits
      .filter(
        (edit): edit is Extract<GuiEdit, { action: "insert" }> =>
          edit.action === "insert",
      )
      .map((edit) => edit.index),
    [38, 39],
  );
  assert.ok(back.edits.every((edit) => edit.action !== "move"));

  // A new count updates the list's item properties; the anchor stays
  // runtime-owned and never differs between declarations.
  const recount = diffGuiTree(
    window(40, 52, 200_000),
    acked,
    ids,
    5000,
    order,
    handle,
  );
  assert.deepEqual(recount.edits, [
    {
      action: "update",
      handle: handle(1),
      patch: {
        data: { kind: "container", containerKind: "virtualList" },
        values: window(40, 52, 200_000)[0]!.values,
      },
    },
  ]);
});

function range(
  node: number,
  first: number,
  last: number,
  revision: number,
): GuiVirtualRangeChangedEffect {
  return {
    kind: "virtualRangeChanged",
    entity: 100n,
    node,
    first,
    last,
    revision,
  };
}

test("wanted ranges wait for their list, then deliver in revision order", () => {
  const subscriptions = new GuiEffectSubscriptions();
  const seen: GuiVirtualRange[] = [];
  const errors: Error[] = [];

  // A range that arrives before its list is acknowledged waits for it.
  assert.equal(subscriptions.feedRanges([range(2, 0, 14, 1)]), 0);
  subscriptions.subscribeRange(100n, 2, (next) => void seen.push(next));
  assert.deepEqual(seen, [{ first: 0, last: 14 }]);

  // Later revisions deliver; stale ones and other lists do not.
  assert.equal(
    subscriptions.feedRanges([
      range(2, 10, 24, 3),
      range(2, 4, 18, 2),
      range(9, 0, 1, 1),
    ]),
    1,
  );
  assert.deepEqual(seen.at(-1), { first: 10, last: 24 });
  assert.equal(seen.length, 2);

  // Replacing the observer does not replay; a throwing one is isolated.
  subscriptions.subscribeRange(
    100n,
    2,
    () => {
      throw new Error("observer failed");
    },
    (error) => void errors.push(error),
  );
  subscriptions.feedRanges([range(2, 11, 25, 4)], (error) =>
    errors.push(error),
  );
  assert.deepEqual(
    errors.map((error) => error.message),
    ["observer failed"],
  );

  // Removing the list drops its observer and its latest range.
  subscriptions.unsubscribe(100n, 1n, 2);
  subscriptions.subscribeRange(100n, 2, (next) => void seen.push(next));
  assert.equal(seen.length, 2);
});
