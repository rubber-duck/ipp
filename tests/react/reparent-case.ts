import * as React from "react";
import type { Client, Command } from "@ipp/client";
import { Entity, EntityLink, Scalar, createRoot } from "@ipp/react";
import { findEntity, rejectedMessage } from "./fixture-helpers.js";

export async function exerciseReparenting(client: Client): Promise<string[]> {
  const batches: Command[][] = [];
  const observed = new Proxy(client, {
    get(target, key) {
      if (key === "batch")
        return async (operations: Command[]) => {
          batches.push(operations);
          return target.batch(operations);
        };
      const value = Reflect.get(target, key);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(observed, { onError: () => {} });
  const symbols = [
    "reparent-first",
    "reparent-second",
    "reparent-third",
    "reparent-fourth",
  ];
  const scene = (
    parents: readonly (string | bigint | null)[],
    before: readonly (string | bigint | null)[] = [],
    failInsert = false,
  ) =>
    React.createElement(
      React.Fragment,
      null,
      parents.map((parent, index) =>
        React.createElement(
          Entity,
          { key: symbols[index]!, id: symbols[index]! },
          React.createElement(EntityLink, {
            parent,
            before: before[index] ?? null,
          }),
        ),
      ),
      // Inserts follow placements in a commit, so this refusal comes after
      // the placements applied.
      failInsert &&
        React.createElement(
          Entity,
          { key: "missing", bindTo: "reparent-missing" },
          React.createElement(Scalar, { value: 1 }),
        ),
    );
  const checks: string[] = [];
  const check = (condition: boolean, label: string) => {
    if (!condition) throw new Error(label);
    checks.push(label);
  };
  /** The entities placed, in order; retained declarations only move. */
  const placed = () =>
    batches.flat().map((operation) => {
      if (
        operation.kind !== "placeEntity" ||
        operation.entity.kind !== "handle"
      )
        throw new Error(
          `Reparenting replaced a retained declaration: ${operation.kind}`,
        );
      return operation.entity.id;
    });
  try {
    await root.render(scene([null, symbols[0]!]));
    const initial = await client.inspect();
    const first = findEntity(initial, symbols[0]!)!.id;
    const second = findEntity(initial, symbols[1]!)!.id;
    batches.length = 0;
    await root.render(scene([second, null]));
    let state = await client.inspect();
    check(
      placed().join() === [second, first].join() &&
        findEntity(state, symbols[0]!)?.id === first &&
        findEntity(state, symbols[0]!)?.link.parent === second &&
        findEntity(state, symbols[1]!)?.id === second &&
        findEntity(state, symbols[1]!)?.link.parent === null,
      "stable keyed parent reversal orders acknowledged handle updates without reattachment",
    );
    batches.length = 0;
    await root.render(scene([second, null]));
    if (batches.length !== 0)
      throw new Error("An unchanged successful reversal submitted work");

    await root.render(scene([null, symbols[0]!, symbols[1]!, symbols[2]!]));
    state = await client.inspect();
    const generations = symbols.map((symbol) => findEntity(state, symbol)!.id);
    const permutations = (values: number[]): number[][] =>
      values.length === 0
        ? [[]]
        : values.flatMap((value, index) =>
            permutations(
              values.filter((_, position) => position !== index),
            ).map((tail) => [value, ...tail]),
          );
    batches.length = 0;
    for (const [index, order] of permutations([0, 1, 2, 3]).entries()) {
      const parentIndices = symbols.map((_, target) => {
        const position = order.indexOf(target);
        return position === 0 ? null : order[position - 1]!;
      });
      const parents = parentIndices.map((parent) =>
        parent === null
          ? null
          : index % 2 === 0
            ? symbols[parent]!
            : generations[parent]!,
      );
      await root.render(scene(parents));
      state = await client.inspect();
      for (const [target, symbol] of symbols.entries()) {
        const entity = findEntity(state, symbol)!;
        const parent = parentIndices[target]!;
        if (
          entity.id !== generations[target] ||
          entity.link.parent !== (parent === null ? null : generations[parent])
        )
          throw new Error(
            `Reparent chain permutation changed identity or missed its parent: ${order}`,
          );
      }
    }
    check(
      placed().every((entity) => generations.includes(entity)),
      "all four-entity chain permutations preserve generations and only move them",
    );

    await root.render(scene([null, symbols[0]!, symbols[1]!, symbols[2]!]));
    batches.length = 0;
    await root.render(scene([generations[2]!, null, symbols[1]!, symbols[2]!]));
    if (placed().join() !== [generations[1], generations[0]].join())
      throw new Error(
        "Reparent ordering did not traverse an unchanged parent declaration",
      );
    batches.length = 0;
    await root.render(
      scene(
        [generations[2]!, symbols[2]!, generations[3]!, null],
        [symbols[1]!, null, null, null],
      ),
    );
    const tree = await client.inspectTreePage({
      root: generations[3]!,
      maxDepth: 2,
    });
    check(
      placed().join() ===
        [
          generations[3],
          generations[2],
          generations[1],
          generations[0],
        ].join() &&
        tree.nodes
          .filter((node) => node.depth === 2)
          .map((node) => node.id)
          .join() === [first, second].join(),
      "parent dependencies traverse unchanged declarations and preserve before ordering",
    );

    await root.render(scene([null, symbols[0]!]));
    const invalid = scene([second, null], [], true);
    const message = await rejectedMessage(root.render(invalid));
    state = await client.inspect();
    const prefixApplied =
      findEntity(state, symbols[0]!)?.link.parent === second &&
      findEntity(state, symbols[1]!)?.link.parent === null;
    const submitted = batches.length;
    await rejectedMessage(root.render(invalid));
    const unchangedRejected = batches.length === submitted;
    await root.render(scene([null, symbols[0]!]));
    state = await client.inspect();
    check(
      message.includes("MissingSymbolicId") &&
        prefixApplied &&
        unchangedRejected &&
        findEntity(state, symbols[0]!)?.id === first &&
        findEntity(state, symbols[1]!)?.id === second &&
        findEntity(state, symbols[0]!)?.link.parent === null &&
        findEntity(state, symbols[1]!)?.link.parent === first,
      "reparent prefixes survive partial rejection and corrected rendering reconciles the keyed entities",
    );
    // Unmount deletes nothing; removing the declarations deletes them.
    await root.render(null);
    return checks;
  } finally {
    await root.unmount();
  }
}
