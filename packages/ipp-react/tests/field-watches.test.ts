import assert from "node:assert/strict";
import test from "node:test";
import { createElement, Fragment } from "react";
import {
  type LifecycleTargetWatch,
  type LifecycleWatchClosure,
  type LifecycleWatchEvent,
} from "@ipp/client";
import { createRoot } from "../src/root.js";
import { useField, type ReactFieldState } from "../src/scope/field.js";
import { ReactFieldWatches } from "../src/scope/field-watches.js";
import type { ReactWorldClient } from "../src/reconciler/world-client.js";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function watch(failures = 0) {
  const closure = deferred<LifecycleWatchClosure>();
  let removes = 0;
  const value: LifecycleTargetWatch = {
    world: { id: 1n, incarnation: 1n },
    baselines: [],
    cuts: [],
    closed: closure.promise,
    async removeMembers() {
      return [];
    },
    async remove() {
      removes++;
      if (failures-- > 0) throw new Error("remove failed");
      closure.resolve({ kind: "removed", cuts: [] });
      return [];
    },
  };
  return { value, removes: () => removes };
}

function valueEvent(value: number): LifecycleWatchEvent {
  return {
    world: { id: 1n, incarnation: 1n },
    output: 1n,
    kind: "value",
    member: { output: 1n, generation: 1n },
    tick: 1n,
    values: [{ offset: 0, value }],
  };
}

function receiving(
  admit: NonNullable<ReactWorldClient["watchLifecycle"]>,
): ReactWorldClient {
  return {
    session: 1n,
    schemaHash: 1n,
    components: {
      Scalar: { id: 17, fields: { value: { offset: 0, kind: 1 } } },
    },
    watchLifecycle: admit,
    async batch() {
      throw new Error("Empty reader root must not write");
    },
  };
}

async function settle(root: ReturnType<typeof createRoot>) {
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await root.flush();
  }
}

test("field reader reports initially absent target and failed commit without repeated snapshots", async () => {
  const client = receiving(async () => {
    throw new Error("Missing declaration must not admit");
  });
  const root = createRoot(client, { onError() {} });
  let state: ReactFieldState | undefined;
  let renders = 0;
  function Reader() {
    state = useField("missing", "Scalar", "value");
    renders++;
    return null;
  }
  try {
    await root.render(createElement(Reader));
    await settle(root);
    assert.equal(state?.status, "absent");
    const before = renders;
    await assert.rejects(
      root.render(
        createElement(
          Fragment,
          null,
          createElement(Reader),
          createElement("ipp-children"),
        ),
      ),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(state?.status, "failed");
    assert.ok(
      renders - before < 8,
      "Repeated identical state caused a render loop",
    );
    await root.render(createElement(Reader));
    await settle(root);
    assert.equal(state?.status, "absent");
  } finally {
    await root.unmount();
  }
});

test("healthy watch survives failed commit and restores latest native report after recovery", async () => {
  const native = watch();
  let admits = 0;
  let listener!: (event: LifecycleWatchEvent) => void;
  const root = createRoot(
    receiving(async (_, report) => {
      admits++;
      listener = report;
      return native.value;
    }),
    { onError() {} },
  );
  let state: ReactFieldState | undefined;
  function Reader() {
    state = useField(123n, "Scalar", "value");
    return null;
  }
  try {
    await root.render(createElement(Reader));
    await settle(root);
    listener(valueEvent(13));
    await settle(root);
    assert.equal(state?.value, 13);
    await assert.rejects(
      root.render(
        createElement(
          Fragment,
          null,
          createElement(Reader),
          createElement("ipp-children"),
        ),
      ),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(state?.status, "failed");
    listener(valueEvent(14));
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(state?.status, "failed");
    await root.render(createElement(Reader));
    await settle(root);
    assert.deepEqual(state, { status: "ready", value: 14 });
    assert.equal(admits, 1);
  } finally {
    await root.unmount();
  }
  assert.equal(native.removes(), 1);
});

test("root unmount awaits pending field admission then its exact release", async () => {
  const admission = deferred<LifecycleTargetWatch>();
  const native = watch();
  let admits = 0;
  const root = createRoot(
    receiving(async () => {
      admits++;
      return admission.promise;
    }),
  );
  function Reader() {
    useField(123n, "Scalar", "value");
    return null;
  }
  await root.render(createElement(Reader));
  await settle(root);
  assert.equal(admits, 1);
  let completed = false;
  const closing = root.unmount().then(() => {
    completed = true;
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(completed, false);
  admission.resolve(native.value);
  await closing;
  assert.equal(native.removes(), 1);
});

test("root cleanup retries a failed field watch release without losing its handle", async () => {
  const native = watch(2);
  const root = createRoot(
    receiving(async () => native.value),
    { onError() {} },
  );
  function Reader() {
    useField(123n, "Scalar", "value");
    return null;
  }
  await root.render(createElement(Reader));
  await settle(root);
  await assert.rejects(root.unmount(), /cleanup/);
  assert.equal(native.removes(), 2);
  await root.unmount();
  assert.equal(native.removes(), 3);
});

test("failed partial admission remains owned until exact cleanup succeeds", async () => {
  const native = watch(1);
  const errors: unknown[] = [];
  const registry = new ReactFieldWatches((error) => {
    errors.push(error);
  });
  const lease = registry.track(
    Promise.reject(
      Object.assign(new Error("start failed"), {
        name: "LifecycleWatchStartError",
        partial: native.value,
      }),
    ),
  );
  await assert.rejects(lease.admitted, /start failed/);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(native.removes(), 1);
  assert.equal(errors.length, 1);
  await registry.dispose();
  assert.equal(native.removes(), 2);
});

test("failed watch admission can retry same target after a corrected commit", async () => {
  const native = watch();
  let admits = 0;
  const root = createRoot(
    receiving(async (_, report) => {
      if (++admits === 1) throw new Error("temporary admission failure");
      report(valueEvent(15));
      return native.value;
    }),
    { onError() {} },
  );
  let state: ReactFieldState | undefined;
  function Reader() {
    state = useField(123n, "Scalar", "value");
    return null;
  }
  try {
    await root.render(createElement(Reader));
    await settle(root);
    assert.equal(state?.status, "failed");
    await assert.rejects(
      root.render(
        createElement(
          Fragment,
          null,
          createElement(Reader),
          createElement("ipp-children"),
        ),
      ),
    );
    await root.render(createElement(Reader));
    await settle(root);
    assert.deepEqual(state, { status: "ready", value: 15 });
    assert.equal(admits, 2);
  } finally {
    await root.unmount();
  }
});
