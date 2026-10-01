import assert from "node:assert/strict";
import test from "node:test";
import { createElement, Fragment, StrictMode } from "react";
import type {
  BatchOutcome,
  Command,
  EntityRef,
  InspectionPage,
  WorldManifest,
} from "@ipp/client";
import {
  Children,
  Entity,
  EntityLink,
  ParentJoint,
  Scalar,
  createRoot,
  type ReactWorldClient,
} from "../src/index.js";
import { AnimationMailbox } from "../src/animation.js";
import { describeAnimation } from "../src/animation_tree.js";
import {
  orderEntityLinks,
  type ResolvedEntityLink,
} from "../src/entity_links.js";

type Placement = { parent: EntityRef | null; before: EntityRef | null };

/**
 * A World for link commits: creations (adopting a live symbolic id), deletes,
 * placements and component writes, with aliases and symbols resolved like the
 * core and reported in the outcome.
 */
class LinkBoundary implements ReactWorldClient {
  session = 1n;
  schemaHash = 1n;
  manifest: WorldManifest | undefined;
  capabilities = {
    spatial: false,
    textures: false,
    builtinAssets: false,
    picking: false,
    debugGeometry: false,
    pbr: false,
    shadows: false,
    skeletalAnimation: false,
    meshPoses: false,
  };
  components: ReactWorldClient["components"] = {
    Scalar: { id: 17, fields: { value: { offset: 0, kind: 1 } } },
    ParentJoint: { id: 18, fields: { ordinal: { offset: 0, kind: 3 } } },
  };
  calls: Command[][] = [];
  /** Each call's applied operations with aliases and symbols replaced by handles. */
  resolved: Command[][] = [];
  /** Live entities by symbolic id. */
  entities = new Map<string, bigint>();
  nextEntity = 1000n;
  inspections = 0;
  failLink = false;
  beforeReply: ((operations: Command[]) => Promise<void>) | undefined;

  async batch(operations: Command[]): Promise<BatchOutcome> {
    this.calls.push(operations);
    const aliases = new Map<number, bigint>();
    const outcome = {
      batchId: 1n,
      tick: 1n,
      aliases: [] as { alias: number; id: bigint }[],
      symbols: [] as { symbol: string; id: bigint }[],
      effects: [] as BatchOutcome["effects"],
    };
    const handle = (reference: EntityRef): EntityRef => {
      if (reference.kind === "handle") return reference;
      const id =
        reference.kind === "alias"
          ? aliases.get(reference.alias)
          : this.entities.get(reference.symbol);
      assert.ok(id !== undefined, `unresolved ${JSON.stringify(reference)}`);
      if (
        reference.kind === "symbol" &&
        !outcome.symbols.some((entry) => entry.symbol === reference.symbol)
      )
        outcome.symbols.push({ symbol: reference.symbol, id });
      return { kind: "handle", id };
    };
    const place = (placement: Placement): Placement => ({
      parent: placement.parent && handle(placement.parent),
      before: placement.before && handle(placement.before),
    });
    const resolved: Command[] = [];
    this.resolved.push(resolved);
    for (const [index, operation] of operations.entries()) {
      if (operation.kind === "placeEntity" && this.failLink) {
        this.failLink = false;
        return {
          ...outcome,
          ok: false,
          error: {
            scope: "operation",
            operation: index,
            reason: "InvalidValue",
          },
        };
      }
      switch (operation.kind) {
        case "create": {
          const symbolicId = operation.metadata.symbolicId!;
          const existing = operation.adopt
            ? this.entities.get(symbolicId)
            : undefined;
          const id = existing ?? this.nextEntity++;
          this.entities.set(symbolicId, id);
          aliases.set(operation.alias, id);
          outcome.aliases.push({ alias: operation.alias, id });
          if (existing !== undefined)
            outcome.effects.push({ operation: index, kind: "adopted" });
          resolved.push(operation);
          break;
        }
        case "delete": {
          const target = handle(operation.entity);
          assert.ok(target.kind === "handle");
          for (const [symbolicId, id] of this.entities)
            if (id === target.id) this.entities.delete(symbolicId);
          resolved.push({ ...operation, entity: target });
          break;
        }
        case "placeEntity":
          resolved.push({
            ...operation,
            entity: handle(operation.entity),
            placement: place(operation.placement),
          });
          break;
        case "insertComponent":
        case "setField":
        case "removeComponent":
          resolved.push({ ...operation, entity: handle(operation.entity) });
          break;
        default:
          resolved.push(operation);
      }
    }
    await this.beforeReply?.(operations);
    return { ...outcome, ok: true };
  }

  async inspectPage(): Promise<InspectionPage> {
    this.inspections++;
    return {
      next: 0n,
      tick: 1n,
      time: 0,
      entities: [...this.entities].map(([symbolicId, id]) => ({
        id,
        metadata: { symbolicId, classes: [] },
        link: { parent: null, order: 0n },
        components: [],
      })),
      resources: [],
      renderDiagnostics: [],
    };
  }

  /** The applied operations of every call, with handles. */
  applied(): Command[] {
    return this.resolved.flat();
  }
}

const handle = (id: bigint) => ({ kind: "handle" as const, id });
const scene = (order = ["alpha", "beta", "gamma"]) =>
  createElement(
    Entity,
    { id: "parent" },
    createElement(
      Children,
      null,
      order.map((id) => createElement(Entity, { key: id, id })),
    ),
  );
const quiet = { onError: () => {} };

/** Placements as [entity, parent, before] handles. */
function placements(operations: Command[]) {
  return operations.flatMap((operation) =>
    operation.kind === "placeEntity"
      ? [
          [
            operation.entity,
            operation.placement.parent,
            operation.placement.before,
          ],
        ]
      : [],
  );
}

/** Deleted handles, in order. */
function deletions(operations: Command[]) {
  return operations.flatMap((operation) =>
    operation.kind === "delete" ? [operation.entity] : [],
  );
}

test("Children resolves acknowledged entities, orders links and retains keyed declarations", async () => {
  const client = new LinkBoundary();
  const root = createRoot(client, quiet);
  try {
    await root.render(scene());
    // Declared entities and their placements travel in one batch that names
    // the new entities by their batch aliases.
    assert.equal(client.calls.length, 1);
    assert.deepEqual(
      client.calls[0]!.map((operation) =>
        operation.kind === "create"
          ? [operation.kind, operation.metadata.symbolicId, operation.adopt]
          : [operation.kind],
      ),
      [
        ["create", "parent", true],
        ["create", "alpha", true],
        ["create", "beta", true],
        ["create", "gamma", true],
        ["placeEntity"],
        ["placeEntity"],
        ["placeEntity"],
      ],
    );
    const alias = (symbolicId: string): EntityRef => {
      const create = client.calls[0]!.find(
        (operation) =>
          operation.kind === "create" &&
          operation.metadata.symbolicId === symbolicId,
      );
      assert.ok(create?.kind === "create");
      return { kind: "alias", alias: create.alias };
    };
    assert.deepEqual(placements(client.calls[0]!), [
      [alias("gamma"), alias("parent"), null],
      [alias("beta"), alias("parent"), alias("gamma")],
      [alias("alpha"), alias("parent"), alias("beta")],
    ]);
    const id = (name: string) => handle(client.entities.get(name)!);
    client.calls = [];
    await root.render(scene());
    assert.equal(client.calls.length, 0);
    // Alpha and beta keep their relative order and their placements; only
    // gamma moves, before alpha.
    client.resolved = [];
    await root.render(scene(["gamma", "alpha", "beta"]));
    assert.deepEqual(client.applied(), [
      {
        kind: "placeEntity",
        entity: id("gamma"),
        placement: { parent: id("parent"), before: id("alpha") },
      },
    ]);
    // Removing the Children deletes the declared children, not the subtree.
    const children = ["alpha", "beta", "gamma"].map(id);
    client.resolved = [];
    await root.render(createElement(Entity, { id: "parent" }));
    assert.deepEqual(new Set(deletions(client.applied())), new Set(children));
    assert.deepEqual(placements(client.applied()), []);
    assert.ok(
      !client.applied().some((operation) => operation.kind === "deleteSubtree"),
    );
    assert.deepEqual([...client.entities.keys()], ["parent"]);
  } finally {
    await root.unmount();
  }
  assert.deepEqual(
    [...client.entities.keys()],
    ["parent"],
    "unmount deletes nothing",
  );
});

test("replacing a parent deletes it after placing its child under the new parent in one batch", async () => {
  const client = new LinkBoundary();
  const root = createRoot(client, quiet);
  const declaration = (id: string) =>
    createElement(
      Entity,
      { id },
      createElement(Children, null, createElement(Entity, { id: "child" })),
    );
  try {
    await root.render(declaration("first-parent"));
    const first = handle(client.entities.get("first-parent")!);
    const child = handle(client.entities.get("child")!);
    client.calls = [];
    client.resolved = [];
    await root.render(declaration("second-parent"));
    assert.equal(
      client.calls.length,
      1,
      "Replacement and relink are one batch",
    );
    const second = handle(client.entities.get("second-parent")!);
    assert.deepEqual(
      client.applied().map((operation) => operation.kind),
      ["create", "placeEntity", "delete"],
    );
    assert.deepEqual(placements(client.applied()), [[child, second, null]]);
    assert.deepEqual(deletions(client.applied()), [first]);
  } finally {
    await root.unmount();
  }
});

test("explicit links resolve forward names and keep ParentJoint a component", async () => {
  const client = new LinkBoundary();
  const root = createRoot(client, quiet);
  try {
    await root.render(
      createElement(
        Fragment,
        null,
        createElement(
          Entity,
          { id: "child" },
          createElement(EntityLink, { parent: "parent" }),
          createElement(ParentJoint, { ordinal: 3 }),
        ),
        createElement(Entity, { id: "parent" }),
      ),
    );
    const operations = client.applied();
    const child = handle(client.entities.get("child")!);
    assert.deepEqual(placements(operations), [
      [child, handle(client.entities.get("parent")!), null],
    ]);
    assert.deepEqual(
      operations.filter((operation) => operation.kind === "insertComponent"),
      [
        {
          kind: "insertComponent",
          entity: child,
          component: 18,
          fields: [{ offset: 0, value: { kind: "u32", value: 3 } }],
          adopt: true,
        },
      ],
    );
  } finally {
    await root.unmount();
  }
});

test("competing declarations reject by actual bound identity, not spelling, before any placement", async () => {
  const client = new LinkBoundary();
  client.entities.set("first-name", 500n);
  client.entities.set("second-name", 500n);
  const root = createRoot(client, quiet);
  try {
    await assert.rejects(
      root.render(
        createElement(
          Fragment,
          null,
          ...["first-name", "second-name"].map((bindTo) =>
            createElement(
              Entity,
              { key: bindTo, bindTo },
              createElement(EntityLink, { parent: null }),
            ),
          ),
        ),
      ),
      /same bound entity/,
    );
    assert.ok(client.inspections > 0, "bound handles were resolved first");
    assert.deepEqual(client.calls, []);
    await root.render(null);
    assert.deepEqual(client.calls, [], "bound entities are never deleted");
  } finally {
    await root.unmount();
  }
});

test("manifest admission uses selected operations and components rather than compiled presence", async () => {
  const client = new LinkBoundary();
  client.manifest = { systems: [], components: [], operations: [] };
  const rejected = createRoot(client, quiet);
  await assert.rejects(rejected.render(scene()), /select entityLinks/);
  await rejected.unmount();
  client.manifest = {
    systems: [],
    components: [],
    operations: ["entityLinks"],
  };
  const root = createRoot(client, quiet);
  try {
    await root.render(scene());
    await assert.rejects(
      root.render(
        createElement(
          Entity,
          { id: "scalar" },
          createElement(Scalar, { value: 2 }),
        ),
      ),
      /select Scalar/,
    );
  } finally {
    await root.unmount();
  }
});

test("a rejected link batch keeps its applied prefix and a corrected tree reconciles from it", async () => {
  const client = new LinkBoundary();
  const root = createRoot(client, quiet);
  client.failLink = true;
  try {
    await assert.rejects(root.render(scene()), /InvalidValue/);
    // Every creation before the refused placement applied.
    const created = ["alpha", "beta", "gamma"].map((name) =>
      handle(client.entities.get(name)!),
    );
    const parent = handle(client.entities.get("parent")!);
    client.calls = [];
    client.resolved = [];
    await root.render(scene(["corrected"]));
    const operations = client.applied();
    assert.deepEqual(
      operations.map((operation) =>
        operation.kind === "create"
          ? `${operation.kind}:${operation.metadata.symbolicId}`
          : operation.kind,
      ),
      ["create:corrected", "placeEntity", "delete", "delete", "delete"],
    );
    assert.deepEqual(placements(operations), [
      [handle(client.entities.get("corrected")!), parent, null],
    ]);
    assert.deepEqual(new Set(deletions(operations)), new Set(created));
  } finally {
    await root.unmount();
  }
});

test("StrictMode unmount drains a pending link acknowledgement and deletes nothing", async () => {
  const client = new LinkBoundary();
  let entered!: () => void;
  let release!: () => void;
  const submitted = new Promise<void>((resolve) => {
    entered = resolve;
  });
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  client.beforeReply = async (operations) => {
    if (operations.some((operation) => operation.kind === "placeEntity")) {
      entered();
      await held;
    }
  };
  const root = createRoot(client, quiet);
  const rendered = root.render(createElement(StrictMode, null, scene()));
  await submitted;
  const unmounted = root.unmount();
  assert.ok(
    !client.calls.flat().some((operation) => operation.kind === "delete"),
  );
  release();
  await Promise.all([rendered, unmounted]);
  assert.equal(deletions(client.applied()).length, 0);
  assert.equal(client.entities.size, 4);
});

test("a replaced session fences rendering and unmount sends nothing", async () => {
  const client = new LinkBoundary();
  client.entities.set("target", 700n);
  const root = createRoot(client, quiet);
  const declaration = (parent: bigint | null) =>
    createElement(
      Fragment,
      null,
      createElement(
        Entity,
        { bindTo: "target" },
        createElement(EntityLink, { parent }),
      ),
      createElement(Entity, { id: "owned" }),
    );
  await root.render(declaration(null));
  assert.deepEqual(placements(client.applied()), [[handle(700n), null, null]]);
  assert.ok(client.entities.has("owned"));
  assert.equal(client.inspections, 1, "the bound handle is read once");
  client.calls = [];
  client.session = 2n;
  await assert.rejects(root.render(declaration(901n)), /Session replacement/);
  await root.unmount();
  assert.equal(client.calls.length, 0);
  assert.ok(client.entities.has("owned"), "no cleanup reached the World");
});

test("animation entity bindings snapshot names and handles at the committed boundary", () => {
  const entityBindings: (string | bigint)[] = ["parent", 88n];
  const description = describeAnimation(
    1,
    {
      mailbox: new AnimationMailbox(),
      source: "memory:clip",
      target: "child",
      bindings: [{ track: 0, property: { entityLink: true }, entityBindings }],
    },
    undefined,
    [
      { identity: 2, symbolicId: "child", kind: "declared", parent: undefined },
      { identity: 3, symbolicId: "parent", kind: "bound", parent: undefined },
    ],
    new Map(),
  );
  entityBindings[0] = "changed";
  assert.deepEqual(description.bindings[0]!.target, { entity: 2 });
  assert.deepEqual(description.bindings[0]!.entityBindings, [
    { entity: 3 },
    88n,
  ]);
});

test("link ordering rejects cycles and places before anchors first", () => {
  const first = {
    identity: 1,
    entity: 1,
    target: 10n,
    parent: 30n,
    before: 20n,
  };
  const second = {
    identity: 2,
    entity: 2,
    target: 20n,
    parent: 30n,
    before: null,
  };
  assert.deepEqual(orderEntityLinks([first, second]), [second, first]);
  assert.throws(
    () => orderEntityLinks([first, { ...second, before: 10n }]),
    /cycle/,
  );
  assert.throws(
    () => orderEntityLinks([first, { ...second, parent: null }]),
    /same declared parent/,
  );
});

test("separate Children groups share their enclosing entity's explicit sibling order", async () => {
  const client = new LinkBoundary();
  const root = createRoot(client, quiet);
  const groups = (order: string[]) =>
    createElement(
      Entity,
      { id: "parent" },
      order.map((id) =>
        createElement(Children, { key: id }, createElement(Entity, { id })),
      ),
    );
  try {
    await root.render(groups(["first", "second"]));
    const id = (name: string) => handle(client.entities.get(name)!);
    assert.deepEqual(placements(client.applied()), [
      [id("second"), id("parent"), null],
      [id("first"), id("parent"), id("second")],
    ]);
    client.calls = [];
    await root.render(groups(["second", "first"]));
    assert.deepEqual(
      client.calls.flat().map((operation) => operation.kind),
      ["placeEntity"],
    );
  } finally {
    await root.unmount();
  }
});

/**
 * A LinkBoundary that also applies placements the way the core does: a
 * placed entity takes a fixed position immediately before its `before`
 * sibling (or after the last one), and keeps it until placed again.
 */
class PlacingLinkBoundary extends LinkBoundary {
  readonly order = new Map<bigint | null, bigint[]>();

  override async batch(operations: Command[]): Promise<BatchOutcome> {
    const outcome = await super.batch(operations);
    for (const operation of this.resolved.at(-1)!) {
      if (operation.kind === "placeEntity")
        this.place(operation.entity, operation.placement);
      else if (operation.kind === "delete") {
        assert.ok(operation.entity.kind === "handle");
        this.unplace(operation.entity.id);
      }
    }
    return outcome;
  }

  private unplace(target: bigint): void {
    for (const siblings of this.order.values()) {
      const index = siblings.indexOf(target);
      if (index >= 0) siblings.splice(index, 1);
    }
  }

  private place(reference: EntityRef, placement: Placement): void {
    const entity = (ref: EntityRef | null) => {
      if (ref === null) return null;
      assert.ok(ref.kind === "handle");
      return ref.id;
    };
    const target = entity(reference)!;
    this.unplace(target);
    const parent = entity(placement.parent);
    const before = entity(placement.before);
    let siblings = this.order.get(parent);
    if (!siblings) this.order.set(parent, (siblings = []));
    const index = before === null ? siblings.length : siblings.indexOf(before);
    assert.ok(index >= 0, "A placement named an unplaced sibling");
    siblings.splice(index, 0, target);
  }

  /** The placed children of `parent`, by symbolic id. */
  childrenOf(parent: string): string[] {
    const names = new Map(
      [...this.entities].map(([name, entity]) => [entity, name]),
    );
    return (this.order.get(this.entities.get(parent)!) ?? []).map(
      (entity) => names.get(entity)!,
    );
  }
}

test("sibling order survives insertions, removals and reorders without re-placing unmoved siblings", async () => {
  const client = new PlacingLinkBoundary();
  const root = createRoot(client, quiet);
  // Existing entities are named by handle; new ones by their batch alias.
  const moves = () =>
    client.calls
      .flat()
      .filter(
        (operation) =>
          operation.kind === "placeEntity" &&
          operation.entity.kind === "handle",
      ).length;
  const expect = async (order: string[], moved?: number) => {
    client.calls = [];
    await root.render(scene(order));
    assert.deepEqual(client.childrenOf("parent"), order);
    if (moved !== undefined) assert.equal(moves(), moved, order.join());
  };
  try {
    await expect(["y", "x", "w", "v"], 0);
    // Moving the trailing pair ahead leaves y before x; only the pair moves,
    // and w must land before v's new place rather than keep its old position.
    await expect(["w", "v", "y", "x"], 2);
    // Removals and insertions leave the survivors' placements untouched.
    await expect(["w", "y", "x"], 0);
    await expect(["w", "n", "y", "m", "x"], 0);
    await expect(["x", "w", "n", "y", "m"], 1);
    await expect(["m", "y", "n", "w", "x"], 4);
    // Pseudo-random transitions over a small pool of keys.
    let seed = 0x2545f491;
    const random = (bound: number) => {
      seed = (Math.imul(seed, 1103515245) + 12345) >>> 0;
      return (seed >>> 8) % bound;
    };
    const pool = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
    for (let step = 0; step < 150; step++) {
      const order = pool.filter(() => random(4) !== 0);
      for (let index = order.length - 1; index > 0; index--) {
        if (random(3) !== 0) continue;
        const other = random(index + 1);
        [order[index], order[other]] = [order[other]!, order[index]!];
      }
      await expect(order);
    }
  } finally {
    await root.unmount();
  }
});

test("large sibling chains order without recursive dependency traversal", () => {
  const links = Array.from({ length: 16384 }, (_, index) => ({
    identity: index + 1,
    entity: index + 1,
    target: BigInt(index + 1),
    parent: null,
    before: index < 16383 ? BigInt(index + 2) : null,
  }));
  const ordered = orderEntityLinks(links);
  assert.equal(ordered.length, links.length);
  assert.equal(ordered[0]!.target, 16384n);
  assert.equal(ordered.at(-1)!.target, 1n);
});

test("stable keyed reparenting places the new parent before its dependant", async () => {
  const client = new LinkBoundary();
  const root = createRoot(client, quiet);
  const declarations = (parents: Record<string, string | bigint | null>) =>
    createElement(
      Fragment,
      null,
      Object.entries(parents).map(([id, parent]) =>
        createElement(
          Entity,
          { key: id, id },
          createElement(EntityLink, { parent }),
        ),
      ),
    );
  try {
    await root.render(
      declarations({ first: null, second: "first", third: "second" }),
    );
    const id = (name: string) => handle(client.entities.get(name)!);
    client.calls = [];
    await root.render(
      declarations({
        first: client.entities.get("second")!,
        second: null,
        third: "second",
      }),
    );
    assert.deepEqual(client.calls.flat(), [
      {
        kind: "placeEntity",
        entity: id("second"),
        placement: { parent: null, before: null },
      },
      {
        kind: "placeEntity",
        entity: id("first"),
        placement: { parent: id("second"), before: null },
      },
    ]);
    await root.render(
      declarations({ first: null, second: "first", third: "second" }),
    );
    client.calls = [];
    await root.render(
      declarations({
        first: client.entities.get("third")!,
        second: null,
        third: "second",
      }),
    );
    assert.deepEqual(
      client.calls
        .flat()
        .map((operation) =>
          operation.kind === "placeEntity" ? operation.entity : operation.kind,
        ),
      [id("second"), id("first")],
    );
    client.calls = [];
    await root.render(
      declarations({
        first: client.entities.get("third")!,
        second: null,
        third: "second",
      }),
    );
    assert.equal(client.calls.length, 0);
  } finally {
    await root.unmount();
  }
});

test("link dependencies include parents and siblings and diagnose parent cycles", () => {
  const child: ResolvedEntityLink = {
    identity: 1,
    entity: 1,
    target: 10n,
    parent: 30n,
    before: 20n,
  };
  const sibling: ResolvedEntityLink = {
    identity: 2,
    entity: 2,
    target: 20n,
    parent: 30n,
    before: null,
  };
  const parent: ResolvedEntityLink = {
    identity: 3,
    entity: 3,
    target: 30n,
    parent: null,
    before: null,
  };
  assert.deepEqual(orderEntityLinks([child, sibling, parent]), [
    parent,
    sibling,
    child,
  ]);
  assert.throws(
    () =>
      orderEntityLinks([
        { ...child, before: null },
        { ...parent, parent: child.target },
      ]),
    /parent or sibling.*cycle/,
  );
  assert.throws(
    () => orderEntityLinks([{ ...parent, parent: parent.target }]),
    /parent or sibling.*cycle/,
  );
  assert.deepEqual(orderEntityLinks([{ ...child, parent: 99n, before: 88n }]), [
    { ...child, parent: 99n, before: 88n },
  ]);
});

test("all four-entity forest transitions remain acyclic after each changed link", () => {
  const targets = [1n, 2n, 3n, 4n];
  const forests: (bigint | null)[][] = [];
  const acyclic = (parents: ReadonlyMap<bigint, bigint | null>): boolean => {
    for (const target of targets) {
      const path = new Set<bigint>();
      let current: bigint | null = target;
      while (current !== null) {
        if (path.has(current)) return false;
        path.add(current);
        current = parents.get(current) ?? null;
      }
    }
    return true;
  };
  const enumerate = (parents: (bigint | null)[]): void => {
    if (parents.length === targets.length) {
      if (
        acyclic(
          new Map(targets.map((target, index) => [target, parents[index]!])),
        )
      )
        forests.push(parents);
      return;
    }
    for (const parent of [null, ...targets])
      if (parent !== targets[parents.length]) enumerate([...parents, parent]);
  };
  enumerate([]);
  assert.equal(forests.length, 125);
  for (const previous of forests) {
    for (const desired of forests) {
      const current = new Map<bigint, bigint | null>(
        targets.map((target, index) => [target, previous[index]!]),
      );
      const links = targets.map((target, index) => ({
        identity: index + 1,
        entity: index + 1,
        target,
        parent: desired[index]!,
        before: null,
      }));
      for (const link of orderEntityLinks(links)) {
        if (current.get(link.target) === link.parent) continue;
        current.set(link.target, link.parent);
        assert.ok(
          acyclic(current),
          `intermediate cycle: ${previous} -> ${desired} at ${link.target}`,
        );
      }
      assert.deepEqual([...current.values()], desired);
    }
  }
});

test("large parent chains order without recursive dependency traversal", () => {
  const links = Array.from({ length: 16384 }, (_, index) => ({
    identity: index + 1,
    entity: index + 1,
    target: BigInt(index + 1),
    parent: index < 16383 ? BigInt(index + 2) : null,
    before: null,
  }));
  const ordered = orderEntityLinks(links);
  assert.equal(ordered.length, links.length);
  assert.equal(ordered[0]!.target, 16384n);
  assert.equal(ordered.at(-1)!.target, 1n);
});
