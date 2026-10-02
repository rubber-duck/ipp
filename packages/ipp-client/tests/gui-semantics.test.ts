import assert from "node:assert/strict";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { MessageChannel } from "node:worker_threads";
import { ClientBase, RequestRejectedError } from "../src/client.js";
import type {
  GuiObservedEffect,
  GuiObservationOptions,
} from "../src/gui-types.js";
import {
  type MessageTransport,
  PortTransport,
  type TransportEvents,
} from "../src/transport.js";
import type { Command, Request, Response, ResponseBody } from "../src/types.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const world = { id: 3n, incarnation: 4n };
const target = { world, entity: 42n, component: 51, incarnation: 6n };
/** A press of `target`: one `guiAction` command. */
const press: Command = {
  kind: "guiAction",
  entity: { kind: "handle", id: target.entity },
  component: target.component,
  incarnation: target.incarnation,
  action: { kind: "press" },
};
/** An ordinary correlated request answered independently of GUI terminals. */
const inspected: ResponseBody = {
  kind: "inspect",
  next: 0n,
  time: 0,
  entities: [],
  resources: [],
  renderDiagnostics: [],
};

function encode(value: unknown): Uint8Array<ArrayBuffer> {
  return encoder.encode(
    JSON.stringify(value, (_key, item: unknown) =>
      typeof item === "bigint" ? `bigint:${item}` : item,
    ),
  );
}

function decode(bytes: Uint8Array): unknown {
  return JSON.parse(decoder.decode(bytes), (_key, item: unknown) =>
    typeof item === "string" && item.startsWith("bigint:")
      ? BigInt(item.slice(7))
      : item,
  );
}

/** Marks a batch page: a fixed header ending in its command count, then its commands. */
const PAGE_MARK = 0xff;

/**
 * JSON requests, except batch pages, which have the counted-header layout the
 * client assembles pages from: each command is a length-prefixed JSON value.
 */
function encodeRequest(request: Request): Uint8Array<ArrayBuffer> {
  if (request.body.kind !== "submitBatch") return encode(request);
  const commands = request.body.operations.map(encode);
  const bytes = new Uint8Array(
    26 + commands.reduce((total, command) => total + 4 + command.length, 0),
  );
  const view = new DataView(bytes.buffer);
  bytes[0] = PAGE_MARK;
  view.setBigUint64(1, request.session, true);
  view.setBigUint64(9, request.requestId, true);
  view.setUint32(17, request.body.batchId, true);
  bytes[21] = request.body.last ? 1 : 0;
  view.setUint32(22, commands.length, true);
  let at = 26;
  for (const command of commands) {
    view.setUint32(at, command.length, true);
    bytes.set(command, at + 4);
    at += 4 + command.length;
  }
  return bytes;
}

function decodeRequest(bytes: Uint8Array): Request {
  if (bytes[0] !== PAGE_MARK) return decode(bytes) as Request;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const operations: Command[] = [];
  let at = 26;
  for (let index = 0; index < view.getUint32(22, true); index++) {
    const length = view.getUint32(at, true);
    operations.push(decode(bytes.subarray(at + 4, at + 4 + length)) as Command);
    at += 4 + length;
  }
  return {
    session: view.getBigUint64(1, true),
    requestId: view.getBigUint64(9, true),
    body: {
      kind: "submitBatch",
      batchId: view.getUint32(17, true),
      last: bytes[21] === 1,
      operations,
    },
  };
}

/** Small pages so paging is exercised without large fixtures. */
const PAGE_LIMITS = { commands: 8, bytes: 64 * 1024 } as const;

class HarnessClient extends ClientBase {
  protected readonly lifecyclePageMembers = 1;
  protected readonly commandPageLimits = PAGE_LIMITS;
  override readonly schemaHash = 1n;
  override readonly components = {};

  constructor(transport: MessageTransport, session: bigint, timeoutMs = 2000) {
    super(transport, { timeoutMs, logLevel: "off" });
    this.initializeAttached(
      session,
      {
        id: world.id,
        symbolicId: "gui",
        persistentId: 1n,
        capacityHints: { entities: 0, systems: {} },
      },
      {
        systems: ["ipp.canvas", "ipp.gui"],
        components: [target.component],
        operations: ["gui"],
      },
      world,
    );
  }

  ordinary() {
    return this.inspectPage();
  }

  /** Submit one GUI action as its own batch. */
  act(action: Command) {
    return this.batch([action]);
  }

  subscribe(
    listener: (effect: GuiObservedEffect) => void,
    options?: GuiObservationOptions,
  ) {
    return this.submitGuiSubscription(listener, options);
  }

  protected override encodeRequest(request: Request): Uint8Array<ArrayBuffer> {
    return encodeRequest(request);
  }

  protected override decodeResponse(
    bytes: Uint8Array,
    session: bigint,
  ): Response {
    const response = decode(bytes) as Response;
    assert.equal(response.session, session);
    return response;
  }
}

function harness(session = 7n) {
  const sent: Request[] = [];
  let events: TransportEvents | undefined;
  let closes = 0;
  const client = new HarnessClient(
    {
      start(next) {
        events = next;
        next.ready();
      },
      send(bytes) {
        sent.push(decodeRequest(bytes));
      },
      async close() {
        closes++;
      },
    },
    session,
  );
  return {
    client,
    sent,
    get closes() {
      return closes;
    },
    reply(request: Request, body: ResponseBody, tick = 11n) {
      events!.message(
        encode({ session, requestId: request.requestId, tick, body }),
      );
    },
  };
}

const subscriptionIdentity = { output: 81n, generation: 3n };

function observationMarker(
  result: "subscribed" | "unsubscribed" | "cancelled",
): ResponseBody {
  return {
    kind: "guiObservation",
    record: {
      kind: "control",
      world,
      subscription: subscriptionIdentity,
      result,
    },
  };
}

function observation(
  ordinal: bigint,
  overrides: Partial<GuiObservedEffect> = {},
): ResponseBody {
  return {
    kind: "guiObservation",
    record: {
      kind: "effect",
      subscription: subscriptionIdentity,
      effect: {
        id: { world, ordinal },
        target,
        source: "semantic",
        tick: 1000n,
        ancestry: [8n, 9n, target.entity],
        effect: { kind: "pressed" },
        ...overrides,
      },
    },
  };
}

test("GUI observation ACK activates synchronously, preserves foreign history and never advances frame time", async () => {
  const transport = harness();
  const effects: GuiObservedEffect[] = [];
  let resolved = false;
  const pending = transport.client.subscribe((effect) => {
    assert.equal(resolved, false);
    effects.push(effect);
  });
  void pending.then(() => {
    resolved = true;
  });
  try {
    const request = transport.sent.at(-1)!;
    assert.deepEqual(request.body, {
      kind: "guiObservation",
      control: { kind: "subscribe", world, classes: "application" },
    });
    transport.reply(request, observationMarker("subscribed"), 0n);
    transport.reply({ ...request, requestId: 0n }, observation(2n), 0n);
    transport.reply(
      { ...request, requestId: 0n },
      observation(5n, {
        source: { kind: "routed", publication: { host: 99n, revision: 3n } },
        target: { ...target, incarnation: 1n },
        ancestry: [88n, target.entity],
      }),
      0n,
    );
    const subscription = await pending;
    assert.equal(effects.length, 2);
    assert.equal(subscription.start.kind, "subscribed");
    assert.equal("ordinal" in subscription.start, false);
    assert.equal(Object.isFrozen(effects[1]), true);
    assert.equal(Object.isFrozen(effects[1]!.target.world), true);
    assert.equal(Object.isFrozen(effects[1]!.ancestry), true);
    const ordinary = transport.client.ordinary();
    transport.reply(transport.sent.at(-1)!, inspected, 1n);
    await ordinary;
    assert.equal(transport.client.closure, undefined);
  } finally {
    await transport.client.close();
  }
});

test("GUI unsubscribe dispatches its prefix, cancellation preserves activity, an action outcome never dispatches", async () => {
  const transport = harness();
  const effects: GuiObservedEffect[] = [];
  const pending = transport.client.subscribe((effect) => effects.push(effect));
  transport.reply(transport.sent.at(-1)!, observationMarker("subscribed"), 0n);
  const subscription = await pending;
  try {
    const action = transport.client.act(press);
    acknowledge(transport, transport.sent.at(-1)!);
    await action;
    assert.equal(effects.length, 0);
    const cancelled = subscription.unsubscribe();
    const cancelledRequest = transport.sent.at(-1)!;
    transport.reply(
      { ...cancelledRequest, requestId: 0n },
      observation(1n),
      0n,
    );
    transport.reply(cancelledRequest, observationMarker("cancelled"), 0n);
    await assert.rejects(cancelled, /cancelled/);
    const ending = subscription.unsubscribe();
    const request = transport.sent.at(-1)!;
    transport.reply(
      { ...request, requestId: 0n },
      observation(3n, {
        source: { kind: "routed", publication: { host: 99n, revision: 4n } },
      }),
      0n,
    );
    transport.reply(request, observationMarker("unsubscribed"), 0n);
    const cut = await ending;
    assert.deepEqual(
      effects.map((effect) => effect.id.ordinal),
      [1n, 3n],
    );
    assert.deepEqual(await subscription.closed, { kind: "unsubscribed", cut });
    assert.deepEqual(await subscription.unsubscribe(), cut);
  } finally {
    await transport.client.close();
  }
});

test("GUI listener reentrant close fences that client without losing another observer", async () => {
  const closing = harness(7n);
  const healthy = harness(8n);
  const counts = [0, 0];
  const first = closing.client.subscribe(() => {
    counts[0]!++;
    void closing.client.close();
  });
  const second = healthy.client.subscribe(() => {
    counts[1]!++;
  });
  closing.reply(closing.sent.at(-1)!, observationMarker("subscribed"), 0n);
  healthy.reply(healthy.sent.at(-1)!, observationMarker("subscribed"), 0n);
  const subscriptions = await Promise.all([first, second]);
  closing.reply(
    { ...closing.sent.at(-1)!, requestId: 0n },
    observation(1n),
    0n,
  );
  healthy.reply(
    { ...healthy.sent.at(-1)!, requestId: 0n },
    observation(1n),
    0n,
  );
  assert.deepEqual(counts, [1, 1]);
  assert.equal((await subscriptions[0]!.closed).kind, "closed");
  assert.equal(healthy.client.closure, undefined);
  await healthy.client.close();
});

test("malformed GUI observation provenance, ordinal and outer clock fence only their client", async () => {
  const cases: [ResponseBody, bigint][] = [
    [observation(1n), 0n],
    [
      observation(2n, {
        id: { world: { id: 99n, incarnation: 4n }, ordinal: 2n },
      }),
      0n,
    ],
    [
      observation(2n, {
        target: { ...target, world: { id: 3n, incarnation: 99n } },
      }),
      0n,
    ],
    [
      observation(2n, {
        effect: { kind: "focusChanged", focused: true, changed: true, part: 0 },
      }),
      0n,
    ],
    [observation(2n), 1n],
  ];
  for (const [bad, tick] of cases) {
    const transport = harness();
    const healthy = harness(8n);
    const pending = transport.client.subscribe(() => {});
    transport.reply(
      transport.sent.at(-1)!,
      observationMarker("subscribed"),
      0n,
    );
    await pending;
    const event = { ...transport.sent.at(-1)!, requestId: 0n };
    transport.reply(event, observation(1n), 0n);
    const request = transport.client.ordinary();
    transport.reply(event, bad, tick);
    await assert.rejects(request);
    await assert.rejects(transport.client.act(press), /closed/i);
    assert.equal(healthy.client.closure, undefined);
    await healthy.client.close();
  }
});

function acknowledge(
  transport: ReturnType<typeof harness>,
  request: Request,
  reason?: string,
): void {
  assert.ok(request.body.kind === "submitBatch" && request.body.last);
  const identity = {
    batchId: BigInt(request.body.batchId),
    tick: 11n,
    aliases: [],
    symbols: [],
    effects: [],
  };
  transport.reply(request, {
    kind: "batch",
    outcome:
      reason === undefined
        ? { ok: true, ...identity }
        : {
            ok: false,
            ...identity,
            error: { scope: "operation", operation: 0, reason },
          },
  });
}

test("a GUI action is an ordinary batch of one command answered by its outcome", async () => {
  const transport = harness();
  try {
    const query = transport.client.ordinary();
    transport.reply(transport.sent[0]!, inspected);
    assert.equal((await query).tick, 11n);
    const action = transport.client.act(press);
    const request = transport.sent[1]!;
    assert.ok(request.body.kind === "submitBatch" && request.body.last);
    assert.deepEqual(request.body.operations, [press]);
    acknowledge(transport, request);
    assert.equal((await action).ok, true);
    assert.equal(transport.sent.length, 2);
  } finally {
    await transport.client.close();
  }
});

test("paged batch pages leave at once and hold neither ordinary requests nor GUI actions", async () => {
  const transport = harness();
  const commands: Command[] = Array.from(
    { length: PAGE_LIMITS.commands + 1 },
    (_, alias) => ({
      kind: "create",
      alias,
      metadata: { symbolicId: "", classes: [] },
    }),
  );
  const batch = transport.client.batch(commands);
  void batch.catch(() => {});
  let ordinary: Promise<unknown> | undefined;
  let action: Promise<unknown> | undefined;
  try {
    const pages = transport.sent.filter(
      (request) => request.body.kind === "submitBatch",
    );
    assert.deepEqual(
      pages.map((request) =>
        request.body.kind === "submitBatch"
          ? [
              request.requestId === 0n,
              request.body.last,
              request.body.operations.length,
            ]
          : [],
      ),
      [
        [true, false, PAGE_LIMITS.commands],
        [false, true, 1],
      ],
      "only the final page is correlated",
    );
    ordinary = transport.client.ordinary();
    action = transport.client.act(press);
    const sentAfter = transport.sent
      .slice(pages.length)
      .map((r) => r.body.kind);
    assert.deepEqual(sentAfter, ["inspect", "submitBatch"]);
    transport.reply(transport.sent[2]!, inspected);
    acknowledge(transport, transport.sent[3]!);
    await ordinary;
    assert.equal(((await action) as { ok: boolean }).ok, true);
    acknowledge(transport, pages[1]!);
    assert.equal((await batch).ok, true);
    assert.equal(transport.client.closure, undefined);
  } finally {
    await transport.client.close();
    await Promise.allSettled([batch, ordinary, action]);
  }
});

for (const operation of ["subscribe", "unsubscribe"] as const) {
  for (const code of [1, 3, 9]) {
    test(`GUI ${operation} code ${code} fences unknown control errors before same-stack effects`, async () => {
      const transport = harness();
      const peer = harness(8n);
      const effects: GuiObservedEffect[] = [];
      const opening = transport.client.subscribe((effect) =>
        effects.push(effect),
      );
      transport.reply(
        transport.sent.at(-1)!,
        observationMarker("subscribed"),
        0n,
      );
      const subscription = await opening;
      const control =
        operation === "subscribe"
          ? transport.client.subscribe((effect) => effects.push(effect))
          : subscription.unsubscribe();
      const request = transport.sent.at(-1)!;
      const rejection = assert.rejects(control, new RegExp(`Host ${code}:`));
      const waiting = transport.client.ordinary();
      const ordinary = transport.sent.at(-1)!;
      void waiting.catch(() => {});
      try {
        transport.reply(request, {
          kind: "error",
          code,
          message: "control outcome unavailable",
        });
        transport.reply({ ...request, requestId: 0n }, observation(1n), 0n);
        transport.reply(ordinary, inspected);
        if (code === 1) {
          assert.equal(transport.client.closure, undefined);
          assert.equal(effects.length, 1);
          await waiting;
          await rejection;
          const retry =
            operation === "subscribe"
              ? transport.client.subscribe(() => {})
              : subscription.unsubscribe();
          const marker = observationMarker(
            operation === "subscribe" ? "subscribed" : "unsubscribed",
          );
          if (operation === "subscribe" && marker.kind === "guiObservation")
            marker.record.subscription = { output: 82n, generation: 3n };
          transport.reply(transport.sent.at(-1)!, marker, 0n);
          await retry;
          assert.equal(transport.client.closure, undefined);
        } else {
          assert.ok(
            transport.client.closure,
            "Unknown control delivery must fence synchronously",
          );
          assert.equal(
            effects.length,
            0,
            "No callback may follow an unknown control error",
          );
          await assert.rejects(waiting, new RegExp(`Host ${code}:`));
          await rejection;
          assert.equal((await subscription.closed).kind, "closed");
          const sent = transport.sent.length;
          await assert.rejects(transport.client.subscribe(() => {}));
          assert.equal(transport.sent.length, sent);
        }
        const healthy = peer.client.act(press);
        acknowledge(peer, peer.sent.at(-1)!);
        assert.equal((await healthy).ok, true);
        assert.equal(peer.client.closure, undefined);
      } finally {
        await transport.client.close();
        await peer.client.close();
        await Promise.allSettled([control, waiting, rejection]);
      }
    });
  }
}

test("a correlated reply of another kind fences only the affected logical Client", async () => {
  const transport = harness();
  const peer = harness(8n);
  const action = transport.client.act(press);
  const waiting = transport.client.ordinary();
  const failure = /Batch response correlation mismatch/;
  const requestFailure = assert.rejects(action, failure);
  void waiting.catch(() => {});
  try {
    transport.reply(transport.sent[0]!, { kind: "cameraNavigated" });
    await requestFailure;
    assert.ok(
      transport.client.closure,
      "An unknown outcome must close this Client",
    );
    assert.match((await transport.client.closed).reason.message, failure);
    await assert.rejects(waiting);
    await assert.rejects(transport.client.act(press), /closed/);
    assert.equal(transport.sent.length, 2);
    const healthy = peer.client.act(press);
    acknowledge(peer, peer.sent[0]!);
    assert.equal((await healthy).ok, true);
    assert.equal(peer.client.closure, undefined);
  } finally {
    await transport.client.close();
    await peer.client.close();
    await Promise.allSettled([action, waiting]);
  }
});

test("a refused or unadmitted GUI action leaves the Client usable without retrying", async () => {
  const transport = harness();
  try {
    const rejected = transport.client.act(press);
    transport.reply(transport.sent.at(-1)!, {
      kind: "error",
      code: 1,
      message: "not admitted",
    });
    await assert.rejects(rejected, RequestRejectedError);
    assert.equal(transport.client.closure, undefined);
    for (const reason of [
      "StaleTarget",
      "Unavailable",
      "UnsupportedAction",
      "InvalidValue",
    ]) {
      const sent = transport.sent.length;
      const pending = transport.client.act(press);
      acknowledge(transport, transport.sent.at(-1)!, reason);
      const outcome = await pending;
      assert.ok(!outcome.ok && outcome.error.reason === reason);
      assert.equal(transport.sent.length, sent + 1);
      assert.equal(transport.client.closure, undefined);
    }
  } finally {
    await transport.client.close();
  }
});

/** A worker port transport that reports when the worker's ready envelope arrived. */
class ObservedPortTransport extends PortTransport {
  readonly started: Promise<void>;
  private markStarted!: () => void;

  constructor(port: MessagePort, connection: bigint) {
    super(port, connection);
    this.started = new Promise((resolve) => {
      this.markStarted = resolve;
    });
  }

  override start(events: TransportEvents): void {
    super.start({
      ...events,
      ready: () => {
        events.ready();
        this.markStarted();
      },
    });
  }
}

test("a request waiting for connection credit gets its reply deadline only when sent", async () => {
  const timeoutMs = 100;
  const channel = new MessageChannel();
  const worker = channel.port2;
  const posted: { request: Request; bytes: number }[] = [];
  let delivery = 0n;
  worker.on("message", (message) => {
    if (message.type === "data")
      posted.push({
        request: decodeRequest(new Uint8Array(message.bytes)),
        bytes: message.bytes.byteLength,
      });
  });
  // The worker grants room for one message and returns no credit until told.
  worker.postMessage({
    type: "ready",
    connection: 5n,
    credit: { messages: 1, bytes: 1 << 20 },
  });
  const transport = new ObservedPortTransport(
    channel.port1 as unknown as MessagePort,
    5n,
  );
  const client = new HarnessClient(transport, 7n, timeoutMs);
  try {
    await transport.started;
    const first = client.ordinary();
    const queued = client.ordinary();
    void queued.catch(() => {});
    await delay(10);
    assert.equal(posted.length, 1, "the second request waits for credit");

    // The connection makes progress: the first request is answered.
    worker.postMessage({
      type: "data",
      connection: 5n,
      delivery: ++delivery,
      bytes: encode({
        session: 7n,
        requestId: posted[0]!.request.requestId,
        tick: 11n,
        body: inspected,
      }).buffer,
    });
    await first;
    await delay(3 * timeoutMs);
    assert.equal(client.closure, undefined, "waiting for credit timed out");
    assert.equal(posted.length, 1);

    // Once it leaves, the request's own deadline applies as usual.
    const released = performance.now();
    worker.postMessage({
      type: "credit",
      connection: 5n,
      messages: 1,
      bytes: posted[0]!.bytes,
    });
    await assert.rejects(queued, /Request timed out/);
    assert.equal(posted.length, 2);
    assert.ok(performance.now() - released >= timeoutMs - 5);
    const closure = client.closure as { reason: Error } | undefined;
    assert.match(closure?.reason.message ?? "", /Request timed out/);
  } finally {
    channel.port1.close();
    worker.close();
  }
});
