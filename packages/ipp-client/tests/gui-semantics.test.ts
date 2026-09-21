/** submitGuiSemanticSnapshot/Action correlation and rejection mapping through ClientBase. */
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
  GuiSemanticActionRequest,
  GuiSemanticSnapshotQuery,
  GuiSemanticTree,
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

  snapshot(query: GuiSemanticSnapshotQuery): Promise<GuiSemanticTree> {
    return this.submitGuiSemanticSnapshot(query);
  }

  act(action: GuiSemanticActionRequest): Promise<void> {
    return this.submitGuiSemanticAction(action);
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
    reply(body: ResponseBody, requestId: bigint): void {
      events?.message(encodeResponse(body, requestId));
    },
  };
}

const tree: GuiSemanticTree = {
  entity: 42n,
  rootIncarnation: 3n,
  evaluationTick: 12n,
  nodes: [
    {
      id: 2,
      lifetime: 1,
      role: "button",
      value: { kind: "none" },
      revision: 0,
      bounds: [1, 1, 2, 1],
      enabled: true,
      visible: true,
      available: true,
      actions: ["press"],
    },
  ],
  focused: { id: 2, lifetime: 1 },
};

test("semantic snapshots submit correlated queries and resolve trees", async () => {
  const { client, sent, reply } = harness();
  const pending = client.snapshot({ entity: 42n });
  assert.equal(sent.length, 1);
  assert.equal(sent[0]!.body.kind, "guiSemanticSnapshot");
  assert.ok(sent[0]!.requestId !== 0n);
  assert.equal(sent[0]!.session, 7n);
  reply({ kind: "guiSemanticSnapshot", snapshot: tree }, sent[0]!.requestId);
  assert.deepEqual(await pending, tree);
});

test("semantic snapshot replies reject on mismatch and map host errors", async () => {
  const { client, sent, reply } = harness();
  const pending = client.snapshot({ entity: 42n });
  reply({ kind: "guiInput", outcome: { tick: 11n } }, sent[0]!.requestId);
  await assert.rejects(
    pending,
    /Invalid GUI semantic snapshot response correlation/,
  );

  const rejected = client.snapshot({ entity: 42n });
  reply(
    { kind: "error", code: 1, message: "semantic action unknown node 9" },
    sent[1]!.requestId,
  );
  await assert.rejects(rejected, RequestRejectedError);
});

test("semantic actions accept control and input admissions", async () => {
  const { client, sent, reply } = harness();
  const action: GuiSemanticActionRequest = {
    entity: 42n,
    rootIncarnation: 3n,
    node: 2,
    lifetime: 1,
    expectedRevision: 0,
    action: { kind: "press" },
  };
  const first = client.act(action);
  const second = client.act(action);
  assert.equal(sent.length, 2);
  assert.equal(sent[0]!.body.kind, "guiSemanticAction");
  assert.equal(sent[1]!.body.kind, "guiSemanticAction");
  assert.ok(sent[0]!.requestId !== sent[1]!.requestId);
  reply({ kind: "guiInput", outcome: { tick: 11n } }, sent[0]!.requestId);
  reply(
    { kind: "gui", outcome: { ok: true, applied: 1, requests: 1 } },
    sent[1]!.requestId,
  );
  await first;
  await second;

  const mismatch = client.act(action);
  reply({ kind: "guiSemanticSnapshot", snapshot: tree }, sent[2]!.requestId);
  await assert.rejects(
    mismatch,
    /Invalid GUI semantic action response correlation/,
  );

  const rejected = client.act(action);
  reply(
    { kind: "error", code: 1, message: "semantic action stale revision" },
    sent[3]!.requestId,
  );
  await assert.rejects(rejected, RequestRejectedError);
});

test("local semantic encoding failures never reach the transport", async () => {
  const { client, sent } = harness();
  const circular: { entity: bigint; self?: unknown } = { entity: 42n };
  circular.self = circular;
  await assert.rejects(
    client.snapshot(circular as unknown as GuiSemanticSnapshotQuery),
    RequestNotSentError,
  );
  assert.equal(sent.length, 0);
});
