import assert from "node:assert/strict";
import test from "node:test";
import { createElement, useLayoutEffect, useState } from "react";
import {
  ValueSynchronization,
  type ValueSynchronizationOptions,
} from "../src/gui-kit/values/value-synchronization.js";
import {
  useScalarValue,
  type ScalarValue,
} from "../src/gui-kit/values/scalar-value.js";
import { useCheckedValue } from "../src/gui-kit/values/checked-value.js";
import { createRoot } from "../src/root.js";
import type { ReactWorldClient } from "../src/reconciler/world-client.js";
import type { GuiControlHandle } from "../src/gui/control-ref.js";

function fixture(initial = 0) {
  const changes: number[] = [];
  const writes: { from: number; to: number; ack(accepted: boolean): void }[] =
    [];
  const errors: unknown[] = [];
  let desired: number | undefined = initial;
  let identity: unknown = {};
  const options = (): ValueSynchronizationOptions<number> => ({
    value: desired,
    defaultValue: initial,
    same: Object.is,
    onChange: (value) => {
      changes.push(value);
    },
    onError: (error) => {
      errors.push(error);
    },
    identity: () => identity,
    write: async (from, to, context) => {
      await context.trackEcho(
        to,
        () =>
          new Promise<boolean>((ack) => {
            writes.push({ from, to, ack });
          }),
      );
    },
  });
  const state = new ValueSynchronization(options());
  return {
    state,
    changes,
    writes,
    errors,
    desired(value: number | undefined) {
      desired = value;
      state.configure(options());
    },
    replace() {
      identity = {};
      state.notifyReady();
    },
  };
}

const observation = (tick: number, initial = false) => ({
  tick: BigInt(tick),
  initial,
});

function asyncChanges() {
  const pending: {
    value: number;
    resolve(): void;
    reject(error: Error): void;
  }[] = [];
  const writes: [number, number][] = [];
  const errors: unknown[] = [];
  let intent = 0;
  const options = (): ValueSynchronizationOptions<number> => ({
    value: intent,
    defaultValue: 0,
    same: Object.is,
    onChange: (value) =>
      new Promise<void>((resolve, reject) => {
        pending.push({ value, resolve, reject });
      }),
    onError: (error) => {
      errors.push(error);
    },
    write: async (from, to, context) => {
      await context.trackEcho(to, async () => {
        writes.push([from, to]);
        return true;
      });
    },
  });
  const state = new ValueSynchronization(options());
  state.observe(0, observation(0, true));
  return {
    state,
    pending,
    writes,
    errors,
    configure(value = intent) {
      intent = value;
      state.configure(options());
    },
  };
}

test("async accepted drag samples keep flowing and older completions cannot restore stale props", async () => {
  const f = asyncChanges();
  f.state.observe(0.1, observation(1));
  f.state.observe(0.2, observation(2));
  f.configure(); // An unrelated parent render still supplies the old intent.
  await f.state.synchronize();
  assert.deepEqual(
    f.pending.map(({ value }) => value),
    [0.1, 0.2],
  );
  assert.deepEqual(f.writes, []);
  assert.equal(f.state.getSnapshot().pending, true);
  f.configure(0.1);
  f.pending[0]!.resolve();
  await Promise.resolve();
  await f.state.synchronize();
  assert.deepEqual(
    f.writes,
    [],
    "the older accepted prop cannot fight the newer native sample",
  );
  f.configure(0.2);
  f.pending[1]!.resolve();
  await Promise.resolve();
  await f.state.synchronize();
  assert.equal(f.state.getSnapshot().pending, false);
  assert.deepEqual(f.writes, []);
});

test("settled async refusal and rejection restore unchanged intent and report errors", async () => {
  for (const rejected of [false, true]) {
    const f = asyncChanges();
    f.state.observe(0.4, observation(1));
    await f.state.synchronize();
    assert.deepEqual(f.writes, []);
    if (rejected) f.pending[0]!.reject(new Error("application refused"));
    else f.pending[0]!.resolve();
    await Promise.resolve();
    await f.state.synchronize();
    assert.deepEqual(f.writes, [[0.4, 0]]);
    assert.equal(f.errors.length, rejected ? 1 : 0);
    f.state.observe(0, observation(2));
    assert.equal(
      f.pending.length,
      1,
      "restoration echoes do not call onChange",
    );
  }
});

test("explicit synchronous replacement supersedes async acceptance and old lifetimes stay fenced", async () => {
  const f = asyncChanges();
  f.state.observe(0.2, observation(1));
  f.configure(0.8);
  await f.state.synchronize();
  assert.deepEqual(f.writes, [[0.2, 0.8]]);
  f.pending[0]!.resolve();
  await Promise.resolve();
  assert.equal(f.state.getSnapshot().pending, false);
  f.state.observe(0.8, observation(2));
  f.state.observe(0.9, observation(3));
  f.state.deactivate();
  f.state.activate();
  f.state.observe(0.8, observation(4, true));
  f.pending[1]!.reject(new Error("retired acceptance"));
  await Promise.resolve();
  await f.state.synchronize();
  assert.deepEqual(f.errors, []);
  assert.deepEqual(f.writes, [[0.2, 0.8]]);
});

test("superseded async rejection reports once without releasing a newer pending sample", async () => {
  const f = asyncChanges();
  f.state.observe(0.1, observation(1));
  f.state.observe(0.2, observation(2));
  f.pending[0]!.reject(new Error("older acceptance failed"));
  await Promise.resolve();
  await f.state.synchronize();
  assert.equal(f.errors.length, 1);
  assert.equal(f.state.getSnapshot().pending, true);
  assert.deepEqual(f.writes, []);
  f.configure(0.2);
  f.pending[1]!.resolve();
  await Promise.resolve();
  await f.state.synchronize();
  assert.deepEqual(f.writes, []);
});

test("mounted scalar async rejection reaches the root error observer and restores intent", async () => {
  let state!: ScalarValue;
  const writes: number[] = [];
  const errors: Error[] = [];
  const refused = new Error("async application rejection");
  const handle = {
    target: {
      world: { id: 1n, incarnation: 1n },
      entity: 1n,
      component: 1,
      incarnation: 1n,
    },
    async compareAndSet(_field: string, _from: number, to: number) {
      writes.push(to);
      return true;
    },
  } as unknown as GuiControlHandle;
  const root = createRoot(
    {
      session: 1n,
      schemaHash: 1n,
      components: {},
      manifest: { systems: [], components: [], operations: [] },
      async batch() {
        throw new Error("Probe has no native declarations");
      },
    },
    { onError: (error) => errors.push(error) },
  );
  function Probe() {
    state = useScalarValue(
      [0],
      [0],
      async () => {
        throw refused;
      },
      undefined,
    );
    useLayoutEffect(() => {
      if (typeof state.control.ref === "function")
        return state.control.ref(handle);
    }, [state.control.ref]);
    return null;
  }
  try {
    await root.render(createElement(Probe));
    state.control.onScalarCommit({
      target: handle.target,
      tick: 0n,
      value: 0,
      initial: true,
    });
    state.control.onScalarCommit({
      target: handle.target,
      tick: 1n,
      value: 0.5,
    });
    for (let count = 0; count < 10 && !writes.length; count++)
      await root.flush();
    assert.deepEqual(errors, [refused]);
    assert.deepEqual(writes, [0]);
  } finally {
    await root.unmount();
  }
});

test("repeated identical rejected native edits each restore controlled intent", async () => {
  const { state, changes, writes } = fixture(0);
  state.observe(0, observation(0, true));
  for (let tick = 1; tick <= 3; tick++) {
    assert.equal(state.observe(1, observation(tick * 2)), true);
    const pending = state.synchronize();
    assert.deepEqual([writes.at(-1)!.from, writes.at(-1)!.to], [1, 0]);
    assert.equal(state.observe(0, observation(tick * 2 + 1)), false);
    writes.at(-1)!.ack(true);
    await pending;
    assert.equal(state.getSnapshot().value, 0);
  }
  assert.deepEqual(changes, [1, 1, 1]);
});

test("A to B to C writes serialize and delayed own reports never notify the application", async () => {
  const f = fixture(0);
  f.state.observe(0, observation(0, true));
  f.desired(1);
  const first = f.state.synchronize();
  f.desired(2);
  await f.state.synchronize();
  assert.equal(f.writes.length, 1);
  f.writes[0]!.ack(true);
  await first;
  const staleExpected = f.state.synchronize();
  assert.deepEqual([f.writes[1]!.from, f.writes[1]!.to], [0, 2]);
  // B's delayed report is an acknowledged programmatic echo, not a user edit.
  assert.equal(f.state.observe(1, observation(1)), false);
  f.writes[1]!.ack(false);
  await staleExpected;
  const latest = f.state.synchronize();
  assert.deepEqual([f.writes[2]!.from, f.writes[2]!.to], [1, 2]);
  f.state.observe(2, observation(2));
  f.writes[2]!.ack(true);
  await latest;
  assert.deepEqual(f.changes, []);
  f.desired(undefined);
  assert.equal(
    f.state.getSnapshot().value,
    2,
    "dropping control retains the latest real echo observation",
  );
});

test("a failed CAS releases a matching pending native report and retires the candidate", async () => {
  const f = fixture(0);
  f.state.observe(0, observation(0, true));
  f.desired(1);
  const pending = f.state.synchronize();
  f.state.observe(1, observation(1));
  assert.deepEqual(f.changes, [], "origin is unconfirmed until ACK");
  assert.equal(
    f.state.getSnapshot().value,
    1,
    "the actual report is never hidden from the view",
  );
  f.writes[0]!.ack(false);
  await pending;
  assert.deepEqual(f.changes, [1]);
  f.state.observe(2, observation(2));
  f.state.observe(1, observation(3));
  assert.deepEqual(
    f.changes,
    [1, 2, 1],
    "a later genuine matching value is not a stale echo",
  );
});

test("failed expected pairs wait for a fresh observation rather than spinning", async () => {
  const f = fixture(0);
  f.state.observe(0, observation(0, true));
  f.desired(1);
  const pending = f.state.synchronize();
  f.writes[0]!.ack(false);
  await pending;
  for (let attempt = 0; attempt < 5; attempt++) await f.state.synchronize();
  assert.equal(f.writes.length, 1);
  f.state.observe(3, observation(1));
  const fresh = f.state.synchronize();
  assert.deepEqual([f.writes[1]!.from, f.writes[1]!.to], [3, 1]);
  f.writes[1]!.ack(false);
  await fresh;
});

test("initial native seeds precede writes and never call public change listeners", async () => {
  const f = fixture(1);
  await f.state.synchronize();
  assert.equal(f.writes.length, 0);
  f.state.observe(0, observation(0, true));
  const pending = f.state.synchronize();
  f.state.observe(1, observation(1));
  f.writes[0]!.ack(true);
  await pending;
  assert.deepEqual(f.changes, []);
});

test("unmount and native identity replacement fence pending ACKs and reset report ticks", async () => {
  const f = fixture(0);
  f.state.observe(0, observation(100, true));
  f.desired(1);
  const pending = f.state.synchronize();
  f.state.observe(1, observation(101));
  f.state.deactivate();
  f.state.activate();
  f.replace();
  f.state.observe(3, observation(0, true));
  f.writes[0]!.ack(false);
  await pending;
  assert.deepEqual(f.changes, []);
  assert.equal(f.state.getSnapshot().value, 3);
});

test("unknown ACK outcomes preserve observed truth and release unconfirmed changes", async () => {
  const changes: number[] = [];
  const errors: unknown[] = [];
  let reject!: (error: unknown) => void;
  const state = new ValueSynchronization({
    value: 1,
    defaultValue: 0,
    same: Object.is,
    onChange: (value: number) => {
      changes.push(value);
    },
    onError: (error) => {
      errors.push(error);
    },
    write: async (_from, to, context) => {
      await context.trackEcho(
        to,
        () =>
          new Promise<boolean>((_accept, fail) => {
            reject = fail;
          }),
      );
    },
  });
  state.observe(0, observation(0, true));
  const pending = state.synchronize();
  state.observe(1, observation(1));
  reject(new Error("unknown transport outcome"));
  await pending;
  assert.equal(state.getSnapshot().value, 1);
  assert.deepEqual(changes, [1]);
  assert.equal(errors.length, 1);
});

test("the scalar range adapter orders native fields and stops after a failed first CAS", async () => {
  const writes: [string, number, number][] = [];
  let state!: ScalarValue;
  const handle = {
    target: {
      world: { id: 1n, incarnation: 1n },
      entity: 1n,
      component: 1,
      incarnation: 1n,
    },
    async compareAndSet(field: string, from: number, to: number) {
      writes.push([field, from, to]);
      return false;
    },
  } as unknown as GuiControlHandle;
  const client: ReactWorldClient = {
    session: 1n,
    schemaHash: 1n,
    components: {},
    manifest: { systems: [], components: [], operations: [] },
    async batch() {
      throw new Error("Probe has no native declarations");
    },
  };
  function Probe({ value }: { value: readonly number[] }) {
    state = useScalarValue(value, [0, 1], undefined, undefined);
    useLayoutEffect(() => {
      const ref = state.control.ref;
      if (typeof ref === "function") return ref(handle);
    }, [state.control.ref]);
    return null;
  }
  const root = createRoot(client);
  try {
    await root.render(createElement(Probe, { value: [0, 1] }));
    state.control.onScalarCommit({
      target: handle.target,
      tick: 0n,
      value: 0,
      upper: 1,
      initial: true,
    });
    await root.render(createElement(Probe, { value: [2, 3] }));
    for (let attempt = 0; attempt < 10 && !writes.length; attempt++)
      await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(
      writes,
      [["upper", 1, 3]],
      "a refused upper extension never proceeds with the lower field",
    );
  } finally {
    await root.unmount();
  }
});

test("deferred genuine reports call the latest committed callback", async () => {
  const f = fixture(0);
  f.state.observe(0, observation(0, true));
  f.desired(1);
  const pending = f.state.synchronize();
  f.state.observe(1, observation(1));
  const latest: number[] = [];
  f.state.configure({
    value: 1,
    defaultValue: 0,
    same: Object.is,
    onChange: (value) => {
      latest.push(value);
    },
    write: async () => {},
  });
  f.writes[0]!.ack(false);
  await pending;
  assert.deepEqual(f.changes, []);
  assert.deepEqual(latest, [1]);
});

test("a baseline delivered before initial ref publication admits later programmatic edits", async () => {
  let identity: unknown;
  const writes: number[] = [];
  const state = new ValueSynchronization({
    value: 1,
    defaultValue: 0,
    same: Object.is,
    onChange: undefined,
    identity: () => identity,
    write: async (_from, to, context) => {
      await context.trackEcho(to, async () => {
        writes.push(to);
        return true;
      });
    },
  });
  state.observe(0, observation(4, true));
  await state.synchronize();
  assert.deepEqual(
    writes,
    [],
    "no write guesses an unpublished native identity",
  );
  identity = {};
  state.notifyReady();
  await state.synchronize();
  assert.deepEqual(
    writes,
    [1],
    "the acknowledged baseline survives initial handle publication",
  );
});

test("a mounted controlled adapter lets the application's accepted callback commit before correction", async () => {
  const writes: boolean[] = [];
  let state!: ReturnType<typeof useCheckedValue>;
  const handle = {
    target: {
      world: { id: 1n, incarnation: 1n },
      entity: 1n,
      component: 1,
      incarnation: 1n,
    },
    async compareAndSet(_field: string, _from: boolean, to: boolean) {
      writes.push(to);
      return true;
    },
  } as unknown as GuiControlHandle;
  const client: ReactWorldClient = {
    session: 1n,
    schemaHash: 1n,
    components: {},
    manifest: { systems: [], components: [], operations: [] },
    async batch() {
      throw new Error("Probe has no native declarations");
    },
  };
  function Probe() {
    const [value, setValue] = useState(false);
    state = useCheckedValue(value, false, setValue, undefined);
    useLayoutEffect(() => {
      const ref = state.ref;
      if (typeof ref === "function") return ref(handle);
    }, [state.ref]);
    return null;
  }
  const root = createRoot(client);
  try {
    await root.render(createElement(Probe));
    state.changed({
      target: handle.target,
      tick: 0n,
      value: false,
      initial: true,
    });
    state.changed({ target: handle.target, tick: 1n, value: true });
    for (let attempt = 0; attempt < 10; attempt++)
      await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(state.current, true);
    assert.deepEqual(
      writes,
      [],
      "accepted application intent must not be corrected back to the previous prop",
    );
  } finally {
    await root.unmount();
  }
});

test("group member admission wakes a deferred expected pair without losing its aggregate observation", async () => {
  const writes: number[] = [];
  let admitted = false;
  const state = new ValueSynchronization({
    value: 1,
    defaultValue: 0,
    onChange: undefined,
    same: Object.is,
    write: async (_from, to, context) => {
      if (admitted)
        await context.trackEcho(to, async () => {
          writes.push(to);
          return true;
        });
    },
  });
  state.observe(0, observation(0, true));
  await state.synchronize();
  admitted = true;
  state.retry();
  await state.synchronize();
  assert.deepEqual(writes, [1]);
  assert.equal(
    state.getSnapshot().value,
    0,
    "admission is not an optimistic field report",
  );
});
