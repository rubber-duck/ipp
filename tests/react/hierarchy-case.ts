import * as React from "react";
import {
  createRoot,
  Children,
  Entity,
  Hierarchy,
  LookAt,
  Transform,
} from "@ipp/react";
import type { Inspection } from "@ipp/client";
import {
  type ReactRuntimeConfiguration,
  connect,
  requireSuccess,
  requireAlias,
  rejectedMessage,
  findEntity,
  settleRoots,
} from "./fixture-helpers.js";

/** Explicit parenting through the public package and real acknowledged generations. */
export async function childrenHierarchy(
  configuration: ReactRuntimeConfiguration,
): Promise<string[]> {
  const { contract, client } = await connect(configuration);
  const root = createRoot(client, { onError: () => {} });
  const checks: string[] = [];
  const check = (condition: boolean, label: string) => {
    if (!condition) throw new Error(label);
    checks.push(label);
  };
  const hierarchy = client.components.Hierarchy!;
  const parentWrite = (id: bigint) => ({
    offset: hierarchy.fields.parent!.offset,
    value: { kind: "entity" as const, value: { kind: "handle" as const, id } },
  });
  const parentOf = (
    state: Inspection,
    symbol: string,
    layer: "base" | "effective" = "effective",
  ) =>
    findEntity(state, symbol)?.[layer].find(
      (value) => value.component === hierarchy.id,
    )?.fields.parent;
  try {
    const producer = await requireSuccess(
      client.batch([
        contract.Entity.create(1, { symbolicId: "producer-parent-a" }),
        contract.Entity.create(2, { symbolicId: "producer-parent-b" }),
        contract.Entity.create(3, { symbolicId: "producer-child" }),
      ]),
    );
    const a = requireAlias(producer, 1);
    const b = requireAlias(producer, 2);
    const child = requireAlias(producer, 3);
    await requireSuccess(
      client.batch([
        {
          kind: "insertComponent",
          entity: contract.Entity.handle(child),
          component: hierarchy.id,
          fields: [parentWrite(a)],
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
      parentOf(state, "react-plain-nested") === undefined &&
        parentOf(state, "producer-child", "base") === a,
      "plain nesting leaves parenting explicit and preserves producer base",
    );
    await root.render(scene("react-parent", true));
    state = await client.inspect();
    check(
      findEntity(state, "react-sibling")?.id === sibling &&
        findEntity(state, "react-grandchild")?.id === grandchild,
      "keyed reordering preserves owned generations",
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
    await requireSuccess(
      client.batch([
        {
          kind: "setField",
          entity: contract.Entity.handle(child),
          component: hierarchy.id,
          field: parentWrite(b),
        },
      ]),
    );
    await root.render(null);
    state = await client.inspect();
    check(
      parentOf(state, "producer-child") === b &&
        !state.entities.some((entity) =>
          entity.metadata.symbolicId?.startsWith("react-"),
        ),
      "unmount deletes owned descendants and reveals latest bound parent",
    );

    const references = (target: bigint) =>
      React.createElement(
        Entity,
        { id: "react-references" },
        React.createElement(Hierarchy, { parent: target }),
        React.createElement(LookAt, { target }),
      );
    await root.render(references(a));
    await root.render(references(b));
    state = await client.inspect();
    const target = findEntity(state, "react-references")!.effective.find(
      (value) => value.component === client.components.LookAt!.id,
    )?.fields.target;
    check(
      parentOf(state, "react-references") === b && target === b,
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
        React.createElement(Hierarchy, { parent: a }),
      ),
    );
    check(
      (await rejectedMessage(root.render(explicitConflict))).includes(
        "Children",
      ),
      "Children rejects an explicit Hierarchy on another binding to its child",
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
    await rejectedMessage(root.render(invalid));
    check(
      findEntity(await client.inspect(), "react-partial-parent") !== undefined,
      "a rejected parent relationship retains acknowledged partial ownership",
    );
    await root.render(scene("react-corrected-parent"));
    state = await client.inspect();
    check(
      findEntity(state, "react-partial-parent") === undefined &&
        parentOf(state, "producer-child") ===
          findEntity(state, "react-corrected-parent")?.id,
      "corrected rendering cleans partial ownership and rebuilds relationships",
    );
    await root.unmount();
    state = await client.inspect();
    check(
      state.entities.length === 3 && parentOf(state, "producer-child") === b,
      "final cleanup preserves only producer entities",
    );
    return checks;
  } finally {
    await settleRoots([root]);
    await client.close();
  }
}

/** Dynamic declaration reconciliation through the actual generated worker protocol. */
