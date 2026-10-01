import test from "node:test";
import assert from "node:assert/strict";
import { HostPhysicalInput, GuiPhysicalRejection } from "../src/host-input.js";
import { HostWireReader, HostWireWriter } from "../src/host-protocol.js";
import type { PresentationView } from "../src/host-presentation.js";
import { physicalWireTag } from "./physical-wire-fixture.js";

const view: PresentationView = {
  surface: { id: 1n, context: 1n, maxWidth: 100, maxHeight: 100 },
  selection: 1n,
  binding: {
    output: { kind: "canvas", world: { id: 1n, incarnation: 1n } },
    viewport: { width: 100, height: 100, devicePixelRatio: 1 },
    generation: { host: 1n, serial: 1n },
  },
};

function response(write: (writer: HostWireWriter) => void): Uint8Array {
  const payload = new HostWireWriter();
  write(payload);
  const envelope = new HostWireWriter();
  envelope.u8(19);
  const bytes = payload.finish();
  envelope.u32(bytes.length);
  envelope.raw(bytes);
  return envelope.finish();
}

/** Physical input bounds of the maintained GUI target contract. */
function physicalLimit(name: string): number {
  const limits: Record<string, number> = {
    GUI_PHYSICAL_POINTERS: 32,
    GUI_PHYSICAL_BLOCKERS: 32768,
  };
  return limits[name] ?? assert.fail(`unknown physical limit ${name}`);
}

function harness() {
  let afterAccept = () => {};
  let reject = false;
  let next = 0n;
  const kinds: number[] = [];
  const input = new HostPhysicalInput(
    async (_tag, encode, accept) => {
      const writer = new HostWireWriter();
      encode(writer);
      const reader = new HostWireReader(writer.finish());
      const payload = new HostWireReader(reader.raw(reader.u32()));
      const kind = payload.u8();
      kinds.push(kind);
      const bytes = response((reply) => {
        if (reject) {
          reply.u8(3);
          reply.string("StalePath");
        } else if (kind === 0) {
          reply.u8(0);
          reply.u64(++next);
        } else if (kind === 1) reply.u8(1);
        else {
          reply.u8(2);
          reply.u8(0);
          reply.u32(1);
          reply.u32(0);
          reply.u32(0);
          reply.u8(0);
          reply.u8(0);
          reply.u8(0);
        }
      });
      accept(new HostWireReader(bytes));
      afterAccept();
      return new HostWireReader(bytes);
    },
    physicalWireTag,
    physicalLimit,
  );
  return {
    input,
    kinds,
    after: (callback: () => void) => {
      afterAccept = callback;
    },
    reject: (value: boolean) => {
      reject = value;
    },
  };
}

test("same-stack Open ACK then revocation never exposes a live physical context", async () => {
  const run = harness();
  run.after(() =>
    run.input.notification(
      new HostWireReader(
        response((writer) => {
          writer.u8(4);
          writer.u64(1n);
        }),
      ),
    ),
  );
  await assert.rejects(run.input.open(view), /closed during acquisition/);
  assert.deepEqual(run.kinds, [0]);
});

test("revocation fences its exact generation and close fences before the ACK", async () => {
  const run = harness();
  const old = await run.input.open(view);
  const current = await run.input.open(view);
  run.input.notification(
    new HostWireReader(
      response((writer) => {
        writer.u8(4);
        writer.u64(old.identity);
      }),
    ),
  );
  assert.equal(old.isClosed, true);
  assert.equal(current.isClosed, false);
  await assert.rejects(old.send({ kind: "blur" }), /revoked/);
  const closed = current.close();
  assert.equal(current.isClosed, true);
  await closed;
  await current.close();
  assert.deepEqual(run.kinds, [0, 0, 1]);
});

test("known gate rejection preserves the context and sends preserve call order", async () => {
  const run = harness();
  const context = await run.input.open(view);
  run.reject(true);
  await assert.rejects(
    context.send({ kind: "key", key: "enter" }),
    GuiPhysicalRejection,
  );
  assert.equal(context.isClosed, false);
  run.reject(false);
  const first = context.send({
    kind: "pointerDown",
    pointer: 1n,
    point: [0.1, 0.1],
  });
  const second = context.send({
    kind: "pointerUp",
    pointer: 1n,
    point: [0.1, 0.1],
  });
  assert.deepEqual(run.kinds, [0, 2, 2, 2]);
  assert.equal((await first).applied, 1);
  assert.equal((await second).applied, 1);
});

test("native cancellation is synchronous and scoped to the exact live context", async () => {
  const run = harness();
  const context = await run.input.open(view);
  const received: bigint[] = [];
  context.onCancel((event) => received.push(...event.pointers));
  const notification = (identity: bigint) =>
    new HostWireReader(
      response((writer) => {
        writer.u8(5);
        writer.u64(identity);
        writer.u8(1);
        writer.u64(7n);
        writer.u8(0);
      }),
    );
  run.input.notification(notification(context.identity + 1n));
  assert.deepEqual(received, []);
  run.input.notification(notification(context.identity));
  assert.deepEqual(received, [7n]);
  assert.equal(context.isClosed, false);
  await context.close();
  run.input.notification(notification(context.identity));
  assert.deepEqual(received, [7n]);
});

test("native close listeners observe terminal state before reentrant sends", async () => {
  const run = harness();
  const context = await run.input.open(view);
  context.observeText({
    fence: {
      target: {
        world: view.binding.output.world,
        entity: 4n,
        component: 8,
        incarnation: 1n,
      },
      generation: 1n,
    },
    text: "owned",
    selectionStart: 5,
    selectionEnd: 5,
  });
  const late: Promise<unknown>[] = [];
  context.onText((state) => {
    if (state !== null) return;
    assert.equal(context.isClosed, true);
    late.push(
      assert.rejects(context.send({ kind: "key", key: "enter" }), /closed/),
    );
  });
  await context.close();
  await Promise.all(late);
});
