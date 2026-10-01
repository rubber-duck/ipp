import type { ReactEntityLinkDescription } from "./tree.js";

export interface ResolvedEntityLink extends ReactEntityLinkDescription {
  readonly target: bigint;
  readonly parent: bigint | null;
  readonly before: bigint | null;
}

export function orderEntityLinks(
  links: readonly ResolvedEntityLink[],
): ResolvedEntityLink[] {
  const targets = new Map<bigint, ResolvedEntityLink>();
  for (const link of links) {
    if (targets.has(link.target))
      throw new Error(
        "Children or EntityLink conflicts with another link declaration for the same bound entity",
      );
    targets.set(link.target, link);
  }
  for (const link of links) {
    const sibling = link.before === null ? undefined : targets.get(link.before);
    if (sibling && sibling.parent !== link.parent)
      throw new Error("EntityLink before must have the same declared parent");
  }
  const ordered: ResolvedEntityLink[] = [];
  const placed = new Set<bigint>();
  const pending = new Set<bigint>();
  for (const initial of links) {
    const stack = [{ link: initial, exiting: false }];
    while (stack.length) {
      const { link, exiting } = stack.pop()!;
      if (placed.has(link.target)) continue;
      if (exiting) {
        pending.delete(link.target);
        placed.add(link.target);
        ordered.push(link);
        continue;
      }
      if (pending.has(link.target))
        throw new Error(
          "EntityLink parent or sibling dependencies contain a cycle",
        );
      pending.add(link.target);
      stack.push({ link, exiting: true });
      const sibling =
        link.before === null ? undefined : targets.get(link.before);
      if (sibling) stack.push({ link: sibling, exiting: false });
      const parent =
        link.parent === null ? undefined : targets.get(link.parent);
      if (parent) stack.push({ link: parent, exiting: false });
    }
  }
  return ordered;
}

/**
 * Sibling order per resolved parent. A group whose `before` links form one
 * chain ending at the parent's end lists its link identities in declared
 * order; any other group (a foreign or shared `before`, or several ends) is
 * `undefined` and places links individually.
 */
export function siblingChains(
  links: readonly ResolvedEntityLink[],
): Map<bigint | null, readonly number[] | undefined> {
  const groups = new Map<bigint | null, ResolvedEntityLink[]>();
  for (const link of links) {
    const group = groups.get(link.parent);
    if (group) group.push(link);
    else groups.set(link.parent, [link]);
  }
  const chains = new Map<bigint | null, readonly number[] | undefined>();
  for (const [parent, group] of groups) {
    const preceding = new Map<bigint | null, ResolvedEntityLink>();
    let chain = true;
    for (const link of group) {
      if (preceding.has(link.before)) chain = false;
      preceding.set(link.before, link);
    }
    const reversed: number[] = [];
    for (
      let link = chain ? preceding.get(null) : undefined;
      link;
      link = preceding.get(link.target)
    ) {
      reversed.push(link.identity);
      if (reversed.length > group.length) break;
    }
    chains.set(
      parent,
      chain && reversed.length === group.length
        ? reversed.reverse()
        : undefined,
    );
  }
  return chains;
}

/**
 * The largest subset of `order` whose previous positions already increase:
 * those siblings keep their placement while every other sibling moves.
 * `previous` maps a link identity to its position in the last applied order;
 * identities without a position always move.
 */
export function siblingsInPlace(
  order: readonly number[],
  previous: ReadonlyMap<number, number>,
): Set<number> {
  const candidates = order.filter((identity) => previous.has(identity));
  // Patience sorting: tails[length - 1] indexes the candidate ending the
  // best increasing run of that length.
  const tails: number[] = [];
  const predecessors = new Array<number>(candidates.length).fill(-1);
  for (let index = 0; index < candidates.length; index++) {
    const position = previous.get(candidates[index]!)!;
    let low = 0;
    let high = tails.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (previous.get(candidates[tails[middle]!]!)! < position)
        low = middle + 1;
      else high = middle;
    }
    if (low > 0) predecessors[index] = tails[low - 1]!;
    tails[low] = index;
  }
  const kept = new Set<number>();
  for (
    let index = tails.length ? tails[tails.length - 1]! : -1;
    index >= 0;
    index = predecessors[index]!
  )
    kept.add(candidates[index]!);
  return kept;
}
