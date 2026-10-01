/** Scheduling/fault-boundary unit tests; real runtime coverage is tests/react. */
import assert from "node:assert/strict";
import test from "node:test";
import "./resources.test.js";
import "./transform.test.js";
import "./entity-links.test.js";
import "./attachment-journal.test.js";
import "./gui-input.test.js";
import "./gui-ime.test.js";
import "./gui-clipboard.test.js";
import "./gui-soft-keyboard.test.js";
import "./gui-text-bridge.test.js";
import "./gui-declaration.test.js";
import "./control-refs.test.js";
import "./canvas-world.test.js";
import {
  createElement,
  Fragment,
  StrictMode,
  useEffect,
  useState,
} from "react";
import type { Dispatch, SetStateAction } from "react";
import type {
  AnimationControllerDescription,
  AnimationControllerTransition,
  AnimationWorldClient,
  AssetResourceSnapshot,
  BatchOutcome,
  ClientAssetSource,
  Command,
  Client,
  EntityRef,
} from "@ipp/client";
import { CanvasWorldSession } from "../src/canvas-world-session.js";
import {
  ReactWorldBatchRejectedError,
  ReactWorldDuplicateEntityError,
  Animation,
  Asset,
  Children,
  createRoot,
  Entity,
  ParticleMesh,
  ParticleSprite,
  Scalar,
  Transform,
  UnlitMaterial,
  MeshInstance,
  SurfaceCache,
  UnlitTexture,
} from "../src/index.js";
import type { ReactWorldClient } from "../src/index.js";
import {
  animationMutation,
  animationSignature,
  describeAnimation,
} from "../src/animation_tree.js";
import { AnimationMailbox } from "../src/animation.js";

test("animation transitions apply only to changed driver descriptions", () => {
  const drivers = [{ source: "memory:first", track: 0, target: 1n }];
  const previous = { drivers, speed: 1, looping: false };
  const reversed = { ...previous, speed: -1 };
  const replacement = {
    ...reversed,
    drivers: [{ ...drivers[0], source: "memory:second" }],
  };
  const signature = animationSignature(previous);
  const driverSignature = animationSignature(previous.drivers);

  assert.equal(
    animationMutation(
      signature,
      driverSignature,
      animationSignature(reversed),
      animationSignature(reversed.drivers),
      true,
    ),
    "update",
  );
  assert.equal(
    animationMutation(
      animationSignature(reversed),
      animationSignature(reversed.drivers),
      animationSignature(replacement),
      animationSignature(replacement.drivers),
      true,
    ),
    "transition",
  );
  assert.equal(
    animationMutation(
      animationSignature(replacement),
      animationSignature(replacement.drivers),
      animationSignature(replacement),
      animationSignature(replacement.drivers),
      true,
    ),
    "none",
  );
});

test("animation transition seek policy is captured at the commit boundary", () => {
  const startTime = { policy: "seek" as const, time: 0.25 };
  const description = describeAnimation(
    1,
    {
      mailbox: new AnimationMailbox(),
      source: "memory:clip",
      target: 1n,
      bindings: [{ track: 0, property: { component: 1, offsets: [0] } }],
      transition: { duration: 0.5, startTime },
    },
    undefined,
    [],
    new Map(),
  );
  startTime.time = 0.75;
  assert.deepEqual(description.transition?.startTime, {
    policy: "seek",
    time: 0.25,
  });
});

/**
 * A World whose batch outcomes the test releases: it applies creations
 * (adopting a live symbolic id), component inserts and removals and deletes,
 * resolves aliases and symbols, and reports adoption like the core. The bound
 * entity "producer" is live with handle 200.
 */
class DeliveryBoundary implements ReactWorldClient {
  session = 1n;
  schemaHash = 123n;
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
    Scalar: { id: 17, fields: { value: { offset: 12, kind: 1 } } },
  };
  calls: Command[][] = [];
  pending: {
    operations: Command[];
    resolve: (outcome: BatchOutcome) => void;
  }[] = [];
  nextEntity = 100n;
  /** Live entities by symbolic id. */
  symbols = new Map<string, bigint>([["producer", 200n]]);
  /** Present components as `entity:component`. */
  present = new Set<string>();

  batch(operations: Command[]): Promise<BatchOutcome> {
    this.calls.push(operations);
    return new Promise((resolve) => {
      this.pending.push({ operations, resolve });
    });
  }

  acknowledge(): BatchOutcome {
    const pending = this.pending.shift();
    assert.ok(pending);
    const outcome = this.apply(pending.operations);
    pending.resolve(outcome);
    return outcome;
  }

  /** Refuse `operation` after applying the commands before it, like the core. */
  rejectAt(operation: number, reason = "InvalidValue"): void {
    const pending = this.pending.shift();
    assert.ok(pending);
    pending.resolve(this.apply(pending.operations, { operation, reason }));
  }

  /** Refuse the last command of the next batch. */
  reject(): void {
    const pending = this.pending[0];
    assert.ok(pending);
    this.rejectAt(pending.operations.length - 1);
  }

  /** The components present on `entity`. */
  componentsOf(entity: bigint): Set<number> {
    return new Set(
      [...this.present].flatMap((key) => {
        const [owner, component] = key.split(":");
        return BigInt(owner!) === entity ? [Number(component)] : [];
      }),
    );
  }

  /** Why the World refuses `operation`, if it does. */
  protected refusal(_operation: Command, _entity: bigint): string | undefined {
    return undefined;
  }

  private apply(
    operations: readonly Command[],
    failure?: { operation: number; reason: string },
  ): BatchOutcome {
    const aliases = new Map<number, bigint>();
    const outcome = {
      batchId: BigInt(this.calls.length),
      tick: BigInt(this.calls.length),
      aliases: [] as { alias: number; id: bigint }[],
      symbols: [] as { symbol: string; id: bigint }[],
      effects: [] as BatchOutcome["effects"],
    };
    const entity = (reference: EntityRef): bigint => {
      if (reference.kind === "handle") return reference.id;
      if (reference.kind === "alias") return aliases.get(reference.alias)!;
      const id = this.symbols.get(reference.symbol);
      assert.ok(id !== undefined, `missing ${reference.symbol}`);
      if (!outcome.symbols.some((entry) => entry.symbol === reference.symbol))
        outcome.symbols.push({ symbol: reference.symbol, id });
      return id;
    };
    const failed = (operation: number, reason: string): BatchOutcome => ({
      ...outcome,
      ok: false,
      error: { scope: "operation", operation, reason },
    });
    for (const [index, operation] of operations.entries()) {
      if (index === failure?.operation)
        return failed(failure.operation, failure.reason);
      switch (operation.kind) {
        case "create": {
          const symbolicId = operation.metadata.symbolicId;
          const existing =
            operation.adopt && symbolicId !== null
              ? this.symbols.get(symbolicId)
              : undefined;
          const id = existing ?? this.nextEntity++;
          if (symbolicId !== null) this.symbols.set(symbolicId, id);
          aliases.set(operation.alias, id);
          outcome.aliases.push({ alias: operation.alias, id });
          if (existing !== undefined)
            outcome.effects.push({ operation: index, kind: "adopted" });
          break;
        }
        case "insertComponent": {
          const id = entity(operation.entity);
          const refusal = this.refusal(operation, id);
          if (refusal) return failed(index, refusal);
          const key = `${id}:${operation.component}`;
          if (operation.adopt && this.present.has(key))
            outcome.effects.push({ operation: index, kind: "adopted" });
          this.present.add(key);
          break;
        }
        case "removeComponent":
          this.present.delete(
            `${entity(operation.entity)}:${operation.component}`,
          );
          break;
        case "delete": {
          const id = entity(operation.entity);
          for (const component of this.componentsOf(id))
            this.present.delete(`${id}:${component}`);
          for (const [symbolicId, live] of this.symbols)
            if (live === id) this.symbols.delete(symbolicId);
          break;
        }
        default:
          if ("entity" in operation) entity(operation.entity);
      }
    }
    return { ...outcome, ok: true };
  }
}

/** A World that refuses mutually exclusive particle presentations. */
class ParticleInvariantBoundary extends DeliveryBoundary {
  protected override refusal(
    operation: Command,
    entity: bigint,
  ): string | undefined {
    if (operation.kind !== "insertComponent") return undefined;
    const present = this.componentsOf(entity);
    return (operation.component === 23 && present.has(24)) ||
      (operation.component === 24 && present.has(23))
      ? "InvalidValue"
      : undefined;
  }
}

test("actual Animation JSX accepts transition declarations", async () => {
  const client = new DeliveryBoundary() as DeliveryBoundary &
    AnimationWorldClient;
  let created: AnimationControllerDescription | undefined;
  let transitioned: AnimationControllerTransition | undefined;
  client.createAnimationController = async (description) => {
    created = description;
    return 501n;
  };
  client.updateAnimationController = async () => {};
  client.transitionAnimationController = async (_id, transition) => {
    transitioned = transition;
  };
  client.deleteAnimationController = async () => {};
  client.controlAnimationController = async () => {};
  client.onPlaybackEvent = () => () => {};
  const root = createRoot(client);
  const declaration = (source: string) =>
    createElement(Animation, {
      source,
      target: 200n,
      bindings: [{ track: 0, property: { component: 17, offsets: [12] } }],
      transition: {
        duration: 0.32,
        easing: "smoothstep" as const,
        startTime: { policy: "matchPhase" as const },
      },
    });
  await settle(client, root.render(declaration("memory:clip")));
  assert.deepEqual(created, {
    drivers: [
      {
        source: "memory:clip",
        variant: 0,
        track: 0,
        target: 200n,
        property: { component: 17, offsets: [12] },
      },
    ],
    speed: 1,
    looping: false,
  });
  await settle(client, root.render(declaration("memory:replacement")));
  assert.equal(transitioned?.duration, 0.32);
  assert.equal(transitioned?.easing, "smoothstep");
  assert.deepEqual(transitioned?.startTime, { policy: "matchPhase" });
  assert.equal(
    transitioned?.description.drivers[0]?.source,
    "memory:replacement",
  );
  await settle(client, root.unmount());
});

async function turn(): Promise<void> {
  await new Promise<void>((resolve) => setImmediate(resolve));
}

async function settle(
  client: DeliveryBoundary,
  promise: Promise<void>,
): Promise<void> {
  // This is a unit-test outcome boundary, deliberately not runtime evidence.
  let done = false;
  void promise.then(
    () => {
      done = true;
    },
    () => {
      done = true;
    },
  );
  for (let attempt = 0; attempt < 30 && !done; attempt++) {
    await turn();
    if (client.pending.length) client.acknowledge();
  }
  assert.ok(done, "renderer failed to settle");
  await promise;
}

function world(value?: number, key = "scalar") {
  return createElement(
    Entity,
    { bindTo: "producer" },
    createElement(Scalar, { value, key }),
  );
}

const producer: EntityRef = { kind: "symbol", symbol: "producer" };

/** A SetField of the producer's Scalar value. */
function scalarWrite(value: number, component = 17, offset = 12): Command {
  return {
    kind: "setField",
    entity: producer,
    component,
    field: { offset, value: { kind: "f32", value } },
  };
}

/** Commands of `kind` across every call. */
function sent(client: DeliveryBoundary, kind: Command["kind"]): Command[] {
  return client.calls.flat().filter((operation) => operation.kind === kind);
}

test("the same JSX resolves against each receiving root's contract and records", async () => {
  const first = new DeliveryBoundary();
  const second = new DeliveryBoundary();
  second.session = 2n;
  second.schemaHash = 456n;
  second.components = {
    Scalar: { id: 29, fields: { value: { offset: 32, kind: 1 } } },
  };
  const firstRoot = createRoot(first);
  const secondRoot = createRoot(second);
  const shared = world(7);

  await Promise.all([
    settle(first, firstRoot.render(shared)),
    settle(second, secondRoot.render(shared)),
  ]);
  for (const [client, component, offset] of [
    [first, 17, 12],
    [second, 29, 32],
  ] as const)
    assert.deepEqual(client.calls[0], [
      {
        kind: "insertComponent",
        entity: producer,
        component,
        fields: [{ offset, value: { kind: "f32", value: 7 } }],
        adopt: true,
      },
    ]);

  await settle(first, firstRoot.unmount());
  assert.equal(first.calls.length, 1, "unmount removes nothing");
  assert.equal(
    second.calls.length,
    1,
    "closing another root must not write this root's component",
  );
  await secondRoot.render(shared);
  assert.equal(
    second.calls.length,
    1,
    "unchanged shared JSX retains its live declaration",
  );
  await settle(second, secondRoot.render(world(9)));
  assert.deepEqual(second.calls[1], [scalarWrite(9, 29, 32)]);
  await settle(second, secondRoot.unmount());
});

test("rapid React commits retain the submitted render and coalesce pending descriptions", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  const first = root.render(world(3));
  await turn();
  const superseded = Array.from({ length: 100 }, (_, index) =>
    root.render(world(index + 5)),
  );
  const third = root.render(world());
  await turn();
  assert.equal(client.calls.length, 1);
  assert.deepEqual(client.calls[0], [
    {
      kind: "insertComponent",
      entity: producer,
      component: 17,
      fields: [{ offset: 12, value: { kind: "f32", value: 3 } }],
      adopt: true,
    },
  ]);
  client.acknowledge();
  await first;
  await turn();
  await Promise.all(superseded);
  await third;
  // Only the newest description commits, and a removed prop leaves its value.
  assert.equal(client.calls.length, 1);
  await settle(client, root.unmount());
  assert.equal(client.calls.length, 1, "unmount removes nothing");
});

test("canvas render slots keep explicit queue boundaries and replace later pending work", async () => {
  const client = new DeliveryBoundary();
  Object.assign(client, {
    worldReference: { id: 1n, incarnation: 1n },
    closed: new Promise(() => {}),
  });
  const session = new CanvasWorldSession({
    client: client as unknown as Client,
    host: {
      sessions: new Map([[client.session, client]]),
    } as unknown as import("../src/canvas-presentation.js").CanvasHost,
    onError: () => {},
  });
  const root = session.createRoot();
  const first = root.render(world(1));
  await turn();
  const beforeBarrier = root.render(world(2));
  let barrierRan = false;
  const barrier = session.enqueue(async () => {
    barrierRan = true;
  });
  const afterBarrier = root.render(world(3));
  const latest = root.render(world(4));
  assert.equal(afterBarrier, latest);
  assert.equal(client.calls.length, 1);
  client.acknowledge();
  await first;
  await turn();
  const newest = root.render(world(5));
  assert.equal(newest, latest);
  assert.equal(client.calls.length, 2);
  assert.equal(barrierRan, false);
  client.acknowledge();
  await beforeBarrier;
  await barrier;
  assert.equal(barrierRan, true);
  await turn();
  assert.equal(client.calls.length, 3);
  assert.deepEqual(client.calls[2], [scalarWrite(5)]);
  client.acknowledge();
  await latest;
  await settle(client, root.unmount());
});

test("root flush seals a queued render slot before later producer coalescing", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  await settle(client, root.render(world(0)));
  const first = root.render(world(1));
  await turn();
  const included = root.render(world(2));
  const cutoff = root.flush();
  await turn();
  const later = root.render(world(3));
  assert.notEqual(included, later);
  let laterDone = false;
  void later.then(() => {
    laterDone = true;
  });
  client.acknowledge();
  await first;
  await turn();
  client.acknowledge();
  await included;
  await cutoff;
  assert.equal(laterDone, false);
  await settle(client, later);
  await settle(client, root.unmount());
});

test("rejected commit permits a corrected queued tree to rebuild", async () => {
  const errors: Error[] = [];
  const client = new DeliveryBoundary();
  const root = createRoot(client, {
    onError: (error) => {
      errors.push(error);
    },
  });
  const rejected = root.render(world(2));
  const rejection = assert.rejects(rejected, ReactWorldBatchRejectedError);
  await turn();
  const corrected = root.render(world(4));
  await turn();
  client.reject();
  await rejection;
  await turn();
  assert.equal(client.calls.length, 2);
  // The refused insertion left no record, so the correction inserts anew.
  assert.equal(client.calls[1]?.[0]?.kind, "insertComponent");
  client.acknowledge();
  await corrected;
  assert.equal(errors.length, 1);
  const count = client.calls.length;
  await root.render(world(4));
  await root.flush();
  assert.equal(client.calls.length, count);
  await settle(client, root.unmount());
});

test("pending unmount awaits its commit and sends nothing", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  const mounted = root.render(world(7));
  await turn();
  const unmounted = root.unmount();
  assert.equal(root.unmount(), unmounted);
  await turn();
  assert.equal(client.calls.length, 1);
  client.acknowledge();
  await mounted;
  await settle(client, unmounted);
  assert.equal(client.calls.length, 1, "unmount removes nothing");
});

test("hooks, effects and StrictMode produce real local commits", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  let setValue: Dispatch<SetStateAction<number>> | undefined;
  function Application() {
    const [value, set] = useState(1);
    setValue = set;
    useEffect(() => {
      set(2);
    }, []);
    return world(value);
  }
  await settle(
    client,
    root.render(createElement(StrictMode, null, createElement(Application))),
  );
  await settle(client, root.flush());
  assert.ok(setValue);
  setValue(9);
  await settle(client, root.flush());
  assert.deepEqual(sent(client, "setField").at(-1), scalarWrite(9));
  assert.equal(sent(client, "insertComponent").length, 1);
  await settle(client, root.unmount());
});

test("an unchanged rejected tree does not retry and a later field correction can recover", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  const initial = root.render(world(1));
  await settle(client, initial);
  const bad = root.render(world(Number.NaN));
  const rejected = assert.rejects(bad, ReactWorldBatchRejectedError);
  await turn();
  client.reject();
  await rejected;
  await assert.rejects(
    root.render(world(Number.NaN)),
    ReactWorldBatchRejectedError,
  );
  await assert.rejects(root.flush(), ReactWorldBatchRejectedError);
  assert.equal(client.calls.length, 2);
  // The refused write left the acknowledged value, so the correction writes.
  await settle(client, root.render(world(3)));
  assert.deepEqual(client.calls.slice(2), [[scalarWrite(3)]]);
  await settle(client, root.unmount());
});

test("a corrected render reconciles from a rejected commit's applied prefix", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  const scalars = (first: number, second: number) =>
    createElement(
      Entity,
      { bindTo: "producer" },
      createElement(Scalar, { key: "scalar", value: first }),
      createElement(Scalar, { key: "second", value: second }),
    );
  await settle(client, root.render(world(1)));
  const rejected = assert.rejects(
    root.render(scalars(2, 5)),
    ReactWorldBatchRejectedError,
  );
  await turn();
  assert.deepEqual(
    client.calls[1]?.map((operation) => operation.kind),
    ["setField", "insertComponent"],
  );
  // The write applied; the adopting insertion after it was refused.
  client.rejectAt(1);
  await rejected;
  await settle(client, root.render(scalars(2, 6)));
  assert.deepEqual(client.calls.slice(2), [
    [
      {
        kind: "insertComponent",
        entity: producer,
        component: 17,
        fields: [{ offset: 12, value: { kind: "f32", value: 6 } }],
        adopt: true,
      },
    ],
  ]);
  // The remaining declaration covers the component; only its value changes.
  await settle(client, root.render(world(3)));
  assert.deepEqual(client.calls.slice(3), [[scalarWrite(3)]]);
  await settle(client, root.unmount());
  assert.equal(client.calls.length, 4, "unmount removes nothing");
});

test("mounting adopts existing declared entities and components, writes declared values and deletes them when removed", async () => {
  const client = new DeliveryBoundary();
  // A World that still holds this root's entity from an earlier session.
  client.symbols.set("cube", 300n);
  client.present.add("300:17");
  const root = createRoot(client);
  await settle(
    client,
    root.render(
      createElement(
        Entity,
        { id: "cube" },
        createElement(Scalar, { value: 4 }),
      ),
    ),
  );
  assert.deepEqual(client.calls[0], [
    {
      kind: "create",
      alias: 1,
      metadata: { symbolicId: "cube", classes: [] },
      adopt: true,
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: 17,
      fields: [{ offset: 12, value: { kind: "f32", value: 4 } }],
      adopt: true,
    },
  ]);
  assert.equal(client.nextEntity, 100n, "nothing new was created");
  await settle(
    client,
    root.render(
      createElement(
        Entity,
        { id: "cube" },
        createElement(Scalar, { value: 5 }),
      ),
    ),
  );
  assert.deepEqual(client.calls[1], [
    {
      kind: "setField",
      entity: { kind: "handle", id: 300n },
      component: 17,
      field: { offset: 12, value: { kind: "f32", value: 5 } },
    },
  ]);
  // A removed declaration deletes its entity, adopted or created.
  await settle(client, root.render(null));
  assert.deepEqual(client.calls[2], [
    { kind: "delete", entity: { kind: "handle", id: 300n } },
  ]);
  assert.equal(client.symbols.has("cube"), false);
  await settle(client, root.unmount());
  assert.equal(client.calls.length, 3, "unmount deletes nothing");
});

test("two Entity declarations of one symbolic id in a render reject it locally", async () => {
  const client = new DeliveryBoundary();
  const errors: Error[] = [];
  const root = createRoot(client, { onError: (error) => errors.push(error) });
  const cube = (key: string, value: number) =>
    createElement(
      Entity,
      { key, id: "cube" },
      createElement(Scalar, { value }),
    );
  await settle(client, root.render(cube("a", 1)));
  const acknowledged = client.symbols.get("cube");
  const calls = client.calls.length;
  const duplicate = (error: unknown) =>
    error instanceof ReactWorldDuplicateEntityError &&
    error.symbolicId === "cube" &&
    error.message === "Duplicate Entity id: cube";

  await assert.rejects(
    root.render(createElement(Fragment, null, cube("a", 2), cube("b", 3))),
    duplicate,
  );
  // Siblings under Children would also conflict as links; the duplicate id
  // is reported first.
  await assert.rejects(
    root.render(
      createElement(
        Entity,
        { id: "list" },
        createElement(Children, null, cube("a", 4), cube("b", 5)),
      ),
    ),
    duplicate,
  );
  await turn();
  assert.equal(client.calls.length, calls, "a rejected render sends nothing");
  assert.equal(client.symbols.get("cube"), acknowledged);
  assert.ok(client.present.has(`${acknowledged}:17`));
  assert.equal(errors.filter(duplicate).length, 2);

  // `bindTo` only refers to an entity, so it may name the declared one. A
  // component the `<Entity id>` still declares stays when the reference's
  // declaration of it goes.
  const referenced = (reference: boolean) =>
    createElement(
      Fragment,
      null,
      cube("a", 6),
      reference &&
        createElement(
          Entity,
          { key: "reference", bindTo: "cube" },
          createElement(Scalar, { value: 7 }),
        ),
    );
  await settle(client, root.render(referenced(true)));
  const recreated = client.symbols.get("cube");
  assert.ok(recreated !== undefined);
  const beforeRemoval = client.calls.length;
  await settle(client, root.render(referenced(false)));
  assert.equal(client.calls.length, beforeRemoval);
  assert.ok(client.present.has(`${recreated}:17`));
  await settle(client, root.unmount());
});

test("an Entity id that moves to another node across renders keeps its entity", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  const pair = (first: string, second: string) =>
    createElement(
      Fragment,
      null,
      createElement(
        Entity,
        { key: "one", id: first },
        createElement(Scalar, { value: 1 }),
      ),
      createElement(
        Entity,
        { key: "two", id: second },
        createElement(Scalar, { value: 2 }),
      ),
    );
  await settle(client, root.render(pair("left", "right")));
  const left = client.symbols.get("left")!;
  const right = client.symbols.get("right")!;
  const calls = client.calls.length;

  // Each id moves to the other node: the records are taken over, nothing is
  // created or deleted, and each node's Scalar writes its entity in place.
  await settle(client, root.render(pair("right", "left")));
  assert.equal(client.calls.length, calls + 1);
  assert.deepEqual(client.calls[calls], [
    {
      kind: "insertComponent",
      entity: { kind: "handle", id: right },
      component: 17,
      fields: [{ offset: 12, value: { kind: "f32", value: 1 } }],
      adopt: true,
    },
    {
      kind: "insertComponent",
      entity: { kind: "handle", id: left },
      component: 17,
      fields: [{ offset: 12, value: { kind: "f32", value: 2 } }],
      adopt: true,
    },
  ]);

  // A keyed remount replaces the node that declares the id.
  await settle(
    client,
    root.render(
      createElement(
        Entity,
        { key: "remounted", id: "left" },
        createElement(Scalar, { value: 3 }),
      ),
    ),
  );
  assert.deepEqual(
    client.calls.at(-1)?.map((operation) => operation.kind),
    ["insertComponent", "delete"],
  );
  assert.equal(client.symbols.get("left"), left);
  assert.equal(client.symbols.has("right"), false);
  assert.ok(client.present.has(`${left}:17`));
  await settle(client, root.unmount());
});

test("unchanged keyed declarations send nothing when React reorders children", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  const element = (reverse: boolean) =>
    createElement(
      Entity,
      { bindTo: "producer" },
      (reverse ? ["b", "a"] : ["a", "b"]).map((key) =>
        createElement(Scalar, {
          key,
          value: key === "a" ? 1 : 2,
        }),
      ),
    );
  await settle(client, root.render(element(false)));
  assert.deepEqual(
    client.calls[0]?.map((operation) => operation.kind),
    ["insertComponent", "insertComponent"],
  );
  await root.render(element(true));
  assert.equal(client.calls.length, 1);
  await settle(client, root.unmount());
});

test("particle presentation replacement removes the mutually exclusive component first", async () => {
  const client = new ParticleInvariantBoundary();
  client.components = {
    ...client.components,
    ParticleSprite: { id: 23, fields: { r: { offset: 0, kind: 1 } } },
    ParticleMesh: { id: 24, fields: { source: { offset: 0, kind: 5 } } },
    UnlitMaterial: { id: 4, fields: { r: { offset: 0, kind: 1 } } },
  };
  const root = createRoot(client);
  const scene = (presentation: "sprites" | "meshes") =>
    createElement(
      Entity,
      { id: "particle-emitter" },
      presentation === "meshes"
        ? createElement(
            Fragment,
            null,
            createElement(ParticleMesh, { source: "memory:cube" }),
            createElement(UnlitMaterial, { r: 0.5 }),
          )
        : createElement(ParticleSprite, { r: 1 }),
    );

  await settle(client, root.render(scene("sprites")));
  const emitter = client.symbols.get("particle-emitter")!;
  await settle(client, root.render(scene("meshes")));
  assert.deepEqual(
    client.calls[1]?.map((operation) => operation.kind),
    ["removeComponent", "insertComponent", "insertComponent"],
  );
  assert.deepEqual(client.componentsOf(emitter), new Set([4, 24]));
  await settle(client, root.render(scene("sprites")));
  assert.deepEqual(
    client.calls[2]?.map((operation) => operation.kind),
    ["removeComponent", "removeComponent", "insertComponent"],
  );
  assert.deepEqual(client.componentsOf(emitter), new Set([23]));
  await settle(client, root.unmount());
});

test("unmount following a rejected initial insertion sends no cleanup", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  const mounted = root.render(world(1));
  const rejected = assert.rejects(mounted, ReactWorldBatchRejectedError);
  await turn();
  const unmounted = root.unmount();
  await turn();
  client.reject();
  await rejected;
  await unmounted;
  assert.equal(client.calls.length, 1);
});

test("local malformed trees reject render promises and allow a corrected tree", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  await assert.rejects(
    root.render(createElement(Scalar, { value: 1 })),
    /inside an Entity/,
  );
  await settle(client, root.render(world(2)));
  await settle(client, root.unmount());

  const root2 = createRoot(client, { onError: () => {} });
  await settle(client, root2.render(world(1)));
  const callsBeforeError = client.calls.length;
  // Runtime guard complements the public id xor bindTo TypeScript contract.
  await assert.rejects(
    root2.render(createElement(Entity, { id: "", children: null })),
    /nonempty id or bindTo/,
  );
  await turn();
  assert.equal(
    client.calls.length,
    callsBeforeError,
    "local error must not delete acknowledged declarations",
  );
  // Recovery deletes the declared entities the records hold and commits the
  // tree anew; the bound entity's component is adopted again in place.
  await settle(client, root2.render(world(3)));
  assert.equal(client.calls.length, callsBeforeError + 1);
  assert.equal(client.calls[callsBeforeError]?.[0]?.kind, "insertComponent");
  await settle(client, root2.unmount());
});

test("local failure queues recovery after the pending acknowledgement", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  const initial = root.render(world(1));
  await turn();
  assert.equal(client.calls.length, 1);

  const invalid = root.render(createElement(Scalar, { value: 2 }));
  const rejection = assert.rejects(invalid, /inside an Entity/);
  const corrected = root.render(world(3));
  await turn();
  assert.equal(
    client.calls.length,
    1,
    "local rejection and correction wait for the pending commit",
  );

  client.acknowledge();
  await initial;
  await rejection;
  await turn();
  // The bound component is adopted again in place, not removed first.
  assert.equal(client.calls[1]?.[0]?.kind, "insertComponent");
  client.acknowledge();
  await corrected;
  assert.equal(client.calls.length, 2);
  await settle(client, root.unmount());
});

test("local failure fences an already queued named asset reapply", async () => {
  const client = new DeliveryBoundary() as DeliveryBoundary &
    Required<
      Pick<
        ReactWorldClient,
        "registerAsset" | "releaseAsset" | "onResourceChange"
      >
    >;
  let registered: ClientAssetSource | undefined;
  let notify: ((resource: AssetResourceSnapshot) => void) | undefined;
  client.registerAsset = async (resource) => {
    registered = resource;
  };
  client.releaseAsset = async () => {};
  client.onResourceChange = (listener) => {
    notify = listener;
    return () => {};
  };
  const root = createRoot(client, { onError: () => {} });
  const scene = (value: number) =>
    createElement(
      Fragment,
      null,
      world(value),
      createElement(Asset<Uint8Array<ArrayBuffer>>, {
        id: "queued-asset",
        kind: 1,
        data: new Uint8Array([1]),
        encode: (data) => data,
      }),
    );
  await settle(client, root.render(scene(1)));
  assert.ok(registered);
  assert.ok(notify);
  const callsBeforeFailure = client.calls.length;

  notify({
    id: 1n,
    kind: registered.kind,
    source: registered.source,
    variant: registered.variant ?? 0,
    status: "loaded",
    representation: {
      decoded: true,
      graphicsReady: null,
      sourceBytes: 1n,
      residentBytes: 1n,
      graphicsBytes: null,
    },
  });
  const invalid = root.render(createElement(Scalar, { value: 2 }));
  const rejection = assert.rejects(invalid, /inside an Entity/);
  const corrected = root.render(scene(3));
  await turn();
  // Stale asset work must not write the old records before recovery: the
  // first write is the corrected tree adopting the bound component again.
  assert.equal(
    client.calls[callsBeforeFailure]?.[0]?.kind,
    "insertComponent",
    "stale asset work must not write the old records before recovery",
  );
  await rejection;
  client.acknowledge();
  await corrected;
  assert.equal(client.calls.length, callsBeforeFailure + 1);
  await settle(client, root.unmount());
});

test("definitively unsent encoding failures allow a corrected commit", async () => {
  const client = new DeliveryBoundary();
  const originalBatch = client.batch.bind(client);
  let rejectNext = false;
  client.batch = (operations) => {
    if (rejectNext) {
      rejectNext = false;
      return Promise.reject(
        Object.assign(new Error("Non-finite f32"), {
          code: "IPP_REQUEST_NOT_SENT",
        }),
      );
    }
    return originalBatch(operations);
  };
  const root = createRoot(client, { onError: () => {} });
  await settle(client, root.render(world(1)));
  rejectNext = true;
  await assert.rejects(root.render(world(Number.NaN)), /Non-finite f32/);
  assert.equal(client.calls.length, 1);
  await settle(client, root.render(world(2)));
  assert.deepEqual(client.calls[1], [scalarWrite(2)]);
  await settle(client, root.unmount());
});

test("world declarations retain typed target fields through acknowledgement, sparse update and prop removal", async () => {
  const client = new DeliveryBoundary();
  client.capabilities.spatial = true;
  // Deliberately different IDs/offsets: the tree must use the connected contract.
  client.components = {
    ...client.components,
    Transform: {
      id: 23,
      fields: { x: { offset: 20, kind: 1 }, sx: { offset: 32, kind: 1 } },
    },
    UnlitMaterial: {
      id: 24,
      fields: { r: { offset: 8, kind: 1 }, g: { offset: 16, kind: 1 } },
    },
    MeshInstance: {
      id: 25,
      fields: {
        source: { offset: 16, kind: 5 },
        variant: { offset: 28, kind: 3 },
      },
    },
  };
  const root = createRoot(client);
  const mesh = (source: string, variant?: number) =>
    createElement(
      Entity,
      { id: "cube" },
      createElement(Transform, { x: 2, sx: 0.5 }),
      createElement(UnlitMaterial, { r: 0.25, g: 0.75 }),
      createElement(MeshInstance, { source, variant }),
    );
  const first = root.render(mesh("https://example.test/é.ippm", 3));
  await turn();
  const second = root.render(mesh("ipp://mesh/cube?width=1&height=1&length=1"));
  await turn();
  assert.equal(client.calls.length, 1);
  assert.deepEqual(
    client.calls[0]!.map((op) =>
      op.kind === "insertComponent"
        ? [op.component, op.adopt, op.fields]
        : [op.kind],
    ),
    [
      ["create"],
      [
        23,
        true,
        [
          { offset: 20, value: { kind: "f32", value: 2 } },
          { offset: 32, value: { kind: "f32", value: 0.5 } },
        ],
      ],
      [
        24,
        true,
        [
          { offset: 8, value: { kind: "f32", value: 0.25 } },
          { offset: 16, value: { kind: "f32", value: 0.75 } },
        ],
      ],
      [
        25,
        true,
        [
          {
            offset: 16,
            value: { kind: "string", value: "https://example.test/é.ippm" },
          },
          { offset: 28, value: { kind: "u32", value: 3 } },
        ],
      ],
    ],
  );
  client.acknowledge();
  await first;
  await turn();
  // The changed source is written; the removed variant keeps its value.
  assert.deepEqual(client.calls[1], [
    {
      kind: "setField",
      entity: { kind: "handle", id: 100n },
      component: 25,
      field: {
        offset: 16,
        value: {
          kind: "string",
          value: "ipp://mesh/cube?width=1&height=1&length=1",
        },
      },
    },
  ]);
  client.acknowledge();
  await second;
  await root.render(mesh("ipp://mesh/cube?width=1&height=1&length=1"));
  assert.equal(
    client.calls.length,
    2,
    "equal typed values must not cause another commit",
  );
  await settle(client, root.unmount());
});

test("world components omitted from the target reject without sending declarations", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  await assert.rejects(
    root.render(
      createElement(
        Entity,
        { id: "cube" },
        createElement(MeshInstance, {
          source: "ipp://mesh/cube?width=1&height=1&length=1",
          variant: 0,
        }),
      ),
    ),
    /does not support MeshInstance/,
  );
  assert.equal(client.calls.length, 0);
  await settle(client, root.render(world(4)));
  await settle(client, root.unmount());
});

test("texture declaration preserves source and integer fields and removal removes only its component", async () => {
  const client = new DeliveryBoundary();
  client.capabilities.textures = true;
  client.components = {
    ...client.components,
    UnlitTexture: {
      id: 6,
      fields: {
        source: { offset: 0, kind: 5 },
        variant: { offset: 16, kind: 3 },
      },
    },
  };
  const root = createRoot(client);
  const world = (textured: boolean) =>
    createElement(
      Entity,
      { id: "textured" },
      createElement(Scalar, { value: 1 }),
      textured
        ? createElement(UnlitTexture, {
            source: "https://example.test/é.ippt",
            variant: 2,
          })
        : null,
    );
  await settle(client, root.render(world(true)));
  const texture = client.calls[0]!.find(
    (op) => op.kind === "insertComponent" && op.component === 6,
  );
  assert.ok(texture && texture.kind === "insertComponent");
  assert.equal(texture.adopt, true);
  assert.deepEqual(texture.fields, [
    {
      offset: 0,
      value: { kind: "string", value: "https://example.test/é.ippt" },
    },
    { offset: 16, value: { kind: "u32", value: 2 } },
  ]);
  await settle(client, root.render(world(false)));
  assert.deepEqual(client.calls[1], [
    {
      kind: "removeComponent",
      entity: { kind: "handle", id: 100n },
      component: 6,
    },
  ]);
  await settle(client, root.unmount());
});

test("feature-off public texture component rejects locally against the connected registry", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  await assert.rejects(
    root.render(
      createElement(
        Entity,
        { id: "no-texture" },
        createElement(UnlitTexture, {
          source: "ipp://mesh/cube?width=1&height=1&length=1",
        }),
      ),
    ),
    /does not support UnlitTexture/,
  );
  assert.equal(client.calls.length, 0);
  await root.unmount();
});

test("SurfaceCache opts in through the connected contract and removal returns to direct", async () => {
  const client = new DeliveryBoundary();
  // Deliberately different IDs/offsets: the tree must use the connected contract.
  client.components = {
    ...client.components,
    SurfaceCache: {
      id: 27,
      fields: {
        direct_distance: { offset: 0, kind: 1 },
        texels_per_metre: { offset: 4, kind: 1 },
        max_refresh_hz: { offset: 8, kind: 1 },
      },
    },
  };
  const root = createRoot(client);
  const world = (cache: { direct_distance?: number } | null) =>
    createElement(
      Entity,
      { id: "panel" },
      createElement(Scalar, { value: 1 }),
      cache
        ? createElement(SurfaceCache, {
            ...cache,
            texels_per_metre: 128,
          })
        : null,
    );
  await settle(client, root.render(world({ direct_distance: 2 })));
  const policy = client.calls[0]!.find(
    (op) => op.kind === "insertComponent" && op.component === 27,
  );
  assert.ok(policy && policy.kind === "insertComponent");
  assert.deepEqual(policy.fields, [
    { offset: 0, value: { kind: "f32", value: 2 } },
    { offset: 4, value: { kind: "f32", value: 128 } },
  ]);
  await settle(client, root.render(world({ direct_distance: 0 })));
  const panel = { kind: "handle" as const, id: 100n };
  assert.deepEqual(client.calls[1], [
    {
      kind: "setField",
      entity: panel,
      component: 27,
      field: { offset: 0, value: { kind: "f32", value: 0 } },
    },
  ]);
  await settle(client, root.render(world(null)));
  assert.deepEqual(client.calls[2], [
    { kind: "removeComponent", entity: panel, component: 27 },
  ]);
  await settle(client, root.unmount());

  // Targets compiled without Surfaces reject the element before sending.
  const lean = new DeliveryBoundary();
  const leanRoot = createRoot(lean, { onError: () => {} });
  await assert.rejects(
    leanRoot.render(world({ direct_distance: 2 })),
    /does not support SurfaceCache/,
  );
  assert.equal(lean.calls.length, 0);
  await leanRoot.unmount();
});
