/** I/O-boundary timing tests supplement real HTTP/rendering integration scenarios. */
import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate } from "node:timers/promises";
import {
  AssetWorkerService,
  type AssetHostExports,
} from "../src/resource-worker.js";

class AssetInput implements AssetHostExports {
  memory = new WebAssembly.Memory({ initial: 3 });
  pending = true;
  blocked = true;
  delivered: number[] = [];
  ended: { success: number; error: string }[] = [];
  progressCalls = 0;
  serviceCalls = 0;
  autoDrain = false;
  request: Uint8Array;

  constructor() {
    const source = new TextEncoder().encode("https://assets.test/fixture");
    this.request = new Uint8Array(13 + source.length);
    const view = new DataView(this.request.buffer);
    view.setUint8(0, 1);
    view.setBigUint64(1, 1n, true);
    view.setUint32(9, source.length, true);
    this.request.set(source, 13);
    new Uint8Array(this.memory.buffer, 128, this.request.length).set(
      this.request,
    );
  }

  cancelRequest() {
    this.request = new Uint8Array(13);
    const view = new DataView(this.request.buffer);
    view.setBigUint64(1, 1n, true);
    new Uint8Array(this.memory.buffer, 128, this.request.length).set(
      this.request,
    );
    this.pending = true;
  }

  ipp_resource_poll() {
    const result = Number(this.pending);
    this.pending = false;
    return result;
  }
  ipp_progress_resources() {
    this.progressCalls++;
    if (this.autoDrain) this.blocked = false;
    return 1;
  }
  ipp_service_resources() {
    this.serviceCalls++;
    return 1;
  }
  ipp_asset_error_max_bytes() {
    return 2048;
  }
  ipp_resource_buffered_bytes() {
    return 0;
  }
  ipp_resource_chunk(_session: bigint, _id: bigint, length: number) {
    if (this.blocked) return 2;
    this.delivered.push(...new Uint8Array(this.memory.buffer, 4096, length));
    if (this.autoDrain) this.blocked = true;
    return 1;
  }
  ipp_resource_end(
    _session: bigint,
    _id: bigint,
    success: number,
    length: number,
  ) {
    this.ended.push({
      success,
      error: new TextDecoder("utf-8", { fatal: true }).decode(
        new Uint8Array(this.memory.buffer, 4096, length),
      ),
    });
    return 1;
  }
  ipp_resource_input_reserve() {
    return 4096;
  }
  ipp_output_ptr() {
    return 128;
  }
  ipp_output_len() {
    return this.request.length;
  }
}

test("idle post-frame pumps expose provider work without polling loaders", () => {
  const input = new AssetInput();
  input.pending = false;
  let prepared = 0;
  const worker = new AssetWorkerService(
    input,
    1n,
    "off",
    {},
    [],
    undefined,
    () => prepared++,
  );
  try {
    worker.pumpAfterFrame();
    assert.equal(input.serviceCalls, 1);
    assert.equal(input.progressCalls, 0);
    assert.equal(prepared, 0);
  } finally {
    worker.close();
  }
});

test("Host cancellation aborts the exact active HTTP reader without completion", async (context) => {
  const input = new AssetInput();
  let signal: AbortSignal | undefined;
  context.mock.method(
    globalThis,
    "fetch",
    (_url: unknown, options: RequestInit) => {
      signal = options.signal!;
      return new Promise((_resolve, reject) => {
        signal!.addEventListener("abort", () => reject(signal!.reason), {
          once: true,
        });
      });
    },
  );
  const worker = new AssetWorkerService(input, 1n, "off");
  try {
    worker.pump();
    await setImmediate();
    assert.equal(signal?.aborted, false);
    input.cancelRequest();
    worker.pump();
    assert.equal(signal?.aborted, true);
    assert.deepEqual(input.ended, []);
  } finally {
    worker.close();
  }
});

test("multi-chunk HTTP input advances Host resources without frame calls and stays bounded", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const input = new AssetInput();
  input.blocked = false;
  input.autoDrain = true;
  const chunkBytes = 64 << 10;
  const payloadLength = 3 * chunkBytes + 17;
  const payload = Uint8Array.from(
    { length: payloadLength },
    (_, index) => index & 0xff,
  );
  context.mock.method(
    globalThis,
    "fetch",
    async () =>
      new Response(
        new ReadableStream({
          type: "bytes",
          start(controller) {
            controller.enqueue(payload);
            controller.close();
          },
        }),
      ),
  );
  const counters: Record<string, number> = {};
  const worker = new AssetWorkerService(input, 1n, "off", counters);
  try {
    worker.pump();
    for (let turn = 0; turn < 20 && input.ended.length === 0; turn++) {
      await setImmediate();
      context.mock.timers.tick(0);
    }
    assert.equal(input.delivered.length, payloadLength);
    assert.ok(input.delivered.every((byte, index) => byte === (index & 0xff)));
    assert.deepEqual(input.ended, [{ success: 1, error: "" }]);
    assert.ok(input.progressCalls >= 4, "each full pipe made Host progress");
    assert.equal(counters.sourceBytes, payloadLength);
    assert.ok(
      (counters.sourcePeakBufferedBytes ?? Number.POSITIVE_INFINITY) <=
        2 * chunkBytes,
      `retained ${counters.sourcePeakBufferedBytes} source bytes`,
    );
  } finally {
    worker.close();
  }
});

test("ready HTTP chunks ignore simulation cadence and suspend inactivity during local backpressure", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const input = new AssetInput();
  let signal: AbortSignal | undefined;
  context.mock.method(
    globalThis,
    "fetch",
    async (_url: unknown, options: RequestInit) => {
      signal = options.signal!;
      let sent = false;
      return new Response(
        new ReadableStream({
          type: "bytes",
          pull(controller) {
            if (!sent) {
              sent = true;
              controller.enqueue(new Uint8Array([1, 2, 3]));
            } else {
              controller.close();
              controller.byobRequest?.respond(0);
            }
          },
        }),
      );
    },
  );
  const worker = new AssetWorkerService(input, 1n, "off");
  try {
    worker.pump();
    await setImmediate();
    context.mock.timers.tick(31_000);
    assert.equal(signal!.aborted, false, "a ready chunk is local backpressure");
    assert.deepEqual(input.delivered, []);
    input.blocked = false;
    context.mock.timers.tick(4);
    await setImmediate();
    context.mock.timers.tick(0);
    assert.deepEqual(
      input.delivered,
      [1, 2, 3],
      "scheduled pump delivered without another frame",
    );
    assert.deepEqual(input.ended, [{ success: 1, error: "" }]);
  } finally {
    worker.close();
  }
});

test("network inactivity fails its resource and UTF-8 errors share the Host byte limit", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const input = new AssetInput();
  context.mock.method(
    globalThis,
    "fetch",
    (_url: unknown, options: RequestInit) =>
      new Promise((_resolve, reject) => {
        options.signal!.addEventListener(
          "abort",
          () => reject(options.signal!.reason),
          { once: true },
        );
      }),
  );
  const worker = new AssetWorkerService(input, 1n, "off");
  try {
    worker.pump();
    context.mock.timers.tick(29_999);
    assert.equal(input.ended.length, 0);
    context.mock.timers.tick(1);
    await setImmediate();
    context.mock.timers.tick(0);
    assert.match(input.ended[0]!.error, /no progress for 30 seconds/);
  } finally {
    worker.close();
  }
  context.mock.method(globalThis, "fetch", async () => {
    throw new Error("é".repeat(1023) + "🌍");
  });
  const unicode = new AssetInput();
  const other = new AssetWorkerService(unicode, 1n, "off");
  try {
    other.pump();
    await setImmediate();
    context.mock.timers.tick(0);
    assert.equal(unicode.ended[0]!.error, "é".repeat(1023));
  } finally {
    other.close();
  }
});

class AvailabilitySocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static latest: AvailabilitySocket | undefined;

  readyState = AvailabilitySocket.CONNECTING;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  readonly sent: string[] = [];

  constructor(readonly url: URL) {
    AvailabilitySocket.latest = this;
  }

  open() {
    this.readyState = AvailabilitySocket.OPEN;
    this.onopen?.();
  }

  send(message: string) {
    this.sent.push(message);
  }

  message(message: unknown) {
    this.onmessage?.({ data: JSON.stringify(message) });
  }

  close() {
    this.readyState = 3;
  }
}

async function flushTasks(): Promise<void> {
  await setImmediate();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await setImmediate();
}

test("pending HTTP sources free acquisition progress and resume after fenced readiness", async (context) => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "WebSocket");
  Object.defineProperty(globalThis, "WebSocket", {
    configurable: true,
    value: AvailabilitySocket,
  });
  const input = new AssetInput();
  let fetches = 0;
  context.mock.method(globalThis, "fetch", async () => {
    fetches++;
    if (fetches === 1)
      return new Response(
        JSON.stringify({
          source: "/fixture",
          monitor: "/availability?session=export-7",
        }),
        { status: 202, headers: { "content-type": "application/json" } },
      );
    let sent = false;
    return new Response(
      new ReadableStream({
        type: "bytes",
        pull(controller) {
          if (!sent) {
            sent = true;
            controller.enqueue(new Uint8Array([4, 5, 6]));
          } else {
            controller.close();
            controller.byobRequest?.respond(0);
          }
        },
      }),
      { headers: { etag: '"immutable"' } },
    );
  });
  const worker = new AssetWorkerService(input, 1n, "off");
  try {
    input.blocked = false;
    worker.pump();
    await flushTasks();
    const socket = AvailabilitySocket.latest!;
    assert.equal(fetches, 1);
    assert.equal(socket.url.protocol, "wss:");
    socket.open();
    assert.deepEqual(JSON.parse(socket.sent[0]!), {
      source: "/fixture",
      watch: true,
    });
    socket.message({
      session: "export-7",
      assets: [{ source: "/fixture", state: "pending" }],
    });
    await flushTasks();
    assert.equal(fetches, 1, "pending source retried before readiness");
    assert.equal(input.ended.length, 0);
    socket.message({
      session: "export-7",
      assets: [{ source: "/fixture", state: "ready" }],
    });
    await flushTasks();
    await flushTasks();
    await flushTasks();
    assert.equal(fetches, 2);
    assert.deepEqual(input.delivered, [4, 5, 6]);
    assert.deepEqual(input.ended, [{ success: 1, error: "" }]);
  } finally {
    worker.close();
    if (original) Object.defineProperty(globalThis, "WebSocket", original);
    else delete (globalThis as { WebSocket?: unknown }).WebSocket;
  }
});

test("provider disconnect fails unresolved pending sources", async (context) => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "WebSocket");
  Object.defineProperty(globalThis, "WebSocket", {
    configurable: true,
    value: AvailabilitySocket,
  });
  const input = new AssetInput();
  context.mock.method(
    globalThis,
    "fetch",
    async () =>
      new Response(
        JSON.stringify({
          source: "/fixture",
          monitor: "/availability?session=export-8",
        }),
        { status: 202, headers: { "content-type": "application/json" } },
      ),
  );
  const worker = new AssetWorkerService(input, 1n, "off");
  try {
    worker.pump();
    await flushTasks();
    const socket = AvailabilitySocket.latest!;
    socket.open();
    socket.onclose?.();
    await flushTasks();
    assert.equal(input.ended[0]!.success, 0);
    assert.match(input.ended[0]!.error, /producer disconnected/);
  } finally {
    worker.close();
    if (original) Object.defineProperty(globalThis, "WebSocket", original);
    else delete (globalThis as { WebSocket?: unknown }).WebSocket;
  }
});
