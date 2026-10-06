import assert from "node:assert/strict";
import test from "node:test";
import {
  PresentationError,
  outputProducer,
  type CameraOutputReference,
  type OutputReference,
  type RootBinding,
  type PresentationView,
} from "@ipp/client";
import {
  CanvasPresentation,
  canvasViewport,
  type CanvasHost,
} from "../src/canvas/presentation.js";
import { CanvasLifetime, CanvasCleanupError } from "../src/canvas/lifetime.js";

function gate() {
  let resolve!: () => void;
  const promise = new Promise<void>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

const size = { width: 80, height: 60, devicePixelRatio: 2 };
const first: CameraOutputReference = {
  world: { id: 1n, incarnation: 3n },
  entity: 8n,
  incarnation: 4n,
  kind: "camera",
};
const second: CameraOutputReference = { ...first, entity: 9n };

function boundary() {
  let serial = 0n;
  let selected: PresentationView | null = null;
  let binding: RootBinding | null = null;
  const changed: (PresentationView | null)[] = [];
  const calls: string[] = [];
  const surface = { id: 1n, context: 1n, maxWidth: 200, maxHeight: 200 };
  const host = {
    resolveOutput: async (output: OutputReference) => output,
    getRootOutputBinding: async () => binding,
    setRootOutput: async (output: OutputReference, viewport: typeof size) => {
      calls.push("bind");
      binding = {
        output,
        viewport,
        generation: { host: 10n, serial: ++serial },
      };
      return binding;
    },
    clearRootOutput: async (expected: RootBinding) => {
      calls.push("clear-root");
      if (binding?.generation.serial === expected.generation.serial)
        binding = null;
    },
    reads: { release: async () => {} },
    presentation: {
      surface: async () => ({ ...surface }),
      select: async (
        surface: PresentationView["surface"],
        binding: RootBinding,
      ) => {
        calls.push("select");
        selected = {
          surface,
          binding: structuredClone(binding),
          selection: ++serial,
        };
        return selected;
      },
      resize: async (view: PresentationView, viewport: typeof size) => {
        calls.push("resize");
        if (
          selected?.selection !== view.selection ||
          binding?.generation.serial !== view.binding.generation.serial ||
          view.surface.id !== surface.id ||
          view.surface.context !== surface.context
        )
          throw new PresentationError("staleView");
        binding = {
          output: view.binding.output,
          viewport,
          generation: { host: 10n, serial: ++serial },
        };
        selected = {
          surface: { ...surface },
          binding: structuredClone(binding),
          selection: ++serial,
        };
        return selected;
      },
      clear: async (expected: PresentationView) => {
        calls.push("clear-view");
        if (selected?.selection === expected.selection) selected = null;
      },
    },
  };
  const controller = new CanvasPresentation(
    host as unknown as CanvasHost,
    (view) => changed.push(view),
  );
  return {
    host,
    controller,
    calls,
    changed,
    surface,
    root: () => binding,
    selected: () => selected,
  };
}

test("Canvas negotiates density before root binding and equal values produce no writes", async () => {
  const value = boundary();
  await value.controller.select(first, size);
  await value.controller.select(structuredClone(first), { ...size });
  assert.deepEqual(value.calls, ["bind", "select"]);
  assert.deepEqual(value.controller.viewport, {
    width: 160,
    height: 120,
    devicePixelRatio: 2,
  });
  assert.ok(Object.isFrozen(value.controller.view!.binding.generation));
  assert.deepEqual(
    canvasViewport(
      { width: 400, height: 200, devicePixelRatio: 2 },
      value.surface,
    ),
    { width: 200, height: 100, devicePixelRatio: 0.5 },
  );
  await value.controller.close();
});

test("a viewport change of the selected output resizes it in one request", async () => {
  const value = boundary();
  await value.controller.select(first, size);
  const initial = value.controller.view!;
  await value.controller.select(first, { ...size, width: 90 });
  await value.controller.select(first, {
    ...size,
    width: 90,
    devicePixelRatio: 1,
  });
  assert.deepEqual(value.calls, ["bind", "select", "resize", "resize"]);
  const view = value.controller.view!;
  assert.deepEqual(view.binding.viewport, {
    width: 90,
    height: 60,
    devicePixelRatio: 1,
  });
  assert.equal(view.selection, value.selected()!.selection);
  assert.equal(view.binding.generation.serial, value.root()!.generation.serial);
  assert.ok(view.selection > initial.selection);
  assert.equal(value.changed.length, 3);
  assert.equal(value.changed[0], initial);
  assert.equal(value.changed[2], view);
  assert.deepEqual(value.controller.journal.views, [view]);
  assert.deepEqual(value.controller.journal.bindings, [view.binding]);
  // A CSS change rounding to the same pixels and ratio sends nothing.
  await value.controller.select(first, {
    ...size,
    width: 90.2,
    devicePixelRatio: 1,
  });
  assert.equal(value.calls.length, 4);
  // Another output still binds and selects it, then releases the old root.
  await value.controller.select(second, size);
  assert.deepEqual(value.calls.slice(4), [
    "bind",
    "select",
    "clear-view",
    "clear-root",
  ]);
  await value.controller.close();
  assert.equal(value.selected(), null);
  assert.equal(value.root(), null);
});

test("a superseded resize keeps the Host's selection for the next resize", async () => {
  const value = boundary();
  await value.controller.select(first, size);
  const arrived = gate();
  const release = gate();
  const original = value.host.presentation.resize;
  value.host.presentation.resize = async (...args) => {
    const view = await original(...args);
    arrived.resolve();
    await release.promise;
    return view;
  };
  const superseded = value.controller.select(first, { ...size, width: 90 });
  await arrived.promise;
  value.host.presentation.resize = original;
  const latest = value.controller.select(first, { ...size, width: 70 });
  release.resolve();
  await Promise.all([superseded, latest]);
  assert.deepEqual(value.calls, ["bind", "select", "resize", "resize"]);
  assert.equal(value.controller.view!.binding.viewport.width, 140);
  assert.equal(value.controller.view!.selection, value.selected()!.selection);
  assert.equal(value.controller.journal.views.length, 1);
  assert.equal(value.controller.journal.bindings.length, 1);
  await value.controller.close();
  assert.equal(value.selected(), null);
  assert.equal(value.root(), null);
});

test("a stale resize leaves the selection and needs no cleanup of its own", async () => {
  const value = boundary();
  await value.controller.select(first, size);
  const foreign = await value.host.presentation.select(
    value.surface,
    value.root()!,
  );
  await assert.rejects(
    value.controller.select(first, { ...size, width: 90 }),
    /staleView/,
  );
  assert.equal(value.selected(), foreign);
  assert.equal(value.controller.journal.unknownMutations.length, 0);
  assert.equal(value.controller.journal.bindings.length, 1);
  await value.controller.close();
  assert.equal(value.selected(), foreign);
});

test("late root receipt is cleaned without selecting after supersession", async () => {
  const value = boundary();
  const arrived = gate();
  const release = gate();
  const original = value.host.setRootOutput;
  value.host.setRootOutput = async (...args) => {
    const result = await original(...args);
    arrived.resolve();
    await release.promise;
    return result;
  };
  const stale = value.controller.select(first, size);
  await arrived.promise;
  const current = value.controller.select(second, size);
  release.resolve();
  await Promise.all([stale, current]);
  assert.equal(
    outputProducer(value.controller.view!.binding.output)?.entity,
    second.entity,
  );
  assert.equal(value.calls.filter((call) => call === "select").length, 1);
  assert.equal(value.controller.journal.bindings.length, 1);
  await value.controller.close();
});

test("late surface ACK after cancellation stays journaled until conditional cleanup", async () => {
  const value = boundary();
  const arrived = gate();
  const release = gate();
  const original = value.host.presentation.select;
  value.host.presentation.select = async (...args) => {
    const view = await original(...args);
    arrived.resolve();
    await release.promise;
    return view;
  };
  const pending = value.controller.select(first, size);
  await arrived.promise;
  const closing = value.controller.close();
  release.resolve();
  await Promise.all([pending, closing]);
  assert.equal(value.selected(), null);
  assert.equal(value.root(), null);
  assert.deepEqual(value.changed, []);
});

test("known rejection permits identical explicit retry without a second root receipt", async () => {
  const value = boundary();
  const original = value.host.presentation.select;
  value.host.presentation.select = async () => {
    throw new PresentationError("capacity");
  };
  await assert.rejects(value.controller.select(first, size));
  assert.equal(value.controller.journal.bindings.length, 1);
  value.host.presentation.select = original;
  await value.controller.select(first, size);
  assert.equal(value.calls.filter((call) => call === "bind").length, 1);
  await value.controller.close();
});

for (const retry of ["recover", "select"] as const) {
  test(`resized recovery root ACK survives select rejection and ${retry} retry`, async () => {
    const value = boundary();
    await value.controller.select(first, size);
    value.surface.context++;
    const resized = { ...size, width: 90 };
    await assert.rejects(
      value.controller.select(first, resized),
      /context changed/,
    );
    const original = value.host.presentation.select;
    value.host.presentation.select = async () => {
      throw new PresentationError("capacity");
    };
    await assert.rejects(value.controller.recover(), /capacity/);
    const acknowledged = value.root()!;
    assert.equal(acknowledged.viewport.width, 180);
    assert.equal(value.controller.journal.bindings.length, 2);
    value.host.presentation.select = original;
    if (retry === "recover") await value.controller.recover();
    else await value.controller.select(first, resized);
    assert.deepEqual(value.controller.view!.binding, acknowledged);
    assert.equal(value.calls.filter((call) => call === "bind").length, 2);
    assert.equal(value.controller.journal.bindings.length, 1);
    await value.controller.close();
    assert.equal(value.root(), null);
    assert.equal(value.selected(), null);
    assert.equal(value.controller.journal.bindings.length, 0);
  });
}

for (const unknown of [false, true]) {
  test(`failed recovery ${unknown ? "unknown mutation" : "foreign replacement"} remains fenced`, async () => {
    const value = boundary();
    await value.controller.select(first, size);
    value.surface.context++;
    await assert.rejects(
      value.controller.select(first, { ...size, width: 90 }),
    );
    let attempts = 0;
    value.host.presentation.select = async () => {
      attempts++;
      if (unknown) throw new Error("Submitted selection outcome is unknown");
      throw new PresentationError("capacity");
    };
    await assert.rejects(value.controller.recover());
    const foreign = unknown
      ? undefined
      : await value.host.setRootOutput(first, size);
    await assert.rejects(value.controller.recover());
    await assert.rejects(
      value.controller.select(first, { ...size, width: 90 }),
    );
    assert.equal(attempts, 1);
    if (unknown) {
      await assert.rejects(value.controller.close());
      assert.equal(value.controller.journal.unknownMutations.length, 1);
    } else {
      await value.controller.close();
      assert.equal(value.root(), foreign);
      assert.equal(value.controller.journal.bindings.length, 0);
    }
  });
}

test("unknown root mutation is neither retried nor declared cleaned", async () => {
  const value = boundary();
  let submitted = 0;
  value.host.setRootOutput = async () => {
    submitted++;
    throw new Error("No decoded outcome");
  };
  await assert.rejects(value.controller.select(first, size));
  await assert.rejects(value.controller.select(first, size));
  await assert.rejects(value.controller.select(second, size));
  await assert.rejects(value.controller.recover());
  await assert.rejects(value.controller.close());
  assert.equal(submitted, 1);
  assert.equal(value.controller.journal.unknownMutations.length, 1);
});

test("same-context recovery cannot reclaim a foreign selection with the same root", async () => {
  const value = boundary();
  await value.controller.select(first, size);
  const foreign = await value.host.presentation.select(
    value.surface,
    value.root()!,
  );
  await assert.rejects(value.controller.recover(), /fresh context/);
  await value.controller.close();
  assert.equal(value.selected(), foreign);
});

test("new context recovery still refuses a replaced root generation", async () => {
  const value = boundary();
  await value.controller.select(first, size);
  const foreign = await value.host.setRootOutput(first, size);
  value.surface.context++;
  await assert.rejects(value.controller.recover(), /replaced/);
  await value.controller.close();
  assert.equal(value.root(), foreign);
});

test("create options are forwarded unchanged and closing destroys no World", async () => {
  let options: unknown;
  let closed = 0;
  const destroyed: bigint[] = [];
  const host = {
    createWorld: async (value: unknown) => {
      options = value;
      return { reference: first.world };
    },
    openWorld: async () => {
      throw new Error("No Client opened");
    },
    destroyWorld: async (world: typeof first.world) => {
      destroyed.push(world.id);
    },
    close: async () => {
      closed++;
    },
  } as unknown as CanvasHost;
  const lifetime = new CanvasLifetime(host, true);
  await assert.rejects(
    lifetime.open({ create: { selectedSystems: [], temporary: true } }),
  );
  assert.deepEqual(options, { selectedSystems: [], temporary: true });
  await lifetime.close();
  // Like unmount, closing destroys no World; the owned Host still closes.
  assert.deepEqual(destroyed, []);
  assert.equal(closed, 1);
});

test("failed owned cleanup retains Host until exact retry or explicit abandonment", async () => {
  let closed = 0;
  let failing = true;
  const lifetime = new CanvasLifetime(
    {
      close: async () => {
        closed++;
      },
    } as unknown as CanvasHost,
    true,
  );
  lifetime.manage({
    close: async () => {
      if (failing) throw new Error("Held receipt query");
    },
    abandon: async () => {},
    journal: () => ({
      bindings: [],
      views: [],
      captures: [],
      unknownMutations: [],
    }),
  });
  const failure = await lifetime.close().catch((error: unknown) => error);
  assert.ok(failure instanceof CanvasCleanupError);
  assert.equal(closed, 0);
  failing = false;
  await failure.recovery.retry();
  assert.equal(closed, 1);
});
