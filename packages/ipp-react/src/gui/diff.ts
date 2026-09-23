/** Pure diff of desired GUI declarations against acknowledged state.
 *
 * Render-time safe: no transport. The commit phase submits the returned edits
 * in order through the generated GUI client.
 */
import type {
  GuiEdit,
  GuiNodeContent,
  GuiNodeHandle,
  GuiNodePatchStyle,
  GuiNodeStyle,
} from "@ipp/client";
import {
  equalGuiAsset,
  equalGuiContent,
  normalizeGuiStyle,
  type GuiDescribedNode,
} from "./description.js";
import type { GuiDeclarationStyle } from "./components.js";

/** Style patch lanes including the authoring-only enabled lane, which
 * rides the runtime object until the shared contract regenerates it. */
export type GuiDeclarationPatchStyle = GuiNodePatchStyle & {
  enabled?: boolean | undefined;
};

/** Acknowledged runtime state for one declared node. */
export interface GuiAcknowledgedNode {
  readonly nodeId: number;
  /** Parent instance identity, or undefined for the root node. */
  readonly parent: number | undefined;
  readonly parentId: number | undefined;
  readonly lifetime: number;
  readonly content: GuiNodeContent;
  readonly style: GuiDeclarationStyle;
}

export interface GuiDiffResult {
  /** Ordered edits: removes, then inserts, then moves, then updates. */
  readonly edits: readonly GuiEdit[];
  /** Identity to node ID mapping including fresh allocations. */
  readonly ids: ReadonlyMap<number, number>;
  readonly nextId: number;
}

function tuplesEqual(
  a: readonly number[] | undefined,
  b: readonly number[] | undefined,
): boolean {
  if (a === undefined || b === undefined) return a === b;
  return (
    a.length === b.length &&
    a.every((value, index) => Object.is(value, b[index]))
  );
}

function stylePatch(
  desiredInput: GuiNodeStyle | GuiDeclarationStyle,
  ackedInput: GuiNodeStyle | GuiDeclarationStyle,
): GuiDeclarationPatchStyle | undefined {
  const desired = normalizeGuiStyle(desiredInput);
  const acked = normalizeGuiStyle(ackedInput);
  const patch: GuiDeclarationPatchStyle = {};
  let changed = false;
  const set = <K extends keyof GuiDeclarationPatchStyle>(
    key: K,
    value: GuiDeclarationPatchStyle[K],
  ): void => {
    patch[key] = value;
    changed = true;
  };
  const nullable = (
    key:
      | "width"
      | "height"
      | "minWidth"
      | "minHeight"
      | "maxWidth"
      | "maxHeight",
  ): void => {
    const next = desired[key] ?? null;
    const previous = acked[key] ?? null;
    if (!Object.is(next, previous)) set(key, next);
  };
  nullable("width");
  nullable("height");
  nullable("minWidth");
  nullable("minHeight");
  nullable("maxWidth");
  nullable("maxHeight");
  if (!tuplesEqual(desired.padding, acked.padding))
    set("padding", desired.padding === undefined ? null : [...desired.padding]);
  if (!tuplesEqual(desired.margin, acked.margin))
    set("margin", desired.margin === undefined ? null : [...desired.margin]);
  if (!Object.is(desired.flex ?? null, acked.flex ?? null))
    set("flex", desired.flex ?? null);
  if (!Object.is(desired.alignX ?? null, acked.alignX ?? null))
    set("alignX", desired.alignX ?? null);
  if (!Object.is(desired.alignY ?? null, acked.alignY ?? null))
    set("alignY", desired.alignY ?? null);
  if (!tuplesEqual(desired.color, acked.color))
    set("color", [...(desired.color ?? [1, 1, 1, 1])]);
  if (!tuplesEqual(desired.backgroundColor, acked.backgroundColor))
    set(
      "backgroundColor",
      desired.backgroundColor === undefined
        ? null
        : [...desired.backgroundColor],
    );
  if (!Object.is(desired.opacity ?? 1, acked.opacity ?? 1))
    set("opacity", desired.opacity ?? 1);
  if (!Object.is(desired.fontSize ?? 0.1, acked.fontSize ?? 0.1))
    set("fontSize", desired.fontSize ?? 0.1);
  if (!equalGuiAsset(desired.asset, acked.asset))
    set("asset", desired.asset ?? null);
  if (!Object.is(desired.enabled ?? true, acked.enabled ?? true))
    set("enabled", desired.enabled ?? true);
  return changed ? patch : undefined;
}

/**
 * Positions (into `values`) of one longest strictly increasing subsequence,
 * found in O(n log n) with patience sorting.
 */
function longestIncreasingRun(values: readonly number[]): Set<number> {
  // tails[length - 1] is the position ending the best run of that length.
  const tails: number[] = [];
  const previous = new Array<number>(values.length);
  for (let position = 0; position < values.length; position += 1) {
    const value = values[position]!;
    let low = 0;
    let high = tails.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (values[tails[middle]!]! < value) low = middle + 1;
      else high = middle;
    }
    previous[position] = low > 0 ? tails[low - 1]! : -1;
    tails[low] = position;
  }
  const run = new Set<number>();
  for (
    let position = tails.length > 0 ? tails[tails.length - 1]! : -1;
    position >= 0;
    position = previous[position]!
  )
    run.add(position);
  return run;
}

export function diffGuiTree(
  desired: readonly GuiDescribedNode[],
  acked: ReadonlyMap<number, GuiAcknowledgedNode>,
  ids: ReadonlyMap<number, number>,
  nextId: number,
  order: ReadonlyMap<number | undefined, readonly number[]>,
  makeHandle: (nodeId: number, lifetime: number) => GuiNodeHandle,
): GuiDiffResult {
  const assigned = new Map(ids);
  let counter = nextId;
  for (const node of desired) {
    if (!assigned.has(node.identity)) assigned.set(node.identity, counter++);
  }
  const nodeIdOf = (identity: number): number => {
    const id = assigned.get(identity);
    if (id === undefined) throw new Error("Missing GUI node identity");
    return id;
  };

  const desiredIds = new Set(desired.map((node) => node.identity));
  const removed = [...acked]
    .filter(([identity]) => !desiredIds.has(identity))
    .map(([identity, ack]) => ({ identity, ack }));
  const removedIds = new Set(removed.map((entry) => entry.identity));
  // Only the topmost removed nodes need an edit; removal cascades.
  const topmost = removed
    .filter(
      ({ ack }) => ack.parent === undefined || !removedIds.has(ack.parent),
    )
    .sort((a, b) => a.ack.nodeId - b.ack.nodeId);
  const edits: GuiEdit[] = topmost.map(({ ack }) => ({
    action: "remove" as const,
    handle: makeHandle(ack.nodeId, ack.lifetime),
  }));

  // Desired children per parent, and each node's desired sibling position.
  const desiredChildren = new Map<number | undefined, number[]>();
  const desiredParent = new Map<number, number | undefined>();
  const position = new Map<number, number>();
  for (const node of desired) {
    let list = desiredChildren.get(node.parent);
    if (!list) desiredChildren.set(node.parent, (list = []));
    position.set(node.identity, list.length);
    desiredParent.set(node.identity, node.parent);
    list.push(node.identity);
  }

  // Simulate the runtime's child lists after the removes, so every placement
  // names the index it takes when applied: inserts and moves clamp their
  // index to the list, and a move first detaches the node from its parent.
  const current = new Map<number | undefined, number[]>();
  for (const [identity, ack] of acked) {
    if (removedIds.has(identity)) continue;
    let list = current.get(ack.parent);
    if (!list) current.set(ack.parent, (list = []));
    list.push(identity);
  }
  const listOf = (parent: number | undefined): number[] => {
    let list = current.get(parent);
    if (!list) current.set(parent, (list = []));
    return list;
  };

  // Stable children keep their place: one longest run of children staying
  // under their parent that is already in desired relative order. A list
  // whose acknowledged order is unknown (after recovery) has no stable
  // children; every child is then placed after its placed predecessor, so the
  // unknown remainder never determines an index.
  const stable = new Set<number>();
  for (const [parent, members] of current) {
    // A single child has only one order.
    const known =
      members.length === 1
        ? members
        : order
            .get(parent)
            ?.filter(
              (identity) =>
                !removedIds.has(identity) &&
                acked.get(identity)?.parent === parent,
            );
    if (!known || known.length !== members.length) continue;
    current.set(parent, known);
    const parentId = parent === undefined ? undefined : assigned.get(parent);
    const staying = known.filter(
      (identity) =>
        desiredParent.has(identity) &&
        desiredParent.get(identity) === parent &&
        acked.get(identity)!.parentId === parentId,
    );
    const run = longestIncreasingRun(
      staying.map((identity) => position.get(identity)!),
    );
    for (const index of run) stable.add(staying[index]!);
  }

  // Placed nodes appear in desired relative order within their simulated
  // list. A placement just after the nearest placed desired predecessor, or
  // first, preserves that order whatever order the placements happen in.
  const placed = new Set(stable);
  const placementIndex = (
    parent: number | undefined,
    identity: number,
  ): number => {
    const siblings = desiredChildren.get(parent)!;
    const list = listOf(parent);
    for (let index = position.get(identity)! - 1; index >= 0; index -= 1) {
      const sibling = siblings[index]!;
      if (placed.has(sibling)) return list.indexOf(sibling) + 1;
    }
    return 0;
  };
  const inserts = desired
    .filter((node) => !acked.has(node.identity))
    .sort((a, b) => nodeIdOf(a.identity) - nodeIdOf(b.identity));
  for (const node of inserts) {
    const index = placementIndex(node.parent, node.identity);
    listOf(node.parent).splice(index, 0, node.identity);
    placed.add(node.identity);
    edits.push({
      action: "insert",
      entity: 0n,
      rootIncarnation: 0n,
      id: nodeIdOf(node.identity),
      ...(node.parent === undefined ? {} : { parent: nodeIdOf(node.parent) }),
      index,
      content: node.content,
      style: node.style,
    });
  }

  // Moves in desired visit order: reparented children and children outside
  // the stable run go just after their nearest placed predecessor. Inserts
  // and moves each keep the placed nodes in desired relative order, so the
  // lists converge on the desired arrangement.
  for (const node of desired) {
    const identity = node.identity;
    const ack = acked.get(identity);
    if (!ack || stable.has(identity)) continue;
    const from = listOf(ack.parent);
    from.splice(from.indexOf(identity), 1);
    const index = placementIndex(node.parent, identity);
    listOf(node.parent).splice(index, 0, identity);
    placed.add(identity);
    const nextParentId =
      node.parent === undefined ? undefined : nodeIdOf(node.parent);
    edits.push({
      action: "move",
      handle: makeHandle(ack.nodeId, ack.lifetime),
      ...(nextParentId === undefined ? {} : { parent: nextParentId }),
      index,
    });
  }

  for (const node of desired) {
    const ack = acked.get(node.identity);
    if (!ack) continue;
    const patch: { content?: GuiNodeContent; style?: GuiNodePatchStyle } = {};
    if (!equalGuiContent(node.content, ack.content))
      patch.content = node.content;
    const style = stylePatch(node.style, ack.style);
    if (style) patch.style = style;
    if (patch.content !== undefined || patch.style !== undefined) {
      edits.push({
        action: "update",
        handle: makeHandle(ack.nodeId, ack.lifetime),
        patch,
      });
    }
  }

  return { edits, ids: assigned, nextId: counter };
}
