import assert from "node:assert/strict";
import test from "node:test";
import {
  FieldKind,
  type BatchOutcome,
  type Command,
  type InspectionPage,
  type LifecycleTargetEvent,
  type LifecycleTargetWatch,
  type LifecycleWatchClosure,
  type LifecycleWatchEvent,
} from "@ipp/client";
import { ReactControlRefs } from "../src/gui/control-ref-registry.js";
import type { ReactWorldClient } from "../src/reconciler/world-client.js";
import type {
  GuiControlHandle,
  GuiControlRef,
} from "../src/gui/control-ref.js";
import type { ReactWorldDescription } from "../src/reconciler/tree.js";

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

type ControlClient = Pick<
  ReactWorldClient,
  | "session"
  | "worldReference"
  | "closure"
  | "components"
  | "batch"
  | "watchLifecycle"
  | "inspectPage"
>;

const components = {
  GuiCheckbox: {
    id: 42,
    fields: {
      label: { offset: 0, kind: FieldKind.String },
      checked: { offset: 16, kind: FieldKind.Bool },
    },
  },
};

/** A page holding entity `entity` with a checkbox whose `checked` is given. */
function checkboxPage(entity: bigint, checked: boolean): InspectionPage {
  return {
    next: 0n,
    tick: 1n,
    time: 0,
    entities: [
      {
        id: entity,
        metadata: { symbolicId: "control", classes: [] },
        link: { parent: null, order: 0n },
        components: [{ component: 42, fields: { label: "", checked } }],
      },
    ],
    resources: [],
    renderDiagnostics: [],
  };
}

function outcome(error?: string): BatchOutcome {
  const base = { batchId: 1n, tick: 1n, aliases: [], symbols: [], effects: [] };
  return error === undefined
    ? { ...base, ok: true }
    : {
        ...base,
        ok: false,
        error: { scope: "operation", operation: 0, reason: error },
      };
}

function boundary() {
  let listener: ((event: LifecycleWatchEvent) => void) | undefined;
  let subscriptions = 0;
  let unsubscribed = 0;
  let reads = 0;
  let refreshes = 0;
  let incarnation = 1n;
  let checked = false;
  let rejection: string | undefined;
  let closure: ReactWorldClient["closure"];
  const errors: unknown[] = [];
  const batches: Command[][] = [];
  const actions: Command[] = [];
  const subscribed = deferred<LifecycleTargetWatch>();
  const ended = deferred<LifecycleWatchClosure>();
  const started = deferred<void>();
  const client: ControlClient = {
    session: 1n,
    worldReference: { id: 1n, incarnation: 1n },
    get closure() {
      return closure;
    },
    components,
    watchLifecycle: async (_, next) => {
      subscriptions++;
      listener = next;
      started.resolve();
      return subscribed.promise;
    },
    inspectPage: async () => {
      reads++;
      return checkboxPage(10n, checked);
    },
    // A World that applies one compare-and-set of `checked`.
    batch: async (operations) => {
      const [operation] = operations;
      // A GUI action is a batch of one command; this World refuses it.
      if (operation?.kind === "guiAction") {
        assert.equal(operations.length, 1);
        actions.push(operation);
        return outcome("StaleTarget");
      }
      batches.push(operations);
      if (rejection) return outcome(rejection);
      assert.ok(operation?.kind === "setFieldIf");
      assert.ok(
        operation.expected.kind === "bool" &&
          operation.field.value.kind === "bool",
      );
      if (operation.expected.value !== checked) return outcome("ValueMismatch");
      checked = operation.field.value.value;
      return outcome();
    },
  };
  const controls = new ReactControlRefs(
    client,
    (error) => errors.push(error),
    () => {
      refreshes++;
    },
  );
  const description = (ref: GuiControlRef): ReactWorldDescription => ({
    assets: [],
    animations: [],
    attachments: [],
    entities: [
      {
        identity: 1,
        symbolicId: "control",
        kind: "declared",
        parent: undefined,
      },
    ],
    links: [],
    components: [
      {
        identity: 2,
        entity: 1,
        component: 42,
        fields: new Map(),
        control: true,
        controlRef: ref,
      },
    ],
    signature: "control",
  });
  return {
    controls,
    client,
    description,
    errors,
    batches,
    actions,
    started: started.promise,
    reject: (reason: string | undefined) => {
      rejection = reason;
    },
    publish: (_value?: ReactWorldDescription, owner = controls) =>
      owner.publish(() => ({ entity: 10n, component: 42, valid: () => true })),
    counts: () => ({ subscriptions, unsubscribed, reads, refreshes }),
    close: () => {
      closure = { reason: new Error("terminal") };
      ended.resolve({ kind: "closed", reason: closure.reason });
    },
    ack: () =>
      subscribed.resolve({
        world: { id: 1n, incarnation: 1n },
        baselines: [
          {
            member: { output: 1n, generation: 1n },
            target: { kind: "entity", entity: 10n },
            lifetime: { kind: "entity", live: true },
          },
          {
            member: { output: 1n, generation: 2n },
            target: { kind: "component", entity: 10n, component: 42 },
            lifetime: { kind: "component", entityLive: true, incarnation },
          },
        ],
        cuts: [],
        closed: ended.promise,
        removeMembers(this: LifecycleTargetWatch, members) {
          assert.deepEqual(
            members,
            this.baselines.map((value) => value.member),
          );
          return this.remove();
        },
        remove: async () => {
          unsubscribed++;
          ended.resolve({ kind: "removed", cuts: [] });
          return [];
        },
      }),
    replace: (deliver = true) => {
      const previousIncarnation = incarnation++;
      const event: LifecycleTargetEvent = {
        kind: "event",
        world: { id: 1n, incarnation: 1n },
        output: 1n,
        member: { output: 1n, generation: 2n },
        sequence: incarnation,
        tick: 1n,
        observation: {
          kind: "component",
          entity: 10n,
          component: 42,
          change: "replaced",
          previousIncarnation,
          incarnation,
        },
      };
      const notify = () => listener?.(event);
      if (deliver) notify();
      return notify;
    },
  };
}

test("Control refs bind from their tracking and unchanged publication queries nothing", async () => {
  const state = boundary();
  const ref = { current: null as GuiControlHandle | null };
  const description = state.description(ref);
  state.controls.setDesired(description);
  const publishing = state.publish(description);
  await state.started;
  assert.deepEqual(state.counts(), {
    subscriptions: 1,
    unsubscribed: 0,
    reads: 0,
    refreshes: 0,
  });
  state.ack();
  await publishing;
  assert.ok(ref.current);
  assert.deepEqual(ref.current.target, {
    world: { id: 1n, incarnation: 1n },
    entity: 10n,
    component: 42,
    incarnation: 1n,
  });
  await state.publish(description);
  assert.deepEqual(state.counts(), {
    subscriptions: 1,
    unsubscribed: 0,
    reads: 0,
    refreshes: 0,
  });
  await state.controls.dispose();
  assert.equal(ref.current, null);
  assert.equal(state.counts().unsubscribed, 1);
});

test("Control handles read fields, compare-and-set through setFieldIf and act on their exact target", async () => {
  const state = boundary();
  const ref = { current: null as GuiControlHandle | null };
  const description = state.description(ref);
  state.controls.setDesired(description);
  state.ack();
  await state.publish(description);
  const handle = ref.current!;
  assert.deepEqual(await handle.read(), { label: "", checked: false });
  assert.equal(state.counts().reads, 1);

  assert.equal(await handle.compareAndSet("checked", false, true), true);
  assert.deepEqual(state.batches[0], [
    {
      kind: "setFieldIf",
      entity: { kind: "handle", id: 10n },
      component: 42,
      field: { offset: 16, value: { kind: "bool", value: true } },
      expected: { kind: "bool", value: false },
    },
  ]);
  assert.deepEqual(await handle.read(), { label: "", checked: true });
  assert.equal(
    await handle.compareAndSet("checked", false, true),
    false,
    "a mismatch is a false result without effect",
  );
  await assert.rejects(
    handle.compareAndSet("missing", false, true),
    /Unknown control field/,
  );
  await assert.rejects(handle.compareAndSet("checked", 1, true), TypeError);
  assert.equal(state.batches.length, 2);
  state.reject("MissingComponent");
  await assert.rejects(
    handle.compareAndSet("checked", true, false),
    /rejected: MissingComponent/,
  );

  assert.deepEqual(
    await handle.action({ kind: "toggle" }),
    outcome("StaleTarget"),
  );
  assert.deepEqual(state.actions, [
    {
      kind: "guiAction",
      entity: { kind: "handle", id: handle.target.entity },
      component: handle.target.component,
      incarnation: handle.target.incarnation,
      action: { kind: "toggle" },
    },
  ]);

  state.replace();
  assert.equal(ref.current, null);
  await assert.rejects(handle.read(), /no longer live/);
  await assert.rejects(
    handle.compareAndSet("checked", true, false),
    /no longer live/,
  );
  await assert.rejects(handle.action({ kind: "toggle" }), /no longer live/);
  assert.equal(state.actions.length, 1);
  await state.controls.dispose();
});

test("An early refresh retains dirty refs until their authoring acknowledgement", async () => {
  const state = boundary();
  const ref = { current: null as GuiControlHandle | null };
  const description = state.description(ref);
  state.controls.setDesired(description);
  await state.controls.publish(() => undefined);
  assert.equal(state.controls.needsPublication(), true);
  assert.equal(state.counts().subscriptions, 0);
  state.ack();
  await state.publish(description);
  assert.ok(ref.current);
  assert.equal(state.controls.needsPublication(), false);
  assert.equal(state.counts().subscriptions, 1);
  await state.controls.dispose();
});

test("A replaced component retires its handle and the next publication binds the new incarnation", async () => {
  for (const replacementsBeforeAck of [0, 1, 3]) {
    const state = boundary();
    const ref = { current: null as GuiControlHandle | null };
    const description = state.description(ref);
    state.controls.setDesired(description);
    const publishing = state.publish(description);
    await state.started;
    // Replacements before the ACK are part of the baseline's incarnation.
    for (let index = 0; index < replacementsBeforeAck; index++)
      state.replace(false);
    state.ack();
    await publishing;
    const first = ref.current!;
    assert.equal(first.target.incarnation, BigInt(1 + replacementsBeforeAck));
    state.replace();
    assert.equal(ref.current, null);
    assert.equal(state.counts().refreshes, 1);
    await assert.rejects(first.read(), /no longer live/);
    await state.publish(description);
    const next = (): GuiControlHandle | null => ref.current;
    assert.equal(next()?.target.incarnation, BigInt(2 + replacementsBeforeAck));
    assert.equal(state.counts().subscriptions, 1, "tracking continues");
    await state.controls.dispose();
  }
});

test("Connection closure permanently fences every ref in that scope, including unchanged rerenders", async () => {
  const state = boundary();
  const ref = { current: null as GuiControlHandle | null };
  const current = () => ref.current;
  const description = state.description(ref);
  state.controls.setDesired(description);
  state.ack();
  await state.publish(description);
  const old = ref.current!;
  state.close();
  await Promise.resolve();
  assert.equal(ref.current, null);
  await assert.rejects(old.read(), /no longer live/);
  assert.equal(state.errors.length, 1);
  state.controls.setDesired(description);
  await assert.rejects(state.publish(description), /terminal/);
  assert.equal(current(), null);
  assert.equal(state.counts().subscriptions, 1);
  state.controls.setDesired({ ...description, entities: [], components: [] });
  await state.controls.releasePending();
  await state.controls.publish(() => {
    throw new Error("An empty ref scope resolved a binding");
  });
  state.close();
  const restored = current();
  if (restored) await assert.rejects(restored.read(), /no longer live/);
  await state.controls.dispose();
  assert.equal(ref.current, null);
});

test("Connection closure during a held subscription acknowledgement cannot publish a binding", async () => {
  const state = boundary();
  const ref = { current: null as GuiControlHandle | null };
  const description = state.description(ref);
  state.controls.setDesired(description);
  const pending = state.publish(description);
  await state.started;
  state.close();
  await Promise.resolve();
  state.ack();
  await assert.rejects(pending, /terminal/);
  assert.equal(ref.current, null);
  await state.controls.dispose();
  assert.equal(state.counts().unsubscribed, 0);
});

test("Unmount during a held subscription acknowledgement releases tracking without publication", async () => {
  const state = boundary();
  const ref = { current: null as GuiControlHandle | null };
  const description = state.description(ref);
  state.controls.setDesired(description);
  const pending = state.publish(description);
  await state.started;
  const disposing = state.controls.dispose();
  state.ack();
  await Promise.all([pending, disposing]);
  assert.equal(ref.current, null);
  assert.equal(state.counts().unsubscribed, 1);
});

test("A synchronously superseded ref assignment disposes only its own callback result", async () => {
  const state = boundary();
  const next = { current: null as GuiControlHandle | null };
  let disposed = 0;
  const first = state.description((value) => {
    if (value) state.controls.setDesired(state.description(next));
    return () => {
      disposed++;
    };
  });
  state.controls.setDesired(first);
  state.ack();
  await state.publish(first);
  assert.ok(next.current);
  assert.equal(disposed, 1);
  state.replace();
  assert.equal(next.current, null);
  assert.equal(disposed, 1);
  assert.equal(state.counts().refreshes, 1);
  await state.controls.dispose();
});

test("Throwing returned cleanup is reported once without invoking null fallback", async () => {
  const state = boundary();
  let nulls = 0;
  let disposals = 0;
  const description = state.description((value) => {
    if (!value) {
      nulls++;
      return;
    }
    state.controls.close();
    return () => {
      disposals++;
      throw new Error("cleanup failed");
    };
  });
  state.controls.setDesired(description);
  state.ack();
  await state.publish(description);
  await state.controls.dispose();
  assert.equal(disposals, 1);
  assert.equal(nulls, 0);
  assert.equal(state.errors.length, 1);
});

test("Known rejected lifecycle acquisition retries; unknown acquisition retains failure until client closure", async () => {
  for (const code of [
    "IPP_REQUEST_NOT_SENT",
    "IPP_REQUEST_REJECTED",
    "unknown",
  ]) {
    const state = boundary();
    let attempts = 0;
    const error = Object.assign(new Error(code), { code });
    state.client.watchLifecycle = async () => {
      attempts++;
      throw error;
    };
    const description = state.description({ current: null });
    state.controls.setDesired(description);
    await assert.rejects(state.publish(description), error);
    await assert.rejects(state.publish(description), error);
    assert.equal(attempts, code === "unknown" ? 1 : 2);
    if (code === "unknown") await assert.rejects(state.controls.dispose());
    else await state.controls.dispose();
    state.close();
    await state.controls.dispose();
  }
});

test("Roots sharing one tracking follow its lifetime and only the last release unsubscribes", async () => {
  const state = boundary();
  const first = { current: null as GuiControlHandle | null };
  const description = state.description(first);
  state.controls.setDesired(description);
  state.ack();
  await state.publish(description);
  const old = first.current!;
  const deliver = state.replace(false);
  const second = new ReactControlRefs(
    state.client,
    (error) => state.errors.push(error),
    () => {},
  );
  const ref = { current: null as GuiControlHandle | null };
  second.setDesired(state.description(ref));
  await state.publish(undefined, second);
  assert.equal(state.counts().subscriptions, 1);
  assert.equal(
    ref.current?.target.incarnation,
    1n,
    "an undelivered replacement is not yet the tracked lifetime",
  );
  deliver();
  assert.equal(first.current, null);
  assert.equal(ref.current, null);
  await assert.rejects(old.read(), /no longer live/);
  await state.publish(undefined, second);
  const next = (): GuiControlHandle | null => ref.current;
  assert.equal(next()?.target.incarnation, 2n);
  await state.controls.dispose();
  assert.equal(state.counts().unsubscribed, 0);
  await second.dispose();
  assert.equal(state.counts().unsubscribed, 1);
  assert.deepEqual(state.errors, []);
});

test("Publication rechecks only reacknowledged controls and keeps tracking of the same component", async () => {
  const state = boundary();
  const ref = { current: null as GuiControlHandle | null };
  const description = state.description(ref);
  state.controls.setDesired(description);
  state.ack();
  // Each acknowledgement is valid until the next commit retires it.
  let latest = { valid: true };
  let resolutions = 0;
  const resolve = () => {
    resolutions++;
    latest.valid = false;
    const acknowledgement = { valid: true };
    latest = acknowledgement;
    return {
      entity: 10n,
      component: 42,
      valid: () => acknowledgement.valid,
    };
  };
  await state.controls.publish(resolve);
  const handle = ref.current!;
  assert.equal(resolutions, 1);
  latest.valid = false;
  await state.controls.publish(resolve);
  assert.equal(resolutions, 1, "an unnamed control is not rechecked");
  await state.controls.publish(resolve, [2]);
  assert.equal(resolutions, 2);
  assert.equal(ref.current, handle, "the same component keeps its binding");
  assert.equal(state.counts().subscriptions, 1);
  await state.controls.publish(resolve, [2]);
  assert.equal(resolutions, 2, "a valid acknowledgement is not resolved again");
  latest.valid = false;
  await state.controls.publish(() => undefined, [2]);
  assert.equal(ref.current, null, "a lost acknowledgement retires the handle");
  await state.controls.releasePending();
  assert.equal(state.counts().unsubscribed, 1);
  assert.equal(state.controls.needsPublication(), true);
  await state.controls.dispose();
});

test("Indexed invalidation and dirty publication do not iterate unrelated ref maps", async () => {
  const listeners = new Map<bigint, (event: LifecycleWatchEvent) => void>();
  const incarnations = new Map<bigint, bigint>();
  let resolutions = 0;
  let refreshes = 0;
  let pendingRemovals = 0;
  let peakRemovals = 0;
  let registrations = 0;
  let removalGroups = 0;
  const client: ControlClient = {
    session: 1n,
    worldReference: { id: 1n, incarnation: 1n },
    components,
    batch: async () => assert.fail("Publication must not write"),
    watchLifecycle: async (targets, listener) => {
      registrations++;
      if (registrations === 1)
        throw Object.assign(new Error("Coalesced start not submitted"), {
          code: "IPP_REQUEST_NOT_SENT",
        });
      for (const { target } of targets) {
        listeners.set(target.entity, listener);
        incarnations.set(target.entity, 1n);
      }
      return {
        world: { id: 1n, incarnation: 1n },
        baselines: targets.map((selection, index) => ({
          target: selection.target,
          member: { output: 10n, generation: BigInt(index + 1) },
          lifetime:
            selection.target.kind === "entity"
              ? { kind: "entity", live: true }
              : { kind: "component", entityLive: true, incarnation: 1n },
        })),
        cuts: [],
        closed: new Promise(() => {}),
        async removeMembers(this: LifecycleTargetWatch, members) {
          assert.deepEqual(
            members,
            this.baselines.map((value) => value.member),
          );
          removalGroups++;
          pendingRemovals++;
          peakRemovals = Math.max(peakRemovals, pendingRemovals);
          await Promise.resolve();
          pendingRemovals--;
          return [];
        },
        remove: async () => [],
      };
    },
  };
  const controls = new ReactControlRefs(
    client,
    (error) => {
      throw error;
    },
    () => {
      refreshes++;
    },
  );
  const refs = Array.from({ length: 140 }, () => ({
    current: null as GuiControlHandle | null,
  }));
  const description: ReactWorldDescription = {
    entities: refs.map((_, identity) => ({
      identity,
      symbolicId: `control-${identity}`,
      kind: "declared",
      parent: undefined,
    })),
    components: refs.map((controlRef, identity) => ({
      identity,
      entity: identity,
      component: 42,
      fields: new Map(),
      control: true,
      controlRef,
    })),
    assets: [],
    animations: [],
    attachments: [],
    links: [],
    signature: "indexed",
  };
  const publish = () =>
    controls.publish((identity) => {
      resolutions++;
      return {
        entity: BigInt(identity + 10),
        component: 42,
        valid: () => true,
      };
    });
  controls.setDesired(description);
  await assert.rejects(publish(), /Coalesced start not submitted/);
  assert.ok(refs.every((ref) => ref.current === null));
  await controls.releasePending();
  await publish();
  assert.ok(refs.every((ref) => ref.current !== null));
  assert.equal(registrations, 2);
  const originals = new Map<string, object>();
  for (const name of ["desired", "bindings", "tracked"]) {
    const value: unknown = Reflect.get(controls, name);
    assert.ok(value instanceof Map);
    originals.set(name, value);
    Reflect.set(
      controls,
      name,
      new Proxy(value, {
        get(target, property) {
          if (
            [Symbol.iterator, "entries", "values", "keys", "forEach"].includes(
              property,
            )
          )
            throw new Error(`Event/publication scanned ${name}`);
          const member: unknown = Reflect.get(target, property, target);
          return typeof member === "function" ? member.bind(target) : member;
        },
      }),
    );
  }
  const old = refs[0]!.current!;
  incarnations.set(10n, 2n);
  listeners.get(10n)!({
    kind: "event",
    world: { id: 1n, incarnation: 1n },
    output: 10n,
    member: { output: 10n, generation: 2n },
    sequence: 1n,
    tick: 1n,
    observation: {
      kind: "component",
      entity: 10n,
      component: 42,
      change: "replaced",
      previousIncarnation: 1n,
      incarnation: 2n,
    },
  });
  assert.equal(refs[0]!.current, null);
  assert.equal(refreshes, 1);
  await publish();
  assert.equal(resolutions, 281);
  assert.equal(refs.at(0)?.current?.target.incarnation, 2n);
  assert.equal(refs.at(1)?.current?.target.incarnation, 1n);
  await assert.rejects(old.read(), /no longer live/);
  for (const [name, value] of originals) Reflect.set(controls, name, value);
  await controls.dispose();
  assert.equal(peakRemovals, 1);
  assert.equal(removalGroups, 1);
});
