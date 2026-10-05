/** CanvasWorld ownership, canvas state updates and root claims against fakes;
 * real runtime coverage is tests/react/canvas-world-case.ts. */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import {
  canvasOutput,
  type BatchOutcome,
  type Client,
  type Command,
  type OutputReference,
  type RootBinding,
  type SystemCommand,
  type WorldCreateOptions,
  type WorldReference,
} from "@ipp/client";
import { CanvasContext } from "../src/canvas-context.js";
import { CanvasWorldSession } from "../src/canvas-world-session.js";
import {
  CanvasWorld,
  validateCanvasWorld,
  type CanvasWorldHandle,
  type CanvasWorldProps,
} from "../src/canvas-world.js";
import {
  AttachedWorldSlot,
  describeAttachedWorld,
} from "../src/attached-world.js";
import type { CanvasHost } from "../src/canvas-presentation.js";
import { createRoot, Entity } from "../src/index.js";
import { World } from "../src/canvas-scope.js";

const CANVAS_SELECTION = ["ipp.canvas"] as const;
const size = { width: 80, height: 60, devicePixelRatio: 1 };

async function turns(count = 8): Promise<void> {
  for (let index = 0; index < count; index++)
    await new Promise<void>((resolve) => setImmediate(resolve));
}

/** A World session that acknowledges every batch and records System commands. */
class FakeWorldClient {
  readonly batches: Command[][] = [];
  readonly commands: SystemCommand[] = [];
  closure: undefined | { reason: Error } = undefined;
  closedCount = 0;
  readonly components = {};
  readonly schemaHash = 1n;
  readonly closed = new Promise<never>(() => {});
  private nextEntity = 100n;
  rejectNext = false;

  constructor(
    readonly session: bigint,
    readonly worldReference: WorldReference,
  ) {}

  async batch(operations: Command[]): Promise<BatchOutcome> {
    this.batches.push(operations);
    if (this.rejectNext) {
      this.rejectNext = false;
      return {
        ok: false,
        batchId: BigInt(this.batches.length),
        tick: 1n,
        aliases: [],
        symbols: [],
        effects: [],
        error: { scope: "operation", operation: 0, reason: "InvalidValue" },
      };
    }
    return {
      ok: true,
      batchId: BigInt(this.batches.length),
      tick: 1n,
      aliases: operations.flatMap((operation) =>
        operation.kind === "create"
          ? [{ alias: operation.alias, id: this.nextEntity++ }]
          : [],
      ),
      symbols: [],
      effects: [],
    };
  }

  sendCommand(command: SystemCommand): void {
    this.commands.push(structuredClone(command));
  }

  async close(): Promise<void> {
    this.closedCount++;
  }
}

/** A Host whose Worlds, sessions and root presentation the test observes. */
function fakeHost() {
  const sessions = new Map<bigint, FakeWorldClient>();
  const created: WorldCreateOptions[] = [];
  const destroyed: WorldReference[] = [];
  const roots: (OutputReference | null)[] = [];
  let binding: RootBinding | null = null;
  let serial = 0n;
  let nextWorld = 10n;
  const canvas = new FakeWorldClient(1n, { id: 1n, incarnation: 1n });
  sessions.set(canvas.session, canvas);
  const host = {
    sessions,
    async createWorld(options: WorldCreateOptions) {
      created.push(structuredClone(options));
      return { reference: { id: nextWorld++, incarnation: 1n } };
    },
    async openWorld(world: WorldReference) {
      const client = new FakeWorldClient(BigInt(sessions.size + 1), world);
      sessions.set(client.session, client);
      return client;
    },
    async destroyWorld(world: WorldReference) {
      destroyed.push(world);
    },
    resolveOutput: async (output: OutputReference) => output,
    getRootOutputBinding: async () => binding,
    async setRootOutput(output: OutputReference, viewport: typeof size) {
      roots.push(output);
      binding = {
        output,
        viewport,
        generation: { host: 1n, serial: ++serial },
      };
      return binding;
    },
    async clearRootOutput(expected: RootBinding) {
      roots.push(null);
      if (binding?.generation.serial === expected.generation.serial)
        binding = null;
    },
    reads: { release: async () => {} },
    presentation: {
      surface: async () => ({
        id: 1n,
        context: 1n,
        maxWidth: 200,
        maxHeight: 200,
      }),
      select: async (
        surface: { id: bigint; context: bigint },
        selected: RootBinding,
      ) => ({ surface, binding: structuredClone(selected), selection: 1n }),
      clear: async () => {},
    },
  };
  return {
    host: host as unknown as CanvasHost,
    canvas,
    sessions,
    created,
    destroyed,
    roots,
  };
}

function session(boundary: ReturnType<typeof fakeHost>) {
  const errors: Error[] = [];
  const value = new CanvasWorldSession({
    host: boundary.host,
    client: boundary.canvas as unknown as Client,
    onError: (error) => errors.push(error),
  });
  return { session: value, errors };
}

test("CanvasWorld validates its selection, state and presentation before creating", () => {
  const valid: CanvasWorldProps = {
    create: { selectedSystems: CANVAS_SELECTION },
    presentation: { root: true },
  };
  validateCanvasWorld(valid);
  validateCanvasWorld({ ...valid, presentation: { anchor: "panel" } });
  assert.throws(
    () =>
      validateCanvasWorld({
        ...valid,
        create: { selectedSystems: ["ipp.gui"] },
      }),
    /selects ipp\.canvas/,
  );
  assert.throws(
    () =>
      validateCanvasWorld({
        ...valid,
        create: {
          selectedSystems: CANVAS_SELECTION,
          canvas: { extent: [1, 1], unitsPerMetre: 1 },
        } as CanvasWorldProps["create"],
      }),
    /extent and unitsPerMetre/,
  );
  for (const extent of [
    [0, 1],
    [1, Number.NaN],
    [1, Number.POSITIVE_INFINITY],
  ] as const)
    assert.throws(() => validateCanvasWorld({ ...valid, extent }), RangeError);
  assert.throws(
    () => validateCanvasWorld({ ...valid, unitsPerMetre: -1 }),
    RangeError,
  );
  assert.throws(
    () =>
      validateCanvasWorld({
        ...valid,
        presentation: { root: true, anchor: "panel" } as never,
      }),
    /root or at one anchor/,
  );
});

test("SurfaceCanvas attachments name no output and canvas state is not their identity", () => {
  const props = {
    slot: new AttachedWorldSlot(),
    anchor: "panel",
    child: { create: { selectedSystems: CANVAS_SELECTION } },
    attachment: { mode: "surface-canvas" },
  };
  const plain = describeAttachedWorld(props);
  assert.equal(plain.canvas, undefined);
  const sized = describeAttachedWorld({
    ...props,
    canvas: { extent: [320, 200], unitsPerMetre: 2 },
  });
  assert.deepEqual(sized.canvas, { extent: [320, 200], unitsPerMetre: 2 });
  assert.equal(sized.signature, plain.signature);
  assert.throws(
    () =>
      describeAttachedWorld({
        ...props,
        attachment: { mode: "surface-canvas", output: { entity: "panel" } },
      }),
    /Only SurfaceCamera attachments name an output/,
  );
  assert.throws(
    () =>
      describeAttachedWorld({
        ...props,
        attachment: { mode: "surface-camera" },
      }),
    /require an explicit camera/,
  );
  assert.throws(
    () =>
      describeAttachedWorld({
        ...props,
        child: { borrow: { id: 1n, incarnation: 1n } },
        canvas: { extent: [1, 1] },
      }),
    /Only a created child World takes canvas state/,
  );
});

test("a root CanvasWorld creates its World with its canvas state, claims the root and sends only changed values", async () => {
  const boundary = fakeHost();
  const { session: canvas, errors } = session(boundary);
  await canvas.selectOutput(undefined, size);
  const ready: WorldReference[] = [];
  const owned = canvas.openCanvasWorld(
    { selectedSystems: CANVAS_SELECTION },
    { extent: [320, 200] },
    (error) => errors.push(error),
    (world) => ready.push(world),
  );
  owned.render(createElement(Entity, { id: "panel" }));
  await turns();
  assert.deepEqual(boundary.created, [
    {
      selectedSystems: [...CANVAS_SELECTION],
      canvas: { extent: [320, 200], unitsPerMetre: 1 },
    },
  ]);
  const world = ready[0]!;
  assert.ok(world);
  const client = [...boundary.sessions.values()].find(
    (candidate) => candidate.worldReference.id === world.id,
  )!;
  assert.deepEqual(
    client.batches
      .flat()
      .map((command) =>
        command.kind === "create" ? command.metadata.symbolicId : command.kind,
      ),
    ["panel"],
    "declarations author the owned World",
  );
  assert.deepEqual(boundary.canvas.batches, []);
  assert.deepEqual(boundary.roots, [canvasOutput(world)]);

  owned.update({ extent: [320, 200] });
  owned.update({});
  assert.deepEqual(
    client.commands,
    [],
    "unchanged or omitted values send nothing",
  );
  owned.update({ extent: [320, 200], unitsPerMetre: 2 });
  owned.update({ extent: [160, 100], unitsPerMetre: 2 });
  owned.update({ unitsPerMetre: 2 });
  assert.deepEqual(client.commands, [
    { type: "CanvasStateUpdateCommand", unitsPerMetre: 2 },
    { type: "CanvasStateUpdateCommand", extent: [160, 100] },
  ]);

  await assert.rejects(
    canvas.selectOutput(null, size),
    /explicit output while a CanvasWorld presents the root/,
  );
  const second = canvas.openCanvasWorld(
    { selectedSystems: CANVAS_SELECTION },
    {},
    (error) => errors.push(error),
    () => assert.fail("a second root claim must not become ready"),
  );
  await turns();
  assert.match(errors.at(-1)?.message ?? "", /already presents the root/);
  await second.remove();
  errors.length = 0;

  await canvas.selectOutput(undefined, size);
  await owned.remove();
  await owned.closed;
  await turns();
  assert.equal(client.closedCount, 1);
  assert.deepEqual(boundary.destroyed.at(-1), world);
  assert.equal(boundary.roots.at(-1), null, "removal clears the root");
  assert.deepEqual(errors, []);
  await canvas.close();
});

test("closing the Canvas releases a root CanvasWorld without destroying its World", async () => {
  const boundary = fakeHost();
  const { session: canvas, errors } = session(boundary);
  const handles: CanvasWorldHandle[] = [];
  const parent = createRoot(boundary.canvas as unknown as Client);
  await parent.render(
    createElement(
      CanvasContext,
      { value: canvas },
      createElement(CanvasWorld, {
        create: { selectedSystems: CANVAS_SELECTION },
        unitsPerMetre: 4,
        presentation: { root: true },
        onReady: (handle) => handles.push(handle),
      }),
    ),
  );
  await turns();
  const handle = handles[0]!;
  assert.ok(handle);
  assert.deepEqual(handle.output, canvasOutput(handle.world));
  assert.deepEqual(boundary.created[0]?.canvas, {
    extent: [1, 1],
    unitsPerMetre: 4,
  });
  await canvas.close();
  await handle.closed;
  assert.deepEqual(boundary.destroyed, []);
  const client = [...boundary.sessions.values()].find(
    (candidate) => candidate.worldReference.id === handle.world.id,
  )!;
  assert.equal(client.closedCount, 1);
  await parent.unmount();
  assert.deepEqual(errors, []);
});

for (const kind of ["World", "CanvasWorld"] as const) {
  for (const handler of ["default", "explicit", "throwing"] as const) {
    test(`${kind} reports a rejected declaration through ${handler} observation and accepts correction`, async () => {
      const boundary = fakeHost();
      const declarations: Error[] = [];
      const lifecycle: Error[] = [];
      const explicit: Error[] = [];
      const canvas = new CanvasWorldSession({
        host: boundary.host,
        client: boundary.canvas as unknown as Client,
        onError: (error) => lifecycle.push(error),
        onDeclarationError: (error) => declarations.push(error),
      });
      const parentErrors: Error[] = [];
      const parent = createRoot(boundary.canvas as unknown as Client, {
        onError: (error) => parentErrors.push(error),
      });
      let committed = 0;
      let ready: CanvasWorldHandle | undefined;
      const errorHandler =
        handler === "default"
          ? undefined
          : (error: Error) => {
              explicit.push(error);
              if (handler === "throwing")
                throw new Error("Error observer failed");
            };
      const tree = (name: string) =>
        createElement(
          CanvasContext,
          { value: canvas },
          kind === "World"
            ? createElement(
                World,
                {
                  onCommit: () => {
                    committed++;
                  },
                  ...(errorHandler ? { onError: errorHandler } : {}),
                },
                createElement(Entity, { id: name }),
              )
            : createElement(
                CanvasWorld,
                {
                  create: { selectedSystems: CANVAS_SELECTION },
                  presentation: { root: true },
                  onReady: (handle) => {
                    ready = handle;
                  },
                  ...(errorHandler ? { onError: errorHandler } : {}),
                },
                createElement(Entity, { id: name }),
              ),
        );
      try {
        await parent.render(tree("initial"));
        await turns();
        await canvas.flush();
        const authored =
          kind === "World"
            ? boundary.canvas
            : [...boundary.sessions.values()].find(
                (client) => client.worldReference.id === ready!.world.id,
              )!;
        authored.rejectNext = true;
        await parent.render(tree("rejected"));
        await turns();
        await assert.rejects(canvas.flush(), /InvalidValue/);
        const batches = authored.batches.length;
        await assert.rejects(canvas.flush(), /InvalidValue/);
        assert.equal(
          authored.batches.length,
          batches,
          "flush cannot retry the invalid declaration",
        );
        if (handler === "default")
          assert.match(declarations.at(-1)?.message ?? "", /InvalidValue/);
        else assert.match(explicit.at(-1)?.message ?? "", /InvalidValue/);
        if (handler === "throwing")
          assert.match(declarations.at(-1)?.message ?? "", /observer failed/);
        assert.deepEqual(lifecycle, []);
        assert.deepEqual(parentErrors, []);
        await parent.render(tree("corrected"));
        await turns();
        await canvas.flush();
        assert.ok(authored.batches.length > batches);
        if (kind === "World") assert.equal(committed, 2);
        assert.equal(
          boundary.created.length,
          kind === "World" ? 0 : 1,
          "correction keeps the owned World",
        );
        assert.deepEqual(boundary.destroyed, []);
      } finally {
        await parent.unmount();
        await canvas.close();
      }
    });
  }
}

test("Canvas presentation failures retain their lifecycle observer", async () => {
  const boundary = fakeHost();
  const lifecycle: Error[] = [];
  const declarations: Error[] = [];
  const canvas = new CanvasWorldSession({
    host: boundary.host,
    client: boundary.canvas as unknown as Client,
    onError: (error) => lifecycle.push(error),
    onDeclarationError: (error) => declarations.push(error),
  });
  try {
    await canvas.selectOutput(null, size);
    const release = canvas.claimRoot(
      canvasOutput(boundary.canvas.worldReference),
    );
    await turns();
    await canvas.selectOutput(null, size).catch(() => {});
    release();
    await turns();
    assert.match(lifecycle[0]?.message ?? "", /explicit output/);
    assert.deepEqual(declarations, []);
  } finally {
    await canvas.close();
  }
});
