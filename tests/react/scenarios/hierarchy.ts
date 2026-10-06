import * as React from "react";
import {
  createRoot,
  Children,
  Entity,
  EntityLink,
  ParentJoint,
  LookAt,
  Transform,
} from "@ipp/react";
import type { Inspection } from "@ipp/client";
import { exerciseLinkAnimation } from "./link-animation.js";
import { exerciseReparenting } from "./reparent.js";
import {
  type ReactRuntimeConfiguration,
  type GeneratedClient,
  type GeneratedModule,
  connect,
  requireSuccess,
  requireAlias,
  rejectedMessage,
  findEntity,
  settleRoots,
} from "../support/react-runtime.js";

/** Explicit parenting through the public package and real acknowledged generations. */
export async function childrenHierarchy(
  configuration: ReactRuntimeConfiguration,
): Promise<string[]> {
  const { contract, client } = await connect(configuration);
  try {
    return await exerciseEntityLinks(client, contract);
  } finally {
    await client.close();
  }
}

export async function exerciseEntityLinks(
  client: GeneratedClient,
  contract: GeneratedModule,
): Promise<string[]> {
  const root = createRoot(client, { onError: () => {} });
  const peer = createRoot(client, { onError: () => {} });
  const checks: string[] = [];
  const check = (condition: boolean, label: string) => {
    if (!condition) throw new Error(label);
    checks.push(label);
  };
  const parentOf = (state: Inspection, symbol: string) =>
    findEntity(state, symbol)?.link.parent;
  const childrenOf = async (parent: bigint): Promise<bigint[]> => {
    const page = await client.inspectTreePage({ root: parent, maxDepth: 1 });
    return page.nodes.filter((node) => node.depth === 1).map((node) => node.id);
  };
  try {
    const producer = await requireSuccess(
      client.batch([
        contract.Entity.create(1, { symbolicId: "producer-parent-a" }),
        contract.Entity.create(2, { symbolicId: "producer-parent-b" }),
        contract.Entity.create(3, { symbolicId: "producer-child" }),
      ]),
    );
    const parentA = requireAlias(producer, 1);
    const parentB = requireAlias(producer, 2);
    const child = requireAlias(producer, 3);
    await requireSuccess(
      client.batch([
        {
          kind: "placeEntity",
          entity: contract.Entity.handle(child),
          placement: { parent: contract.Entity.handle(parentA), before: null },
        },
      ]),
    );
    function BoundChild() {
      return React.createElement(
        Entity,
        { bindTo: "producer-child" },
        React.createElement(Transform, { x: 2 }),
        React.createElement(
          Children,
          null,
          React.createElement(Entity, { id: "react-grandchild" }),
        ),
      );
    }
    const scene = (id: string, reverse = false) =>
      React.createElement(
        Entity,
        { id },
        React.createElement(Transform, { x: 3 }),
        React.createElement(
          Children,
          null,
          React.createElement(
            React.Fragment,
            null,
            (reverse ? ["owned", "bound"] : ["bound", "owned"]).map((key) =>
              key === "bound"
                ? React.createElement(BoundChild, { key })
                : React.createElement(Entity, { key, id: "react-sibling" }),
            ),
          ),
        ),
        React.createElement(Entity, { id: "react-plain-nested" }),
      );
    await root.render(scene("react-parent"));
    let state = await client.inspect();
    const parent = findEntity(state, "react-parent")!.id;
    const sibling = findEntity(state, "react-sibling")!.id;
    const grandchild = findEntity(state, "react-grandchild")!.id;
    check(
      parentOf(state, "producer-child") === parent &&
        parentOf(state, "react-sibling") === parent &&
        parentOf(state, "react-grandchild") === child,
      "Children assigns each enclosing parent through fragments and function components",
    );
    check(
      parentOf(state, "react-plain-nested") === null,
      "plain nesting leaves parenting explicit",
    );
    await root.render(scene("react-parent", true));
    state = await client.inspect();
    check(
      findEntity(state, "react-sibling")?.id === sibling &&
        findEntity(state, "react-grandchild")?.id === grandchild &&
        (await childrenOf(parent)).join() === [sibling, child].join(),
      "keyed reordering changes core sibling order and preserves generations",
    );
    await requireSuccess(
      client.batch([
        contract.Entity.create(4, { symbolicId: "foreign-child" }),
        {
          kind: "placeEntity",
          entity: contract.Entity.alias(4),
          placement: { parent: contract.Entity.handle(parent), before: null },
        },
      ]),
    );
    await root.render(scene("react-parent-renamed", true));
    state = await client.inspect();
    const renamed = findEntity(state, "react-parent-renamed")!.id;
    check(
      renamed !== parent &&
        parentOf(state, "producer-child") === renamed &&
        parentOf(state, "react-sibling") === renamed &&
        findEntity(state, "react-sibling")?.id === sibling,
      "parent replacement updates retained child relationships",
    );
    check(
      parentOf(state, "foreign-child") === null,
      "declared parent deletion leaves an unrelated client child alive as a root",
    );
    await requireSuccess(
      client.batch([
        {
          kind: "placeEntity",
          entity: contract.Entity.handle(child),
          placement: { parent: contract.Entity.handle(parentB), before: null },
        },
      ]),
    );
    await root.render(null);
    state = await client.inspect();
    check(
      parentOf(state, "producer-child") === parentB &&
        !state.entities.some((entity) =>
          entity.metadata.symbolicId?.startsWith("react-"),
        ),
      "unmount deletes declared descendants and leaves the bound child where a client placed it",
    );

    const references = (target: bigint) =>
      React.createElement(
        Entity,
        { id: "react-references" },
        React.createElement(EntityLink, { parent: target }),
        // Joint parents need a World that selects the skinning Systems.
        client.manifest?.components.includes(
          client.components.ParentJoint!.id,
        ) && React.createElement(ParentJoint, { ordinal: 0xffffffff }),
        React.createElement(LookAt, { target }),
      );
    await root.render(references(parentA));
    await root.render(references(parentB));
    state = await client.inspect();
    const target = findEntity(state, "react-references")!.components.find(
      (value) => value.component === client.components.LookAt!.id,
    )?.fields.target;
    check(
      parentOf(state, "react-references") === parentB && target === parentB,
      "explicit entity reference props update by handle value",
    );
    await root.render(null);

    const duplicate = React.createElement(
      React.Fragment,
      null,
      ...["a", "b"].map((id) =>
        React.createElement(
          Entity,
          { key: id, id: `duplicate-${id}` },
          React.createElement(
            Children,
            null,
            React.createElement(Entity, { bindTo: "producer-child" }),
          ),
        ),
      ),
    );
    const permutations = (values: string[]): string[][] =>
      values.length === 0
        ? [[]]
        : values.flatMap((value, index) =>
            permutations(
              values.filter((_, position) => position !== index),
            ).map((tail) => [value, ...tail]),
          );
    const orderedScene = (order: string[]) =>
      React.createElement(
        Entity,
        { id: "ordered-parent" },
        React.createElement(
          Children,
          null,
          order.map((id) => React.createElement(Entity, { key: id, id })),
        ),
      );
    const symbols = [
      "ordered-first",
      "ordered-second",
      "ordered-third",
      "ordered-fourth",
    ];
    await root.render(orderedScene(symbols));
    const orderedState = await client.inspect();
    const generations = new Map(
      symbols.map((symbol) => [symbol, findEntity(orderedState, symbol)!.id]),
    );
    const orderedParent = findEntity(orderedState, "ordered-parent")!.id;
    for (const order of permutations(symbols)) {
      await root.render(orderedScene(order));
      const actual = await childrenOf(orderedParent);
      if (
        actual.join() !== order.map((symbol) => generations.get(symbol)!).join()
      )
        throw new Error(
          `Keyed sibling permutation did not preserve order and identity: ${order}`,
        );
    }
    await root.render(
      orderedScene([symbols[2]!, "ordered-inserted", symbols[0]!]),
    );
    state = await client.inspect();
    check(
      (await childrenOf(orderedParent)).join() ===
        [
          generations.get(symbols[2]!),
          findEntity(state, "ordered-inserted")!.id,
          generations.get(symbols[0]!),
        ].join(),
      "all keyed permutations and insertion-removal preserve explicit order and retained identities",
    );
    await root.render(null);
    check(
      (await rejectedMessage(root.render(duplicate))).includes("Children"),
      "Children rejects two declarations bound to the same actual entity",
    );
    await root.render(null);
    const explicitConflict = React.createElement(
      React.Fragment,
      null,
      React.createElement(
        Entity,
        { id: "implicit-parent" },
        React.createElement(
          Children,
          null,
          React.createElement(Entity, { bindTo: "producer-child" }),
        ),
      ),
      React.createElement(
        Entity,
        { bindTo: "producer-child" },
        React.createElement(EntityLink, { parent: parentA }),
      ),
    );
    check(
      (await rejectedMessage(root.render(explicitConflict))).includes(
        "Children",
      ),
      "Children rejects an EntityLink on another binding to its child",
    );
    await root.render(null);

    const listed = (keys: readonly string[]) =>
      React.createElement(
        Entity,
        { id: "listed-parent" },
        React.createElement(
          Children,
          null,
          ...keys.map((key) =>
            React.createElement(Entity, { key, id: "listed-item" }),
          ),
        ),
      );
    await root.render(listed(["first"]));
    state = await client.inspect();
    const listedParent = findEntity(state, "listed-parent")!.id;
    const listedItem = findEntity(state, "listed-item")!.id;
    await root.render(listed(["second"]));
    state = await client.inspect();
    check(
      findEntity(state, "listed-item")?.id === listedItem &&
        parentOf(state, "listed-item") === listedParent,
      "a keyed remount of an Entity id keeps its entity and placement",
    );
    const duplicateMessage = await rejectedMessage(
      root.render(listed(["second", "third"])),
    );
    const afterDuplicate = await client.inspect();
    check(
      duplicateMessage.includes("Duplicate Entity id: listed-item") &&
        afterDuplicate.entities.length === state.entities.length &&
        findEntity(afterDuplicate, "listed-item")?.id === listedItem &&
        parentOf(afterDuplicate, "listed-item") === listedParent,
      "two Entity declarations of one id reject the render and send nothing",
    );
    await root.render(null);

    const direct = (parent: bigint | null) =>
      React.createElement(
        Entity,
        { bindTo: "producer-child" },
        React.createElement(EntityLink, { parent }),
      );
    await root.render(direct(parentA));
    await peer.render(direct(parentB));
    await root.render(direct(null));
    check(
      parentOf(await client.inspect(), "producer-child") === null,
      "multiple roots place one entity and the last placement wins",
    );
    await peer.render(null);
    check(
      parentOf(await client.inspect(), "producer-child") === null,
      "withdrawing a root's link leaves the entity where it was last placed",
    );
    await root.render(null);

    const invalid = React.createElement(
      Entity,
      { id: "react-partial-parent" },
      React.createElement(
        Children,
        null,
        React.createElement(Entity, { bindTo: "react-partial-parent" }),
      ),
    );
    checks.push(...(await exerciseLinkAnimation(client, parentA, parentB)));
    checks.push(...(await exerciseReparenting(client)));
    await rejectedMessage(root.render(invalid));
    check(
      findEntity(await client.inspect(), "react-partial-parent") !== undefined,
      "a rejected parent relationship keeps its applied creation",
    );
    await root.render(scene("react-corrected-parent"));
    state = await client.inspect();
    check(
      findEntity(state, "react-partial-parent") === undefined &&
        parentOf(state, "producer-child") ===
          findEntity(state, "react-corrected-parent")?.id,
      "corrected rendering deletes the partial entity and rebuilds relationships",
    );
    const mounted = (await client.inspect()).entities.length;
    await root.unmount();
    check(
      (await client.inspect()).entities.length === mounted,
      "unmount deletes nothing",
    );
    // Removing the declarations deletes the declared entities. The bound
    // child's last parent was a declared entity, so deleting it leaves the
    // child a root.
    const cleanup = createRoot(client);
    await cleanup.render(scene("react-corrected-parent"));
    await cleanup.render(null);
    await cleanup.unmount();
    state = await client.inspect();
    check(
      state.entities.length === 4 && parentOf(state, "producer-child") === null,
      "removing the declarations preserves only client entities",
    );
    return checks;
  } finally {
    await settleRoots([root, peer]);
  }
}

/** Dynamic declaration reconciliation through the actual generated worker protocol. */
