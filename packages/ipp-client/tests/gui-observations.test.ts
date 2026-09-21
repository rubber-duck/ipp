/** subscribeGuiObservations delivery, filtering and teardown through ClientBase. */
import assert from "node:assert/strict";
import test from "node:test";
import { ClientBase } from "../src/client.js";
import type { WorldDescriptor } from "../src/host-protocol.js";
import type { MessageTransport, TransportEvents } from "../src/transport.js";
import type { Request, Response, ResponseBody } from "../src/types.js";
import type { GuiObservationBatch } from "../src/gui-types.js";

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
        session: "bigint:7",
        requestId: `bigint:${requestId.toString()}`,
        tick: "bigint:11",
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
    emit(body: ResponseBody, requestId = 0n): void {
      events?.message(encodeResponse(body, requestId));
    },
  };
}

const press = {
  kind: "buttonPressed",
  entity: "bigint:100",
  rootIncarnation: "bigint:3",
  node: 20,
  lifetime: 1,
  path: [10, 20],
  sourceTick: "bigint:11",
  effectTick: "bigint:12",
} as unknown as GuiObservationBatch["effects"][number];

test("observation chunks arrive as batches in emission order", () => {
  const { client, emit } = harness();
  const seen: GuiObservationBatch[] = [];
  const stop = client.subscribeGuiObservations(
    (batch) => void seen.push(batch),
  );
  emit({
    kind: "guiObservations",
    observations: {
      effects: [press],
      conflicts: [
        {
          session: 7n,
          sourceTick: 11n,
          effectTick: 12n,
          reason: { kind: "revisionMismatch", expected: 1, found: 2 },
        },
      ],
      cancellations: [],
    },
  });
  emit({
    kind: "guiUnhandledInputs",
    inputs: [
      {
        session: 7n,
        tick: 11n,
        input: { kind: "blur" },
        reason: { kind: "noFocus" },
      },
    ],
  });
  assert.equal(seen.length, 2);
  assert.equal(seen[0]!.effects.length, 1);
  assert.equal(seen[0]!.conflicts?.length, 1);
  assert.deepEqual(seen[0]!.unhandled, []);
  assert.deepEqual(seen[1]!.effects, []);
  assert.equal(seen[1]!.unhandled?.length, 1);
  stop();
  emit({
    kind: "guiObservations",
    observations: { effects: [press], conflicts: [], cancellations: [] },
  });
  assert.equal(seen.length, 2);
});

test("unhandled inputs from other sessions never reach subscribers", () => {
  const { client, emit } = harness();
  const seen: GuiObservationBatch[] = [];
  client.subscribeGuiObservations((batch) => void seen.push(batch));
  emit({
    kind: "guiUnhandledInputs",
    inputs: [
      {
        session: 8n,
        tick: 11n,
        input: { kind: "blur" },
        reason: { kind: "notOwner" },
      },
    ],
  });
  assert.equal(seen.length, 0);
  emit({
    kind: "guiUnhandledInputs",
    inputs: [
      {
        session: 8n,
        tick: 11n,
        input: { kind: "blur" },
        reason: { kind: "notOwner" },
      },
      {
        session: 7n,
        tick: 11n,
        input: { kind: "blur" },
        reason: { kind: "noFocus" },
      },
    ],
  });
  assert.equal(seen.length, 1);
  assert.equal(seen[0]!.unhandled?.length, 1);
  assert.equal(seen[0]!.unhandled?.[0]?.session, 7n);
});

test("correlated observation identities stop the session", () => {
  const { client, emit } = harness();
  const seen: GuiObservationBatch[] = [];
  client.subscribeGuiObservations((batch) => void seen.push(batch));
  emit(
    {
      kind: "guiObservations",
      observations: { effects: [], conflicts: [], cancellations: [] },
    },
    3n,
  );
  assert.deepEqual(seen, []);
  assert.throws(
    () => client.subscribeGuiObservations(() => {}),
    /Client is closed/,
  );
});

test("subscribing on a closed client throws", async () => {
  const { client } = harness();
  await client.close();
  assert.throws(
    () => client.subscribeGuiObservations(() => {}),
    /Client is closed/,
  );
});

test("a throwing initial text-focus replay is reported without retaining the listener", () => {
  const { client, emit } = harness();
  emit({
    kind: "guiObservations",
    observations: {
      effects: [],
      conflicts: [],
      cancellations: [],
      textFocus: {
        session: 7n,
        contextGeneration: 1n,
        focusGeneration: 2n,
        entity: 100n,
        rootIncarnation: 3n,
        node: 4,
        lifetime: 5,
        revision: 6,
        text: "ready",
        selectionStart: 5,
        selectionEnd: 5,
      },
    },
  });
  let calls = 0;
  const stop = client.subscribeGuiObservations(() => {
    calls += 1;
    throw new Error("replay failed");
  });
  assert.equal(calls, 1);
  emit({
    kind: "guiObservations",
    observations: { effects: [press], conflicts: [], cancellations: [] },
  });
  assert.equal(calls, 1);
  stop();
});
