/**
 * A commit describes, compares and writes what it changed. Counts come from
 * the real reconciler: `commitCounters` totals the declarations each describe
 * visited and the components each commit compared, and the client records
 * every batch.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement, Fragment, memo, useState } from "react";
import type { BatchOutcome, Command } from "@ipp/client";
import { Entity, Scalar, Transform, createRoot } from "../src/index.js";
import type { ReactWorldClient } from "../src/index.js";
import { commitCounters } from "../src/reconciler/tree.js";

const SCALAR = 17;
const TRANSFORM = 3;

/**
 * A World that applies each batch at once, or, while `holding`, when the test
 * releases it; `refuse` rejects the next batch's first command.
 */
class CommitWorld implements ReactWorldClient {
  session = 1n;
  schemaHash = 1n;
  components: ReactWorldClient["components"] = {
    Scalar: { id: SCALAR, fields: { value: { offset: 12, kind: 1 } } },
    Transform: {
      id: TRANSFORM,
      fields: {
        x: { offset: 0, kind: 1 },
        y: { offset: 4, kind: 1 },
        z: { offset: 8, kind: 1 },
      },
    },
  };
  calls: Command[][] = [];
  /** The entity each create made, by symbolic id. */
  entities = new Map<string, bigint>();
  holding = false;
  refuse = false;
  held: (() => void)[] = [];
  private nextEntity = 100n;

  batch(operations: Command[]): Promise<BatchOutcome> {
    this.calls.push(operations);
    const base = {
      batchId: BigInt(this.calls.length),
      tick: BigInt(this.calls.length),
      symbols: [],
      effects: [],
    };
    let outcome: BatchOutcome;
    if (this.refuse) {
      this.refuse = false;
      outcome = {
        ...base,
        aliases: [],
        ok: false,
        error: { scope: "operation", operation: 0, reason: "InvalidValue" },
      };
    } else
      outcome = {
        ...base,
        ok: true,
        aliases: operations.flatMap((operation) => {
          if (operation.kind !== "create") return [];
          const id = this.nextEntity++;
          this.entities.set(operation.metadata.symbolicId!, id);
          return [{ alias: operation.alias, id }];
        }),
      };
    if (!this.holding) return Promise.resolve(outcome);
    return new Promise((resolve) => this.held.push(() => resolve(outcome)));
  }

  release(): void {
    const next = this.held.shift();
    assert.ok(next, "no held batch");
    next();
  }
}

const staticEntities = (count: number, prefix = "static") =>
  Array.from({ length: count }, (_, index) =>
    createElement(
      Entity,
      { key: `${prefix}-${index}`, id: `${prefix}-${index}` },
      createElement(Scalar, { value: index }),
    ),
  );

/** Static declarations React does not render again while their count holds. */
const StaticSubtree = memo(function StaticSubtree({
  count,
}: {
  count: number;
}) {
  return createElement(Fragment, null, staticEntities(count));
});

/** Wait until the World has received `count` batches. */
async function sent(world: CommitWorld, count: number): Promise<void> {
  while (world.calls.length < count)
    await new Promise((done) => setTimeout(done));
}

/** The entity the World created for `symbolicId`. */
function created(world: CommitWorld, symbolicId: string): bigint {
  const entity = world.entities.get(symbolicId);
  assert.ok(entity !== undefined, `${symbolicId} was not created`);
  return entity;
}

const setValue = (entity: bigint, value: number): Command => ({
  kind: "setField",
  entity: { kind: "handle", id: entity },
  component: SCALAR,
  field: { offset: 12, value: { kind: "f32", value } },
});

/** The counters' growth across `work`. */
async function counted(work: () => Promise<void>) {
  const { described, compared } = commitCounters;
  await work();
  return {
    described: commitCounters.described - described,
    compared: commitCounters.compared - compared,
  };
}

test("a one-field change in a large tree describes its declarations and sends one write", async () => {
  const world = new CommitWorld();
  const root = createRoot(world, { onError: (error) => assert.fail(error) });
  const scene = (value: number) =>
    createElement(
      Fragment,
      null,
      createElement(StaticSubtree, { count: 1000 }),
      createElement(Entity, { id: "leaf" }, createElement(Scalar, { value })),
    );
  try {
    const mount = await counted(() => root.render(scene(1)));
    assert.equal(world.calls.length, 1);
    assert.ok(mount.described >= 2002, "mounting describes every declaration");
    // React renders the leaf's Entity and Scalar again, and nothing else; the
    // commit compares the one changed component.
    assert.deepEqual(await counted(() => root.render(scene(2))), {
      described: 2,
      compared: 1,
    });
    assert.equal(world.calls.length, 2);
    assert.deepEqual(world.calls[1], [setValue(created(world, "leaf"), 2)]);
  } finally {
    await root.unmount();
  }
});

test("a state change in one component describes nothing in an unrelated sibling subtree", async () => {
  const world = new CommitWorld();
  const root = createRoot(world, { onError: (error) => assert.fail(error) });
  let setLeaf: ((value: number) => void) | undefined;
  function Leaf() {
    const [value, set] = useState(1);
    setLeaf = set;
    return createElement(
      Entity,
      { id: "leaf" },
      createElement(Scalar, { value }),
    );
  }
  // The siblings are not memoized: React leaves them alone because only the
  // leaf's state changed.
  function Scene({ count }: { count: number }) {
    return createElement(
      Fragment,
      null,
      staticEntities(count),
      createElement(Leaf),
      staticEntities(count, "after"),
    );
  }
  try {
    await root.render(createElement(Scene, { count: 500 }));
    const leaf = created(world, "leaf");
    for (const value of [2, 3]) {
      assert.deepEqual(
        await counted(async () => {
          setLeaf!(value);
          await root.flush();
        }),
        { described: 2, compared: 1 },
      );
      assert.deepEqual(world.calls.at(-1), [setValue(leaf, value)]);
    }
    assert.equal(world.calls.length, 3);
  } finally {
    await root.unmount();
  }
});

test("changed fields of one component stay one combined write; renders that change nothing send nothing", async () => {
  const world = new CommitWorld();
  const root = createRoot(world, { onError: (error) => assert.fail(error) });
  const scene = (x: number, y: number) =>
    createElement(
      Fragment,
      null,
      createElement(StaticSubtree, { count: 100 }),
      createElement(
        Entity,
        { id: "moved" },
        createElement(Transform, { x, y, z: 0 }),
      ),
    );
  try {
    await root.render(scene(0, 0));
    const moved = created(world, "moved");
    await root.render(scene(1, 2));
    assert.deepEqual(world.calls.at(-1), [
      {
        kind: "insertComponent",
        entity: { kind: "handle", id: moved },
        component: TRANSFORM,
        fields: [
          { offset: 0, value: { kind: "f32", value: 1 } },
          { offset: 4, value: { kind: "f32", value: 2 } },
        ],
        adopt: true,
      },
    ]);
    const submitted = world.calls.length;
    // Equal values change no declaration: nothing is compared or written.
    assert.deepEqual(await counted(() => root.render(scene(1, 2))), {
      described: 2,
      compared: 0,
    });
    assert.equal(world.calls.length, submitted);
  } finally {
    await root.unmount();
  }
});

test("changes committed while a batch is outstanding go out together, in tree order", async () => {
  const world = new CommitWorld();
  const root = createRoot(world, { onError: (error) => assert.fail(error) });
  const scene = (values: readonly number[]) =>
    createElement(
      Fragment,
      null,
      values.map((value, index) =>
        createElement(
          Entity,
          { key: index, id: `item-${index}` },
          createElement(Scalar, { value }),
        ),
      ),
    );
  try {
    await root.render(scene([0, 0, 0]));
    const items = [0, 1, 2].map((index) => created(world, `item-${index}`));
    world.holding = true;
    const first = root.render(scene([1, 0, 0]));
    await sent(world, 2);
    // While the first batch is outstanding the next two renders replace each
    // other, and their changes go out in one batch.
    void root.render(scene([1, 0, 2]));
    const last = root.render(scene([1, 1, 2]));
    world.release();
    await first;
    await sent(world, 3);
    world.release();
    await last;
    assert.deepEqual(world.calls.slice(1), [
      [setValue(items[0]!, 1)],
      [setValue(items[1]!, 1), setValue(items[2]!, 2)],
    ]);
  } finally {
    world.holding = false;
    await root.unmount();
  }
});

test("after a refused write the next commit compares the whole tree and writes what is still unwritten", async () => {
  const world = new CommitWorld();
  const errors: Error[] = [];
  const root = createRoot(world, { onError: (error) => errors.push(error) });
  const scene = (first: number, second: number) =>
    createElement(
      Fragment,
      null,
      createElement(
        Entity,
        { id: "first" },
        createElement(Scalar, { value: first }),
      ),
      createElement(
        Entity,
        { id: "second" },
        createElement(Scalar, { value: second }),
      ),
    );
  try {
    await root.render(scene(0, 0));
    const [first, second] = [created(world, "first"), created(world, "second")];
    world.refuse = true;
    await assert.rejects(root.render(scene(1, 0)), /rejected/);
    await root.render(scene(1, 2));
    assert.deepEqual(world.calls.at(-1), [
      setValue(first, 1),
      setValue(second, 2),
    ]);
  } finally {
    await root.unmount();
  }
});

test("adding and removing a declaration after changes-only commits creates and deletes only it", async () => {
  const world = new CommitWorld();
  const root = createRoot(world, { onError: (error) => assert.fail(error) });
  const scene = (value: number, extra: boolean) =>
    createElement(
      Fragment,
      null,
      createElement(StaticSubtree, { count: 10 }),
      createElement(Entity, { id: "leaf" }, createElement(Scalar, { value })),
      extra
        ? createElement(
            Entity,
            { id: "extra" },
            createElement(Scalar, { value: 7 }),
          )
        : null,
    );
  try {
    await root.render(scene(0, false));
    await root.render(scene(1, false));
    await root.render(scene(1, true));
    assert.deepEqual(
      world.calls.at(-1)!.map((command) => command.kind),
      ["create", "insertComponent"],
    );
    const extra = created(world, "extra");
    await root.render(scene(2, true));
    assert.deepEqual(world.calls.at(-1), [setValue(created(world, "leaf"), 2)]);
    await root.render(scene(2, false));
    assert.deepEqual(world.calls.at(-1), [
      { kind: "delete", entity: { kind: "handle", id: extra } },
    ]);
  } finally {
    await root.unmount();
  }
});
