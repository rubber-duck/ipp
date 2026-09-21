/** submitGuiInput correlation, ordering and rejection mapping through ClientBase. */
import assert from "node:assert/strict";
import test from "node:test";
import {
  ClientBase,
  RequestNotSentError,
  RequestRejectedError,
} from "../src/client.js";
import type { WorldDescriptor } from "../src/host-protocol.js";
import type { MessageTransport, TransportEvents } from "../src/transport.js";
import type { Request, Response, ResponseBody } from "../src/types.js";
import type {
  GuiEdit,
  GuiInputCommand,
  GuiInputRoutingOutcome,
} from "../src/gui-types.js";

const text = new TextDecoder();
const bytes = new TextEncoder();

function encodeRequest(request: Request): Uint8Array<ArrayBuffer> {
  return bytes.encode(
    JSON.stringify(request, (_key, value: unknown) =>
      typeof value === "bigint" ? `bigint:${value.toString()}` : value,
    ),
  );
}

function decodeRequest(sent: Uint8Array): Request {
  return JSON.parse(text.decode(sent), (_key, value: unknown) =>
    typeof value === "string" && value.startsWith("bigint:")
      ? BigInt(value.slice(7))
      : value,
  );
}

function encodeResponse(body: ResponseBody, requestId: bigint): Uint8Array {
  return bytes.encode(
    JSON.stringify(
      {
        session: 7n,
        requestId,
        tick: 11n,
        body,
      },
      (_key, value: unknown) =>
        typeof value === "bigint" ? `bigint:${value.toString()}` : value,
    ),
  );
}

class HarnessClient extends ClientBase {
  override readonly schemaHash = 1n;
  override readonly components = {};
  override readonly capabilities = {
    stateOverlays: true,
    spatial: true,
    textures: false,
    builtinAssets: false,
    picking: false,
    debugGeometry: false,
    pbr: false,
    shadows: false,
    skeletalAnimation: false,
    meshPoses: false,
  };

  constructor(transport: MessageTransport) {
    super(transport, { timeoutMs: 1000, logLevel: "off" });
  }

  attach(): void {
    this.initializeAttached(7n, {} as WorldDescriptor);
  }

  sendInput(input: GuiInputCommand): Promise<GuiInputRoutingOutcome> {
    return this.submitGuiInput(input);
  }

  holdAutomatic(completion: Promise<void>): Promise<void> {
    return this.queueAutomaticWorldBatch(false, () => completion);
  }

  automaticGui(edits: readonly GuiEdit[]) {
    return this.queueAutomaticWorldBatch(true, (operation) =>
      this.submitGui(edits, undefined, operation),
    );
  }

  protected override bootstrap(): Uint8Array<ArrayBuffer> {
    return new Uint8Array();
  }

  protected override acceptBootstrap(): bigint {
    return 7n;
  }

  protected override encodeRequest(request: Request): Uint8Array<ArrayBuffer> {
    return encodeRequest(request);
  }

  protected override decodeResponse(
    message: Uint8Array,
    session: bigint,
  ): Response {
    const response = JSON.parse(text.decode(message), (_key, value: unknown) =>
      typeof value === "string" && value.startsWith("bigint:")
        ? BigInt(value.slice(7))
        : value,
    );
    assert.equal(response.session, session);
    return response;
  }
}

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

function flushTasks(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve));
}

const removeEdit: GuiEdit = {
  action: "remove",
  handle: {
    session: 7n,
    entity: 1n,
    rootIncarnation: 1n,
    nodeId: 1,
    nodeLifetime: 1,
  },
};

function harness() {
  const sent: Request[] = [];
  let events: TransportEvents | undefined;
  const transport: MessageTransport = {
    start(next) {
      events = next;
      next.ready();
    },
    send(message) {
      sent.push(decodeRequest(message));
    },
    async close() {},
  };
  const client = new HarnessClient(transport);
  client.attach();
  return {
    client,
    sent,
    reply(body: ResponseBody, requestId: bigint): void {
      events?.message(encodeResponse(body, requestId));
    },
  };
}

test("gui inputs submit correlated requests in call order", async () => {
  const { client, sent, reply } = harness();
  const first = client.sendInput({
    kind: "pointerDown",
    pointer: 1,
    position: [2, 1.5],
    button: "primary",
  });
  const second = client.sendInput({ kind: "text", text: "hi" });
  assert.equal(sent.length, 2);
  assert.equal(sent[0]!.body.kind, "guiInput");
  assert.equal(sent[1]!.body.kind, "guiInput");
  assert.ok(sent[0]!.requestId !== 0n && sent[1]!.requestId !== 0n);
  assert.ok(sent[0]!.requestId !== sent[1]!.requestId);
  assert.equal(sent[0]!.session, 7n);
  reply(
    {
      kind: "guiInput",
      outcome: { tick: 12n, unhandled: { kind: "noPanelHit" } },
    },
    sent[1]!.requestId,
  );
  reply({ kind: "guiInput", outcome: { tick: 11n } }, sent[0]!.requestId);
  assert.deepEqual(await first, { tick: 11n });
  assert.deepEqual(await second, {
    tick: 12n,
    unhandled: { kind: "noPanelHit" },
  });
});

test("gui input replies reject on mismatch and map host errors", async () => {
  const { client, sent, reply } = harness();
  const pending = client.sendInput({ kind: "blur" });
  reply(
    { kind: "gui", outcome: { ok: true, applied: 1, requests: 1 } },
    sent[0]!.requestId,
  );
  await assert.rejects(pending, /Invalid GUI input response correlation/);

  const rejected = client.sendInput({ kind: "blur" });
  reply({ kind: "error", code: 1, message: "rejected" }, sent[1]!.requestId);
  await assert.rejects(rejected, RequestRejectedError);

  const failed = client.sendInput({ kind: "blur" });
  reply({ kind: "error", code: 3, message: "unavailable" }, sent[2]!.requestId);
  await assert.rejects(failed, /Host 3: unavailable/);
});

test("local encoding failures never reach the transport", async () => {
  const { client, sent } = harness();
  const circular: { kind: string; self?: unknown } = { kind: "text" };
  circular.self = circular;
  await assert.rejects(
    client.sendInput(circular as unknown as GuiInputCommand),
    RequestNotSentError,
  );
  assert.equal(sent.length, 0);
});

test("GUI input bypasses pending automatic World work", async () => {
  const { client, sent, reply } = harness();
  const firstRelease = deferred();
  const first = client.holdAutomatic(firstRelease.promise);
  const queuedGui = client.automaticGui([removeEdit]);
  const ordinary = client.sendInput({ kind: "blur" });
  assert.equal(sent.length, 1);
  assert.equal(sent[0]!.body.kind, "guiInput");
  reply({ kind: "guiInput", outcome: { tick: 11n } }, sent[0]!.requestId);
  await ordinary;

  firstRelease.resolve();
  await first;
  await flushTasks();
  assert.equal(sent.length, 2);
  assert.equal(sent[1]!.body.kind, "gui");
  reply(
    { kind: "gui", outcome: { ok: true, applied: 1, requests: 1 } },
    sent[1]!.requestId,
  );
  assert.deepEqual(await queuedGui, { ok: true, applied: 1, requests: 1 });

  const failedRelease = deferred();
  const heldFailure = client.holdAutomatic(failedRelease.promise);
  const failedGui = client.automaticGui([removeEdit]);
  const afterFailure = client.sendInput({ kind: "blur" });
  assert.equal(sent.length, 3);
  assert.equal(sent[2]!.body.kind, "guiInput");
  reply({ kind: "guiInput", outcome: { tick: 12n } }, sent[2]!.requestId);
  await afterFailure;
  failedRelease.resolve();
  await heldFailure;
  await flushTasks();
  assert.equal(sent.length, 4);
  assert.equal(sent[3]!.body.kind, "gui");
  reply(
    { kind: "error", code: 1, message: "deterministic GUI rejection" },
    sent[3]!.requestId,
  );
  await assert.rejects(failedGui, RequestRejectedError);

  await flushTasks();
  const reused = client.automaticGui([removeEdit]);
  assert.equal(sent.length, 5);
  assert.equal(sent[4]!.body.kind, "gui");
  reply(
    { kind: "gui", outcome: { ok: true, applied: 1, requests: 1 } },
    sent[4]!.requestId,
  );
  assert.deepEqual(await reused, { ok: true, applied: 1, requests: 1 });
});
