import assert from "node:assert/strict";
import test from "node:test";
import { isLifecycleWatchRemoveError } from "@ipp/client";
import type { LifecycleWatchRemoveError } from "@ipp/client";
import * as publicClient from "@ipp/client";
import {
  LifecycleWatches,
  LifecycleWatchStartError,
} from "../src/lifecycle-watches.js";
import type { Response } from "../src/types.js";
import type {
  LifecycleFieldValue,
  LifecycleTargetSelection,
  LifecycleWatchEvent,
  LifecycleWatchRequest,
  LifecycleWatchRecord,
} from "../src/lifecycle-types.js";

const world = { id: 3n, incarnation: 4n };
const selection: LifecycleTargetSelection = {
  target: { kind: "component", entity: 10n, component: 2 },
  kinds: 96,
};

function fixture() {
  let next = 1n;
  let generation = 0n;
  const requests: {
    id: bigint;
    control: LifecycleWatchRequest;
    resolve(response: Response): void;
    reject(error: Error): void;
  }[] = [];
  const failures: Error[] = [];
  /** The target each acknowledged member was added with, by generation. */
  const targets = new Map<bigint, LifecycleTargetSelection["target"]>();
  const manager = new LifecycleWatches({
    nextId: () => next++,
    send: (id, control) =>
      new Promise((resolve, reject) => {
        requests.push({ id, control, resolve, reject });
      }),
    definitelyUnapplied: (error) => error instanceof Unsent,
    fail: (error) => {
      failures.push(error);
      manager.stop(error);
    },
  });
  const deliver = (response: Response) => {
    try {
      const consumed = manager.receive(response);
      if (!consumed && response.requestId !== 0n) {
        const pending = requests.find(
          (request) => request.id === response.requestId,
        )!;
        if (response.body.kind === "error")
          pending.reject(new Unsent(response.body.message));
        else pending.resolve(response);
      }
    } catch (error) {
      const reason = error as Error;
      failures.push(reason);
      manager.stop(reason);
      for (const pending of requests) pending.reject(reason);
    }
  };
  const record = (body: LifecycleWatchRecord, requestId = 0n): Response => ({
    session: 5n,
    requestId,
    tick: 0n,
    body: { kind: "lifecycleWatch", record: body },
  });
  // A removal ACK echoes each removed member's target.
  const ack = (index: number) => {
    const request = requests[index]!;
    const control = request.control;
    const members =
      control.kind === "add"
        ? control.targets.map(({ target }) => {
            targets.set(++generation, target);
            return { generation, target };
          })
        : control.generations.map((generation) => ({
            generation,
            target: targets.get(generation) ?? selection.target,
          }));
    return record(
      {
        kind: "ack",
        action: control.kind,
        world,
        output: 7n,
        cut: { sequence: 10n, tick: 90n },
        result: {
          kind: "applied",
          baselines: members.map(({ generation, target }) => ({
            member: { output: 7n, generation },
            target,
            lifetime:
              control.kind === "add"
                ? { kind: "component", entityLive: true, incarnation: 12n }
                : { kind: "removed" },
          })),
        },
      },
      request.id,
    );
  };
  const event = (member: bigint, sequence = 11n) =>
    record({
      kind: "event",
      world,
      output: 7n,
      member: { output: 7n, generation: member },
      sequence,
      tick: 91n,
      observation: {
        kind: "component",
        entity: 10n,
        component: 2,
        change: "replaced",
        previousIncarnation: 12n,
        incarnation: 13n,
      },
    });
  return { manager, requests, failures, deliver, ack, event, record };
}

class Unsent extends Error {}

/** The sequence of a lifecycle event; these watches have no value members. */
function sequenceOf(event: LifecycleWatchEvent): bigint {
  if (event.kind !== "event") throw new Error("Unexpected value record");
  return event.sequence;
}

test("removing one group never traverses unrelated active registrations", async () => {
  const fixtureState = fixture();
  const watches = [];
  for (let index = 0; index < 130; index++) {
    const pending = fixtureState.manager.watch(world, [selection], () => {}, 1);
    fixtureState.deliver(fixtureState.ack(index));
    watches.push(await pending);
  }
  const active = Reflect.get(fixtureState.manager, "active") as Map<
    bigint,
    unknown
  >;
  let visits = 0;
  const iterate = active[Symbol.iterator].bind(active);
  active[Symbol.iterator] = function* () {
    for (const entry of iterate()) {
      visits++;
      yield entry;
    }
    return undefined;
  };
  const removing = watches[65]!.remove();
  assert.deepEqual(fixtureState.requests[130]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [66n],
  });
  fixtureState.deliver(fixtureState.ack(130));
  await removing;
  assert.equal(active.size, 129);
  assert.equal(visits, 0);
  assert.equal(fixtureState.failures.length, 0);
});

test("retrying a partially removed watch retains every acknowledged removal cut", async () => {
  const fixtureState = fixture();
  const started = fixtureState.manager.watch(
    world,
    [selection, selection],
    () => {},
    1,
  );
  fixtureState.deliver(fixtureState.ack(0));
  await Promise.resolve();
  await Promise.resolve();
  fixtureState.deliver(fixtureState.ack(1));
  const watch = await started;
  const active = Reflect.get(fixtureState.manager, "active") as Map<
    bigint,
    unknown
  >;
  active[Symbol.iterator] = () => {
    throw new Error("Removal must not traverse unrelated groups");
  };
  const removing = watch.remove();
  const failure = assert.rejects(removing, /not submitted/);
  fixtureState.deliver(fixtureState.ack(2));
  await Promise.resolve();
  await Promise.resolve();
  fixtureState.requests[3]!.reject(new Unsent("not submitted"));
  await failure;
  const retried = watch.remove();
  assert.deepEqual(fixtureState.requests[4]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [2n],
  });
  fixtureState.deliver(fixtureState.ack(4));
  assert.deepEqual(
    (await retried).map((cut) => cut.request),
    [3n, 5n],
  );
  assert.equal(fixtureState.failures.length, 0);
});

test("a disposed consumer fences handed-off prefix callbacks before awaiting removal", async () => {
  const fixtureState = fixture();
  let disposed = false;
  const observed: bigint[] = [];
  const pending = fixtureState.manager.watch(
    world,
    [selection],
    (event) => {
      if (!disposed) observed.push(sequenceOf(event));
    },
    1,
  );
  fixtureState.deliver(fixtureState.ack(0));
  const watch = await pending;
  fixtureState.deliver(fixtureState.event(1n, 11n));
  disposed = true;
  const removed = watch.remove();
  fixtureState.deliver(fixtureState.event(1n, 12n));
  fixtureState.deliver(fixtureState.ack(1));
  await removed;
  assert.deepEqual(observed, [11n]);
  assert.equal(fixtureState.failures.length, 0);
});

test("same-stack ACK activates before promise continuation and remove cuts future callbacks", async () => {
  const fixtureState = fixture();
  const observed: bigint[] = [];
  const pending = fixtureState.manager.watch(
    world,
    [selection],
    (event) => observed.push(sequenceOf(event)),
    2,
  );
  fixtureState.deliver(fixtureState.ack(0));
  fixtureState.deliver(fixtureState.event(1n));
  assert.deepEqual(observed, [11n]);
  const watch = await pending;
  assert.equal(watch.baselines[0]!.member.generation, 1n);
  assert.equal(watch.cuts[0]!.tick, 90n);
  const removed = watch.remove();
  fixtureState.deliver(fixtureState.event(1n, 15n));
  fixtureState.deliver(fixtureState.ack(1));
  await removed;
  assert.deepEqual(observed, [11n, 15n]);
  assert.equal((await watch.closed).kind, "removed");
  assert.deepEqual(await watch.remove(), await removed);
  assert.equal(fixtureState.failures.length, 0);
});

test("independent users of one target remove only their exact generations", async () => {
  const fixtureState = fixture();
  const observed: bigint[] = [];
  const first = fixtureState.manager.watch(world, [selection], () => {}, 2);
  fixtureState.deliver(fixtureState.ack(0));
  const firstWatch = await first;
  const second = fixtureState.manager.watch(
    world,
    [selection],
    (event) => observed.push(event.member.generation),
    2,
  );
  fixtureState.deliver(fixtureState.ack(1));
  await second;
  const remove = firstWatch.remove();
  fixtureState.deliver(fixtureState.ack(2));
  await remove;
  fixtureState.deliver(fixtureState.event(2n));
  assert.deepEqual(observed, [2n]);
  const readd = fixtureState.manager.watch(
    world,
    [selection],
    (event) => observed.push(event.member.generation),
    2,
  );
  fixtureState.deliver(fixtureState.ack(3));
  assert.equal((await readd).baselines[0]!.member.generation, 3n);
});

for (const action of ["add", "remove"] as const) {
  for (const code of [1, 3])
    test(`${action} generic error ${code} is synchronously classified`, async () => {
      const fixtureState = fixture();
      const observed: bigint[] = [];
      const initial = fixtureState.manager.watch(
        world,
        [selection],
        (event) => observed.push(sequenceOf(event)),
        2,
      );
      fixtureState.deliver(fixtureState.ack(0));
      const watch = await initial;
      const pending =
        action === "remove"
          ? watch.remove()
          : fixtureState.manager.watch(world, [selection], () => {}, 2);
      const rejected = assert.rejects(pending);
      fixtureState.deliver({
        session: 5n,
        requestId: fixtureState.requests[1]!.id,
        tick: 0n,
        body: { kind: "error", code, message: "control failure" },
      });
      fixtureState.deliver(fixtureState.event(1n));
      await rejected;
      assert.deepEqual(observed, code === 1 ? [11n] : []);
      assert.equal(fixtureState.failures.length, code === 1 ? 0 : 1);
    });
}

test("a host-refused watch fails with the host's reason and keeps the connection", async () => {
  const state = fixture();
  const reason =
    "Lifecycle watches are unsupported: this World does not select ipp.lifecycle-publisher";
  const failure = state.manager
    .watch(world, [selection], () => assert.fail("refused watch callback"), 1)
    .catch((error: unknown) => error);
  state.deliver({
    session: 5n,
    requestId: state.requests[0]!.id,
    tick: 0n,
    body: { kind: "error", code: 1, message: reason },
  });
  const error = await failure;
  assert.ok(error instanceof LifecycleWatchStartError);
  assert.equal(error.message, reason);
  assert.ok(error.cause instanceof Unsent);
  assert.equal(error.partial.baselines.length, 0);
  assert.equal((await error.partial.closed).kind, "closed");
  assert.deepEqual(state.failures, []);

  const retried = state.manager.watch(world, [selection], () => {}, 1);
  state.deliver(state.ack(1));
  assert.equal((await retried).baselines.length, 1);
});

test("paged start failure exposes only acknowledged members and disables partial callbacks", async () => {
  const fixtureState = fixture();
  const pending = fixtureState.manager.watch(
    world,
    [selection, selection, selection],
    () => assert.fail("failed start callback"),
    2,
  );
  const failure = pending.catch((error: unknown) => error);
  fixtureState.deliver(fixtureState.ack(0));
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(fixtureState.requests.length, 2);
  fixtureState.requests[1]!.reject(new Unsent("not submitted"));
  const error = await failure;
  assert.ok(error instanceof LifecycleWatchStartError);
  assert.equal(error.partial.baselines.length, 2);
  fixtureState.deliver(fixtureState.event(1n));
  const remove = error.partial.remove();
  assert.deepEqual(fixtureState.requests[2]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [1n, 2n],
  });
  fixtureState.deliver(fixtureState.ack(2));
  await remove;
});

test("reentrant close fences later observations without replay", async () => {
  const fixtureState = fixture();
  const observed: bigint[] = [];
  const pending = fixtureState.manager.watch(
    world,
    [selection],
    (event) => {
      observed.push(sequenceOf(event));
      fixtureState.manager.stop(new Error("closed in listener"));
    },
    2,
  );
  fixtureState.deliver(fixtureState.ack(0));
  const watch = await pending;
  fixtureState.deliver(fixtureState.event(1n));
  fixtureState.deliver(fixtureState.event(1n, 12n));
  assert.deepEqual(observed, [11n]);
  assert.equal((await watch.closed).kind, "closed");
  await assert.rejects(
    fixtureState.manager.watch(world, [selection], () => {}, 2),
    /closed in listener/,
  );
});

async function groupFixture(count: number, pageSize = 260) {
  const state = fixture();
  const observed: bigint[] = [];
  const starting = state.manager.watch(
    world,
    Array.from({ length: count }, () => selection),
    (event) => observed.push(event.member.generation),
    pageSize,
  );
  for (let page = 0; page < Math.ceil(count / pageSize); page++) {
    state.deliver(state.ack(page));
    await new Promise<void>((resolve) => setImmediate(resolve));
  }
  return { ...state, observed, watch: await starting };
}

test("subset validation finishes before any send, including a foreign last member", async () => {
  const state = await groupFixture(260);
  const ids = state.watch.baselines.map((baseline) => baseline.member);
  for (const invalid of [
    { output: 8n, generation: 1n },
    { output: 7n, generation: 261n },
    { output: 7n, generation: 0n },
  ]) {
    await assert.rejects(
      state.watch.removeMembers([...ids, invalid]),
      /belong/,
    );
    assert.equal(state.requests.length, 1);
  }
  const other = state.manager.watch(world, [selection], () => {}, 260);
  state.deliver(state.ack(1));
  const foreign = await other;
  await assert.rejects(
    state.watch.removeMembers([ids[0]!, foreign.baselines[0]!.member]),
    /belong/,
  );
  assert.equal(state.requests.length, 2);
  assert.deepEqual(await state.watch.removeMembers([]), []);
  assert.equal(state.failures.length, 0);
});

test("singleton removal indexes the original group without scanning group or global membership", async () => {
  const state = await groupFixture(260);
  const active = Reflect.get(state.manager, "active") as Map<bigint, unknown>;
  const group = [...Reflect.get(state.manager, "groups")][0];
  for (const collection of [
    active,
    group.members,
    group.activeGenerations,
    group.baselines,
  ]) {
    collection[Symbol.iterator] = () => {
      throw new Error("Unrelated member iteration");
    };
    collection.values = () => {
      throw new Error("Unrelated member values");
    };
  }
  let lookups = 0;
  const get = group.members.get.bind(group.members);
  group.members.get = (generation: bigint) => {
    lookups++;
    return get(generation);
  };
  const id = state.watch.baselines[129]!.member;
  const removing = state.watch.removeMembers([id]);
  assert.deepEqual(state.requests[1]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [130n],
  });
  state.deliver(state.event(1n));
  state.deliver(state.ack(1));
  await removing;
  assert.equal(lookups, 2);
  assert.equal(active.size, 259);
  assert.deepEqual(state.observed, [1n]);
});

test("overlapping subsets share pages and duplicates retain original ACK cuts", async () => {
  const state = await groupFixture(4);
  const ids = state.watch.baselines.map((baseline) => baseline.member);
  const first = state.watch.removeMembers([ids[1]!, ids[0]!, ids[1]!]);
  const overlap = state.watch.removeMembers([ids[1]!, ids[2]!]);
  const duplicate = state.watch.removeMembers([ids[0]!]);
  assert.equal(state.requests.length, 3);
  assert.deepEqual(state.requests[1]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [1n, 2n],
  });
  assert.deepEqual(state.requests[2]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [3n],
  });
  state.deliver(state.event(1n));
  state.deliver(state.ack(1));
  state.deliver(state.event(4n));
  state.deliver(state.ack(2));
  assert.deepEqual(
    (await first).map((cut) => cut.request),
    [2n],
  );
  assert.deepEqual(
    (await duplicate).map((cut) => cut.request),
    [2n],
  );
  assert.deepEqual(
    (await overlap).map((cut) => cut.request),
    [2n, 3n],
  );
  assert.deepEqual(await state.watch.removeMembers([ids[0]!]), await first);
  assert.equal(state.requests.length, 3);
  const all = state.watch.remove();
  assert.deepEqual(state.requests[3]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [4n],
  });
  state.deliver(state.ack(3));
  assert.deepEqual(
    (await all).map((cut) => cut.request),
    [2n, 3n, 4n],
  );
  assert.deepEqual(await state.watch.remove(), await all);
  assert.equal((await state.watch.closed).kind, "removed");
  assert.deepEqual(state.observed, [1n, 4n]);
});

test("partial subset failure exposes only confirmed cuts and retries only unconfirmed generations", async () => {
  const state = await groupFixture(5, 2);
  const ids = state.watch.baselines.map((baseline) => baseline.member);
  const removing = state.watch.removeMembers(ids.slice(0, 4));
  const failure = removing.catch((error: unknown) => error);
  const joined = state.watch
    .removeMembers([ids[2]!])
    .catch((error: unknown) => error);
  state.deliver(state.ack(3));
  await new Promise<void>((resolve) => setImmediate(resolve));
  const unsent = new Unsent("not submitted");
  state.requests[4]!.reject(unsent);
  const error = await failure;
  assert.ok(isLifecycleWatchRemoveError(error));
  assert.equal(error.cause, unsent);
  assert.deepEqual(
    error.cuts.map((cut) => cut.request),
    [4n],
  );
  assert.deepEqual(error.unconfirmedMembers, ids.slice(2, 4));
  assert.ok(
    Object.isFrozen(error.cuts) && Object.isFrozen(error.unconfirmedMembers),
  );
  assert.ok(isLifecycleWatchRemoveError(await joined));
  state.deliver(state.event(5n));
  const retry = state.watch.removeMembers(ids.slice(0, 4));
  assert.deepEqual(state.requests[5]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [3n, 4n],
  });
  state.deliver(state.ack(5));
  assert.deepEqual(
    (await retry).map((cut) => cut.request),
    [4n, 6n],
  );
  assert.deepEqual(state.observed, [5n]);
  assert.equal(state.failures.length, 0);
});

test("public removal error guard narrows owned information without exposing a constructor", async () => {
  const state = await groupFixture(3, 1);
  const failure = state.watch
    .removeMembers(state.watch.baselines.map((baseline) => baseline.member))
    .catch((error: unknown) => error);
  state.deliver(state.ack(3));
  await new Promise<void>((resolve) => setImmediate(resolve));
  const cause = new Unsent("page not submitted");
  state.requests[4]!.reject(cause);
  const value = await failure;
  assert.ok(isLifecycleWatchRemoveError(value));
  const error: LifecycleWatchRemoveError = value;
  assert.equal(Object.hasOwn(publicClient, "LifecycleWatchRemoveError"), false);
  assert.equal(error, value);
  assert.equal(error.cause, cause);
  assert.deepEqual(
    error.cuts.map((cut) => cut.request),
    [4n],
  );
  assert.deepEqual(
    error.unconfirmedMembers.map((member) => member.generation),
    [2n, 3n],
  );
  const shape = {
    name: error.name,
    message: error.message,
    cause: error.cause,
    cuts: error.cuts,
    unconfirmedMembers: error.unconfirmedMembers,
  };
  assert.ok(isLifecycleWatchRemoveError(shape));
  assert.equal(shape.cuts, error.cuts);
  assert.equal(shape.unconfirmedMembers, error.unconfirmedMembers);
  assert.ok(isLifecycleWatchRemoveError({ ...shape, cause: undefined }));
  assert.ok(isLifecycleWatchRemoveError({ ...shape, cause: "local failure" }));

  const malformed: unknown[] = [
    null,
    undefined,
    false,
    1,
    1n,
    "LifecycleWatchRemoveError",
    new Error("unrelated"),
    Object.assign(new Error("name only"), { name: error.name }),
    { ...shape, name: "OtherError" },
    { ...shape, message: 1 },
    { ...shape, stack: 1 },
    { ...shape, cuts: null },
    { ...shape, cuts: [null] },
    { ...shape, cuts: new Array(1) },
    { ...shape, unconfirmedMembers: null },
    { ...shape, unconfirmedMembers: [] },
    { ...shape, unconfirmedMembers: new Array(1) },
    { ...shape, unconfirmedMembers: [{ output: 7n }] },
    { ...shape, unconfirmedMembers: [{ output: "7", generation: 2n }] },
    { ...shape, unconfirmedMembers: [{ output: 7n, generation: -1n }] },
    { ...shape, unconfirmedMembers: [{ output: 1n << 64n, generation: 2n }] },
    { ...shape, cuts: [{ ...error.cuts[0], world: null }] },
    { ...shape, cuts: [{ ...error.cuts[0], world: { id: 3n } }] },
    {
      ...shape,
      cuts: [{ ...error.cuts[0], world: { id: "3", incarnation: 4n } }],
    },
  ];
  for (const field of [
    "name",
    "message",
    "cause",
    "cuts",
    "unconfirmedMembers",
  ]) {
    const missing = { ...shape };
    Reflect.deleteProperty(missing, field);
    malformed.push(missing);
  }
  for (const field of ["session", "request", "sequence", "tick"]) {
    for (const invalid of [undefined, null, 1, "1", -1n, 1n << 64n])
      malformed.push({
        ...shape,
        cuts: [{ ...error.cuts[0], [field]: invalid }],
      });
  }
  const throwing = Object.defineProperty({ ...shape }, "cuts", {
    get() {
      throw new Error("unreadable field");
    },
  });
  const revoked = Proxy.revocable({}, {});
  revoked.revoke();
  malformed.push(throwing, revoked.proxy);
  for (const candidate of malformed)
    assert.equal(isLifecycleWatchRemoveError(candidate), false);
});

test("whole cleanup joins subset work rather than sending stale duplicate removals", async () => {
  const state = await groupFixture(3);
  const subset = state.watch.removeMembers([state.watch.baselines[1]!.member]);
  const whole = state.watch.remove();
  assert.deepEqual(state.requests[2]!.control, {
    kind: "remove",
    world,
    output: 7n,
    generations: [1n, 3n],
  });
  state.deliver(state.ack(1));
  state.deliver(state.ack(2));
  await subset;
  assert.deepEqual(
    (await whole).map((cut) => cut.request),
    [2n, 3n],
  );
});

test("old group generations cannot remove readded users or revive after cancelled pages", async () => {
  const state = await groupFixture(2);
  const id = state.watch.baselines[0]!.member;
  const removing = state.watch.removeMembers([id]);
  const rejected = removing.catch((error: unknown) => error);
  state.deliver(
    state.record(
      {
        world,
        output: 7n,
        kind: "ack",
        action: "remove",
        cut: null,
        result: { kind: "cancelled" },
      },
      state.requests[1]!.id,
    ),
  );
  assert.ok(isLifecycleWatchRemoveError(await rejected));
  state.deliver(state.event(1n));
  const retry = state.watch.removeMembers([id]);
  state.deliver(state.ack(2));
  await retry;
  const added = state.manager.watch(
    world,
    [selection],
    (event) => state.observed.push(event.member.generation),
    2,
  );
  state.deliver(state.ack(3));
  const newGroup = await added;
  await assert.rejects(
    state.watch.removeMembers([newGroup.baselines[0]!.member]),
    /belong/,
  );
  await state.watch.removeMembers([id]);
  assert.equal(state.requests.length, 4);
  state.deliver(state.event(3n));
  assert.deepEqual(state.observed, [1n, 3n]);
});

test("reentrant close after a remove ACK preserves confirmed cuts without claiming the unsubmitted tail", async () => {
  const state = await groupFixture(3, 1);
  const removing = state.watch.removeMembers(
    state.watch.baselines.map((baseline) => baseline.member),
  );
  const failure = removing.catch((error: unknown) => error);
  state.deliver(state.ack(3));
  state.manager.stop(new Error("closed at ACK boundary"));
  const error = await failure;
  assert.ok(isLifecycleWatchRemoveError(error));
  assert.deepEqual(
    error.cuts.map((cut) => cut.request),
    [4n],
  );
  assert.deepEqual(
    error.unconfirmedMembers.map((id) => id.generation),
    [2n, 3n],
  );
  assert.equal(state.requests.length, 4);
  assert.equal((await state.watch.closed).kind, "closed");
  state.deliver(state.event(2n));
  assert.deepEqual(state.observed, []);
});

for (const code of [1, 3]) {
  test(`subset generic error ${code} retains its prefix and fences unknown effects synchronously`, async () => {
    const state = await groupFixture(3, 1);
    const removing = state.watch.removeMembers(
      state.watch.baselines.map((baseline) => baseline.member),
    );
    const failure = removing.catch((error: unknown) => error);
    state.deliver(state.ack(3));
    await new Promise<void>((resolve) => setImmediate(resolve));
    state.deliver({
      session: 5n,
      requestId: state.requests[4]!.id,
      tick: 0n,
      body: { kind: "error", code, message: "uncertain page" },
    });
    state.deliver(state.event(3n));
    const error = await failure;
    assert.ok(isLifecycleWatchRemoveError(error));
    assert.deepEqual(
      error.cuts.map((cut) => cut.request),
      [4n],
    );
    assert.deepEqual(
      error.unconfirmedMembers.map((member) => member.generation),
      [2n, 3n],
    );
    assert.deepEqual(state.observed, code === 1 ? [3n] : []);
    assert.equal(state.requests.length, 5);
    assert.equal(state.failures.length, code === 1 ? 0 : 1);
  });
}

test("synchronous removal ACK is journaled before a duplicate call can transmit", async () => {
  const state = await groupFixture(2);
  const adapter = Reflect.get(state.manager, "adapter");
  const send = adapter.send.bind(adapter);
  adapter.send = (request: bigint, control: LifecycleWatchRequest) => {
    const pending = send(request, control);
    state.deliver(state.ack(state.requests.length - 1));
    return pending;
  };
  const member = state.watch.baselines[0]!.member;
  const first = state.watch.removeMembers([member]);
  const duplicate = state.watch.removeMembers([member]);
  assert.equal(state.requests.length, 2);
  assert.deepEqual(await duplicate, await first);
  assert.equal(state.failures.length, 0);
});

const valueSelection: LifecycleTargetSelection = {
  target: { kind: "value", entity: 10n, component: 2, fields: [0, 8] },
  kinds: 128,
};

function valueRecord(
  state: ReturnType<typeof fixture>,
  generation: bigint,
  tick: bigint,
  values: readonly LifecycleFieldValue[] | null,
  output = 7n,
): Response {
  return state.record({
    kind: "value",
    world,
    output,
    member: { output, generation },
    tick,
    values,
  });
}

const fieldValues = (
  checked: boolean,
  value: number,
): LifecycleFieldValue[] => [
  { offset: 0, value: checked },
  { offset: 8, value },
];

test("value members deliver the current values first, then frame-end changes, and absence as null", async () => {
  const state = fixture();
  const observed: LifecycleWatchEvent[] = [];
  const pending = state.manager.watch(
    world,
    [valueSelection, selection],
    (event) => observed.push(event),
    2,
  );
  assert.deepEqual(state.requests[0]!.control, {
    kind: "add",
    world,
    targets: [valueSelection, selection],
  });
  state.deliver(state.ack(0));
  const watch = await pending;
  assert.deepEqual(watch.baselines[0]!.target, valueSelection.target);
  assert.deepEqual(watch.baselines[0]!.lifetime, {
    kind: "component",
    entityLive: true,
    incarnation: 12n,
  });
  state.deliver(valueRecord(state, 1n, 90n, fieldValues(false, 0.25)));
  state.deliver(state.event(2n, 11n));
  state.deliver(valueRecord(state, 1n, 92n, fieldValues(true, 0.25)));
  state.deliver(valueRecord(state, 1n, 95n, null));
  assert.equal(state.failures.length, 0);
  assert.deepEqual(
    observed.map((event) =>
      event.kind === "value"
        ? [event.member.generation, event.tick, event.values]
        : [event.member.generation, event.sequence],
    ),
    [
      [1n, 90n, fieldValues(false, 0.25)],
      [2n, 11n],
      [1n, 92n, fieldValues(true, 0.25)],
      [1n, 95n, null],
    ],
  );
  const first = observed[0]!;
  assert.equal(Object.isFrozen(first), true);
  assert.ok(first.kind === "value" && Object.isFrozen(first.values));
  const removed = watch.remove();
  state.deliver(state.ack(1));
  await removed;
  state.deliver(valueRecord(state, 1n, 96n, fieldValues(true, 1)));
  assert.equal(observed.length, 4, "no value after removal");
});

test("value records out of tick order, with other fields or for other members fail the connection", async () => {
  const cases: [string, (state: ReturnType<typeof fixture>) => Response][] = [
    ["repeated tick", (state) => valueRecord(state, 1n, 90n, null)],
    ["earlier tick", (state) => valueRecord(state, 1n, 89n, null)],
    [
      "swapped fields",
      (state) =>
        valueRecord(state, 1n, 91n, [
          { offset: 8, value: 1 },
          { offset: 0, value: true },
        ]),
    ],
    [
      "missing field",
      (state) => valueRecord(state, 1n, 91n, [{ offset: 0, value: true }]),
    ],
    ["lifecycle member", (state) => valueRecord(state, 2n, 91n, null)],
    ["unknown member", (state) => valueRecord(state, 9n, 91n, null)],
    ["foreign output", (state) => valueRecord(state, 1n, 91n, null, 8n)],
  ];
  for (const [name, bad] of cases) {
    const state = fixture();
    const observed: LifecycleWatchEvent[] = [];
    const pending = state.manager.watch(
      world,
      [valueSelection, selection],
      (event) => observed.push(event),
      2,
    );
    state.deliver(state.ack(0));
    const watch = await pending;
    state.deliver(valueRecord(state, 1n, 90n, fieldValues(true, 1)));
    state.deliver(bad(state));
    assert.equal(state.failures.length, 1, name);
    // A record for another output fails at the endpoint check first.
    assert.match(
      state.failures[0]!.message,
      name === "foreign output"
        ? /Foreign lifecycle endpoint/
        : /Unexpected lifecycle value/,
      name,
    );
    assert.equal(observed.length, 1, name);
    assert.equal((await watch.closed).kind, "closed", name);
  }
});
