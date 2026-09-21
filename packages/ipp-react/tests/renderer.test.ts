/** Scheduling/fault-boundary unit tests; real runtime coverage is tests/react. */
import assert from "node:assert/strict";
import test from "node:test";
import "./resources.test.js";
import "./surface_items.test.js";
import "./transform.test.js";
import "./gui-input.test.js";
import "./gui-ime.test.js";
import "./gui-clipboard.test.js";
import "./gui-soft-keyboard.test.js";
import "./gui-text-bridge.test.js";
import "./gui-controls.test.js";
import "./gui-declaration.test.js";
import "./gui-commits.test.js";
import "./gui-effects.test.js";
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
  StateOverlayLifecycleDiagnostic,
  StateOverlayAlias,
} from "@ipp/client";
import {
  ReactWorldBatchRejectedError,
  Animation,
  Asset,
  createRoot,
  Entity,
  ParticleMesh,
  ParticleSprite,
  Scalar,
  Transform,
  UnlitMaterial,
  MeshInstance,
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

class DeliveryBoundary implements ReactWorldClient {
  session = 1n;
  schemaHash = 123n;
  capabilities = {
    stateOverlays: true,
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
  listeners = new Set<(diagnostic: StateOverlayLifecycleDiagnostic) => void>();
  pending: {
    operations: Command[];
    resolve: (outcome: BatchOutcome) => void;
  }[] = [];
  nextHandle = 100n;

  batch(operations: Command[]): Promise<BatchOutcome> {
    this.calls.push(operations);
    return new Promise((resolve) => {
      this.pending.push({ operations, resolve });
    });
  }

  onDiagnostic(
    listener: (diagnostic: StateOverlayLifecycleDiagnostic) => void,
  ): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  emit(diagnostic: StateOverlayLifecycleDiagnostic): void {
    for (const listener of this.listeners) listener(diagnostic);
  }

  acknowledge(): StateOverlayAlias[] {
    const pending = this.pending.shift();
    assert.ok(pending);
    const stateOverlays: StateOverlayAlias[] = [];
    for (const operation of pending.operations) {
      if (
        operation.kind === "createStateOverlayOwner" ||
        operation.kind === "attachEntityOverlayBinding" ||
        operation.kind === "attachComponentStateOverlay"
      ) {
        stateOverlays.push({
          alias: operation.alias,
          id: this.nextHandle++,
          kind:
            operation.kind === "createStateOverlayOwner"
              ? "owner"
              : operation.kind === "attachEntityOverlayBinding"
                ? "entityOverlayBinding"
                : "componentStateOverlay",
          entity: operation.kind === "createStateOverlayOwner" ? null : 200n,
        });
      }
    }
    pending.resolve({
      ok: true,
      batchId: BigInt(this.calls.length),
      tick: BigInt(this.calls.length),
      aliases: [],
      stateOverlays,
    });
    return stateOverlays;
  }

  reject(): void {
    const pending = this.pending.shift();
    assert.ok(pending);
    pending.resolve({
      ok: false,
      aliases: [],
      stateOverlays: [],
      batchId: BigInt(this.calls.length),
      tick: BigInt(this.calls.length),
      error: { scope: "operation", operation: 2, reason: "MissingComponent" },
    });
  }
}

class ParticleInvariantBoundary extends DeliveryBoundary {
  readonly activeComponents = new Map<bigint, number>();

  override acknowledge(): StateOverlayAlias[] {
    const pending = this.pending.shift();
    assert.ok(pending);
    const stateOverlays: StateOverlayAlias[] = [];
    for (const [index, operation] of pending.operations.entries()) {
      if (operation.kind === "releaseStateOverlayOwner") {
        this.activeComponents.clear();
        continue;
      }
      if (operation.kind === "releaseComponentStateOverlay") {
        if (operation.overlay.kind === "handle")
          this.activeComponents.delete(operation.overlay.id);
        continue;
      }
      if (
        operation.kind !== "createStateOverlayOwner" &&
        operation.kind !== "attachEntityOverlayBinding" &&
        operation.kind !== "attachComponentStateOverlay"
      )
        continue;
      if (operation.kind === "attachComponentStateOverlay") {
        const conflict = [...this.activeComponents.values()].some(
          (component) =>
            (component === 23 && operation.component === 24) ||
            (component === 24 && operation.component === 23),
        );
        if (conflict) {
          pending.resolve({
            ok: false,
            aliases: [],
            stateOverlays,
            batchId: BigInt(this.calls.length),
            tick: BigInt(this.calls.length),
            error: {
              scope: "operation",
              operation: index,
              reason: "InvalidValue",
            },
          });
          return stateOverlays;
        }
      }
      const id = this.nextHandle++;
      stateOverlays.push({
        alias: operation.alias,
        id,
        kind:
          operation.kind === "createStateOverlayOwner"
            ? "owner"
            : operation.kind === "attachEntityOverlayBinding"
              ? "entityOverlayBinding"
              : "componentStateOverlay",
        entity: operation.kind === "createStateOverlayOwner" ? null : 200n,
      });
      if (operation.kind === "attachComponentStateOverlay")
        this.activeComponents.set(id, operation.component);
    }
    pending.resolve({
      ok: true,
      batchId: BigInt(this.calls.length),
      tick: BigInt(this.calls.length),
      aliases: [],
      stateOverlays,
    });
    return stateOverlays;
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

function world(value?: number, key = "scalar", bound?: boolean | null) {
  return createElement(
    Entity,
    { bindTo: "producer" },
    createElement(Scalar, { value, key, bound }),
  );
}

test("the same JSX resolves against each receiving root's contract and owner", async () => {
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
  ] as const) {
    const attach = client.calls[0]?.find(
      (operation) => operation.kind === "attachComponentStateOverlay",
    );
    assert.ok(attach);
    assert.equal(attach.component, component);
    assert.deepEqual(attach.fields, [
      { offset, value: { kind: "f32", value: 7 } },
    ]);
  }

  await settle(first, firstRoot.unmount());
  assert.equal(
    second.calls.length,
    1,
    "closing another root must not release this scope",
  );
  await secondRoot.render(shared);
  assert.equal(
    second.calls.length,
    1,
    "unchanged shared JSX retains its live declaration",
  );
  await settle(second, secondRoot.render(world(9)));
  assert.deepEqual(second.calls[1]?.[0], {
    kind: "updateComponentStateOverlay",
    owner: { kind: "handle", id: 100n },
    overlay: { kind: "handle", id: 102n },
    fields: [{ offset: 32, value: { kind: "f32", value: 9 } }],
    clear: [],
  });
  await settle(second, secondRoot.unmount());
});

test("actual React commits queue behind acknowledged handles and clear target offsets", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  const first = root.render(world(3));
  const second = root.render(world(5));
  const third = root.render(world());
  await turn();
  assert.equal(client.calls.length, 1);
  assert.equal(client.calls[0]?.length, 3);
  const attach = client.calls[0]?.[2];
  assert.equal(attach?.kind, "attachComponentStateOverlay");
  if (attach?.kind !== "attachComponentStateOverlay")
    throw new Error("missing attachment");
  assert.equal(attach.component, 17);
  assert.deepEqual(attach.fields, [
    { offset: 12, value: { kind: "f32", value: 3 } },
  ]);
  client.acknowledge();
  await first;
  await turn();
  assert.equal(client.calls.length, 2);
  assert.deepEqual(client.calls[1], [
    {
      kind: "updateComponentStateOverlay",
      owner: { kind: "handle", id: 100n },
      overlay: { kind: "handle", id: 102n },
      fields: [{ offset: 12, value: { kind: "f32", value: 5 } }],
      clear: [],
    },
  ]);
  client.acknowledge();
  await second;
  await turn();
  assert.deepEqual(client.calls[2], [
    {
      kind: "updateComponentStateOverlay",
      owner: { kind: "handle", id: 100n },
      overlay: { kind: "handle", id: 102n },
      fields: [],
      clear: [12],
    },
  ]);
  client.acknowledge();
  await third;
  await settle(client, root.unmount());
  assert.equal(client.listeners.size, 0);
});

test("rejected commit permits a corrected queued tree to rebuild", async () => {
  const errors: Error[] = [];
  const client = new DeliveryBoundary();
  const root = createRoot(client, {
    onError: (error) => {
      errors.push(error);
    },
  });
  const rejected = root.render(world(2, "scalar", true));
  const rejection = assert.rejects(rejected, ReactWorldBatchRejectedError);
  const corrected = root.render(world(4));
  await turn();
  client.reject();
  await rejection;
  await turn();
  assert.equal(client.calls.length, 2);
  assert.equal(client.calls[1]?.[0]?.kind, "createStateOverlayOwner");
  client.acknowledge();
  await corrected;
  assert.equal(errors.length, 1);
  const count = client.calls.length;
  await root.render(world(4));
  await root.flush();
  assert.equal(client.calls.length, count);
  await settle(client, root.unmount());
});

test("pending unmount awaits attachment then owner cleanup exactly once", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client);
  const mounted = root.render(world(7));
  const unmounted = root.unmount();
  assert.equal(root.unmount(), unmounted);
  await turn();
  assert.equal(client.calls.length, 1);
  client.acknowledge();
  await mounted;
  await settle(client, unmounted);
  assert.equal(
    client.calls
      .flat()
      .filter((operation) => operation.kind === "releaseStateOverlayOwner")
      .length,
    1,
  );
  assert.equal(client.listeners.size, 0);
});

test("diagnostics before promise continuation invalidate new handles without reacquiring", async () => {
  const client = new DeliveryBoundary();
  const diagnostics: StateOverlayLifecycleDiagnostic[] = [];
  const root = createRoot(client, {
    onDiagnostic: (diagnostic) => {
      diagnostics.push(diagnostic);
    },
  });
  const mounted = root.render(world(7, "scalar", true));
  const queued = root.render(world(8, "scalar", true));
  await turn();
  const stateOverlays = client.acknowledge();
  const owner = stateOverlays.find((resource) => resource.kind === "owner")?.id;
  const overlay = stateOverlays.find(
    (resource) => resource.kind === "componentStateOverlay",
  )?.id;
  assert.ok(owner !== undefined && overlay !== undefined);
  // Deliberately synchronous after resolving the outcome, before await resumes.
  client.emit({
    owner,
    stateOverlay: overlay,
    entity: 200n,
    component: 17,
    reason: "ComponentReplaced",
  });
  client.emit({
    owner: owner + 999n,
    stateOverlay: overlay,
    entity: 200n,
    component: 17,
    reason: "ComponentReplaced",
  });
  await mounted;
  await queued;
  assert.equal(diagnostics.length, 1);
  assert.equal(client.calls.length, 1);
  await root.render(world(9, "scalar", true));
  assert.equal(client.calls.length, 1);
  await settle(client, root.render(world(10, "replacement", true)));
  assert.deepEqual(
    client.calls[1]?.map((operation) => operation.kind),
    ["releaseComponentStateOverlay", "attachComponentStateOverlay"],
  );
  await settle(client, root.unmount());
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
  const updates = client.calls
    .flat()
    .filter((operation) => operation.kind === "updateComponentStateOverlay");
  assert.deepEqual(updates.at(-1)?.fields, [
    { offset: 12, value: { kind: "f32", value: 9 } },
  ]);
  assert.equal(
    client.calls
      .flat()
      .filter((operation) => operation.kind === "createStateOverlayOwner")
      .length,
    1,
  );
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
  await settle(client, root.render(world(3)));
  assert.deepEqual(client.calls[2], [
    { kind: "releaseStateOverlayOwner", owner: { kind: "handle", id: 100n } },
  ]);
  assert.equal(client.calls[3]?.[0]?.kind, "createStateOverlayOwner");
  assert.equal(client.calls[3]?.[2]?.kind, "attachComponentStateOverlay");
  await settle(client, root.unmount());
});

test("unchanged keyed declarations preserve precedence when React reorders children", async () => {
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
  assert.equal(client.calls[0]?.length, 4);
  await root.render(element(true));
  assert.equal(client.calls.length, 1);
  await settle(client, root.unmount());
});

test("particle presentation replacement releases the mutually exclusive declaration first", async () => {
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
  await settle(client, root.render(scene("meshes")));
  assert.deepEqual(
    client.calls[1]?.map((operation) => operation.kind),
    [
      "releaseComponentStateOverlay",
      "attachComponentStateOverlay",
      "attachComponentStateOverlay",
    ],
  );
  assert.deepEqual(new Set(client.activeComponents.values()), new Set([4, 24]));
  await settle(client, root.render(scene("sprites")));
  assert.deepEqual(
    client.calls[2]?.map((operation) => operation.kind),
    [
      "releaseComponentStateOverlay",
      "releaseComponentStateOverlay",
      "attachComponentStateOverlay",
    ],
  );
  assert.deepEqual(new Set(client.activeComponents.values()), new Set([23]));
  await settle(client, root.unmount());
});

test("unmount following a rejected initial attachment creates no owner cleanup", async () => {
  const client = new DeliveryBoundary();
  const root = createRoot(client, { onError: () => {} });
  const mounted = root.render(world(1));
  const rejected = assert.rejects(mounted, ReactWorldBatchRejectedError);
  const unmounted = root.unmount();
  await turn();
  client.reject();
  await rejected;
  await unmounted;
  assert.equal(client.calls.length, 1);
  assert.equal(client.listeners.size, 0);
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
  await settle(client, root2.render(world(3)));
  assert.deepEqual(client.calls[callsBeforeError], [
    { kind: "releaseStateOverlayOwner", owner: { kind: "handle", id: 103n } },
  ]);
  assert.equal(
    client.calls[callsBeforeError + 1]?.[0]?.kind,
    "createStateOverlayOwner",
  );
  await settle(client, root2.unmount());
});

test("local failure queues recovery after pending ownership acknowledgement", async () => {
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
    "local rejection and correction wait for pending ownership",
  );

  client.acknowledge();
  await initial;
  await rejection;
  await turn();
  assert.deepEqual(client.calls[1], [
    { kind: "releaseStateOverlayOwner", owner: { kind: "handle", id: 100n } },
  ]);
  client.acknowledge();
  await turn();
  assert.equal(client.calls[2]?.[0]?.kind, "createStateOverlayOwner");
  client.acknowledge();
  await corrected;
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
  assert.deepEqual(
    client.calls[callsBeforeFailure],
    [
      {
        kind: "releaseStateOverlayOwner",
        owner: { kind: "handle", id: 100n },
      },
    ],
    "stale asset work must not update the old owner before recovery",
  );
  await rejection;

  client.acknowledge();
  await turn();
  assert.equal(
    client.calls[callsBeforeFailure + 1]?.[0]?.kind,
    "createStateOverlayOwner",
  );
  client.acknowledge();
  await corrected;
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
  assert.equal(client.calls[1]?.[0]?.kind, "updateComponentStateOverlay");
  await settle(client, root.unmount());
});

test("world declarations retain typed target fields through acknowledgement, sparse update and clear", async () => {
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
      createElement(Transform, { x: 2, sx: 0.5, bound: null }),
      createElement(UnlitMaterial, { r: 0.25, g: 0.75 }),
      createElement(MeshInstance, { source, variant, bound: false }),
    );
  const first = root.render(mesh("https://example.test/é.ippm", 3));
  const second = root.render(mesh("ipp://mesh/cube?width=1&height=1&length=1"));
  await turn();
  assert.equal(client.calls.length, 1);
  const attachments = client.calls[0]!.filter(
    (op) => op.kind === "attachComponentStateOverlay",
  );
  assert.deepEqual(
    attachments.map((op) => [op.component, op.mode, op.fields]),
    [
      [
        23,
        "auto",
        [
          { offset: 20, value: { kind: "f32", value: 2 } },
          { offset: 32, value: { kind: "f32", value: 0.5 } },
        ],
      ],
      [
        24,
        "auto",
        [
          { offset: 8, value: { kind: "f32", value: 0.25 } },
          { offset: 16, value: { kind: "f32", value: 0.75 } },
        ],
      ],
      [
        25,
        "owned",
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
  assert.deepEqual(client.calls[1], [
    {
      kind: "updateComponentStateOverlay",
      owner: { kind: "handle", id: 100n },
      overlay: { kind: "handle", id: 104n },
      fields: [
        {
          offset: 16,
          value: {
            kind: "string",
            value: "ipp://mesh/cube?width=1&height=1&length=1",
          },
        },
      ],
      clear: [28],
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

test("texture declaration preserves source and integer fields and removal releases only its overlay", async () => {
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
    (op) => op.kind === "attachComponentStateOverlay" && op.component === 6,
  );
  assert.ok(texture && texture.kind === "attachComponentStateOverlay");
  assert.equal(texture.mode, "auto");
  assert.deepEqual(texture.fields, [
    {
      offset: 0,
      value: { kind: "string", value: "https://example.test/é.ippt" },
    },
    { offset: 16, value: { kind: "u32", value: 2 } },
  ]);
  await settle(client, root.render(world(false)));
  assert.equal(client.calls[1]!.length, 1);
  assert.equal(client.calls[1]![0]!.kind, "releaseComponentStateOverlay");
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
