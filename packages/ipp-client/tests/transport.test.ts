import assert from "node:assert/strict";
import { MessageChannel } from "node:worker_threads";
import test from "node:test";
import { PortTransport, type TransportEvents } from "../src/transport.js";

/** A worker's ready envelope granting its default ingress credit window. */
function readyEnvelope(
  connection: bigint,
  credit = { messages: 64, bytes: 1 << 23 },
) {
  return { type: "ready", connection, credit };
}

test("close before start and peer listener installation settles once without readiness", async () => {
  const channel = new MessageChannel();
  let disposed = 0;
  const port = channel.port1 as unknown as MessagePort;
  const transport = new PortTransport(port, 7n, () => disposed++);
  try {
    const closing = transport.close();
    const staleHandler = port.onmessage;
    assert.equal(transport.close(), closing);
    assert.throws(
      () =>
        transport.start({ ready() {}, message() {}, error() {}, closed() {} }),
      /closed/,
    );
    await new Promise<void>((resolve) => setImmediate(resolve));
    let closeRequests = 0;
    channel.port2.on("message", (message) => {
      assert.deepEqual(message, { type: "close", connection: 7n });
      closeRequests++;
      channel.port2.postMessage({ type: "closed", connection: 7n });
    });
    await closing;
    assert.equal(closeRequests, 1);
    assert.equal(disposed, 1);
    assert.equal(transport.close(), closing);
    assert.equal(port.onmessage, null);
    assert.equal(port.onmessageerror, null);
    staleHandler?.call(port, {
      data: readyEnvelope(7n),
    } as MessageEvent);
    assert.throws(() => transport.send(new Uint8Array(1)), /closed/);
    assert.equal(disposed, 1);
  } finally {
    channel.port1.close();
    channel.port2.close();
  }
});

test("ready arriving after close cannot bootstrap but still drains promoted deliveries", async () => {
  const channel = new MessageChannel();
  let disposed = 0;
  let ready = 0;
  let messages = 0;
  let closed = 0;
  const acknowledgements: bigint[] = [];
  const transport = new PortTransport(
    channel.port1 as unknown as MessagePort,
    7n,
    () => disposed++,
  );
  transport.start({
    ready() {
      ready++;
    },
    message() {
      messages++;
    },
    error(error) {
      throw error;
    },
    closed() {
      closed++;
    },
  });
  channel.port2.on("message", (message) => {
    if (message.type === "close") {
      channel.port2.postMessage(readyEnvelope(7n));
      for (const delivery of [3n, 5n])
        channel.port2.postMessage({
          type: "data",
          connection: 7n,
          delivery,
          bytes: new ArrayBuffer(4),
        });
    } else {
      assert.equal(message.type, "ack");
      acknowledgements.push(message.delivery);
      if (acknowledgements.length === 2)
        channel.port2.postMessage({ type: "closed", connection: 7n });
    }
  });
  try {
    const closing = transport.close();
    assert.equal(transport.close(), closing);
    await closing;
    assert.deepEqual(acknowledgements, [3n, 5n]);
    assert.deepEqual(
      { ready, messages, closed, disposed },
      { ready: 0, messages: 0, closed: 1, disposed: 1 },
    );
  } finally {
    channel.port1.close();
    channel.port2.close();
  }
});

test("reentrant close acknowledges current and queued deliveries without later callbacks", async () => {
  const channel = new MessageChannel();
  let disposed = 0;
  const messages: string[] = [];
  const received: bigint[] = [];
  let closing: Promise<void> | undefined;
  const transport = new PortTransport(
    channel.port1 as unknown as MessagePort,
    7n,
    () => disposed++,
  );
  transport.start({
    ready() {},
    message() {
      messages.push("data");
      closing = transport.close();
    },
    error(error) {
      throw error;
    },
    closed() {},
  });
  const finished = new Promise<void>((resolve) => {
    channel.port2.on("message", (message) => {
      assert.equal(message.connection, 7n);
      if (message.type === "ack") received.push(message.delivery);
      else assert.equal(message.type, "close");
      if (received.length === 2) {
        channel.port2.postMessage({ type: "closed", connection: 7n });
        resolve();
      }
    });
  });
  try {
    channel.port2.postMessage(readyEnvelope(7n));
    for (const delivery of [4n, 8n])
      channel.port2.postMessage({
        type: "data",
        connection: 7n,
        delivery,
        bytes: new ArrayBuffer(16),
      });
    await finished;
    await closing;
    assert.deepEqual(messages, ["data"]);
    assert.deepEqual(received, [4n, 8n]);
    assert.equal(disposed, 1);
  } finally {
    channel.port1.close();
    channel.port2.close();
  }
});

for (const kind of ["foreign", "duplicate"] as const) {
  test(`${kind} delivery fences the endpoint without acknowledging foreign ownership`, async () => {
    const channel = new MessageChannel();
    let disposed = 0;
    let delivered = 0;
    const acknowledgements: bigint[] = [];
    const transport = new PortTransport(
      channel.port1 as unknown as MessagePort,
      2n,
      () => disposed++,
    );
    const failed = new Promise<Error>((resolve) =>
      transport.start({
        ready() {},
        message() {
          delivered++;
        },
        error: resolve,
        closed() {},
      }),
    );
    channel.port2.on("message", (message) =>
      acknowledgements.push(message.delivery),
    );
    try {
      channel.port2.postMessage(readyEnvelope(2n));
      channel.port2.postMessage({
        type: "data",
        connection: kind === "foreign" ? 1n : 2n,
        delivery: 1n,
        bytes: new ArrayBuffer(4),
      });
      if (kind === "duplicate")
        channel.port2.postMessage({
          type: "data",
          connection: 2n,
          delivery: 1n,
          bytes: new ArrayBuffer(4),
        });
      assert.match((await failed).message, /worker envelope/);
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(delivered, kind === "foreign" ? 0 : 1);
      assert.deepEqual(acknowledgements, kind === "foreign" ? [] : [1n]);
      assert.equal(disposed, 1);
    } finally {
      channel.port1.close();
      channel.port2.close();
    }
  });
}

test("worker close tolerates a frame longer than one second within its configured deadline", async () => {
  const channel = new MessageChannel();
  let disposed = 0;
  const transport = new PortTransport(
    channel.port1 as unknown as MessagePort,
    1n,
    () => {
      disposed++;
    },
    undefined,
    3000,
  );
  transport.start({
    ready() {},
    message() {},
    error(error) {
      throw error;
    },
    closed() {},
  });
  channel.port2.on("message", (message) => {
    assert.equal(message.type, "close");
    setTimeout(
      () => channel.port2.postMessage({ type: "closed", connection: 1n }),
      1200,
    );
  });
  try {
    const closing = transport.close();
    assert.equal(transport.close(), closing);
    await closing;
    assert.equal(disposed, 1);
  } finally {
    channel.port1.close();
    channel.port2.close();
  }
});

test("worker close still terminates an unresponsive participant at its deadline", async () => {
  const channel = new MessageChannel();
  let disposed = 0;
  const transport = new PortTransport(
    channel.port1 as unknown as MessagePort,
    1n,
    () => {
      disposed++;
    },
    undefined,
    10,
  );
  try {
    await assert.rejects(transport.close(), /Worker close timed out/);
    assert.equal(disposed, 1);
  } finally {
    channel.port1.close();
    channel.port2.close();
  }
});

/** A started transport whose worker end records posted envelopes. */
async function creditHarness(
  credit: { messages: number; bytes: number },
  ingressProgressMs?: number,
) {
  const channel = new MessageChannel();
  const posted: { type: string; size?: number }[] = [];
  const errors: Error[] = [];
  const transport = new PortTransport(
    channel.port1 as unknown as MessagePort,
    3n,
    undefined,
    undefined,
    undefined,
    ingressProgressMs,
  );
  const started = new Promise<void>((resolve) => {
    const events: TransportEvents = {
      ready: resolve,
      message() {},
      error(error) {
        errors.push(error);
      },
      closed() {},
    };
    transport.start(events);
  });
  channel.port2.on("message", (message) => {
    posted.push({
      type: message.type,
      ...(message.bytes ? { size: message.bytes.byteLength } : {}),
    });
    if (message.type === "close")
      channel.port2.postMessage({ type: "closed", connection: 3n });
  });
  channel.port2.postMessage(readyEnvelope(3n, credit));
  await started;
  const settle = () => new Promise((resolve) => setImmediate(resolve));
  const grant = async (messages: number, bytes: number) => {
    channel.port2.postMessage({
      type: "credit",
      connection: 3n,
      messages,
      bytes,
    });
    await settle();
  };
  const close = () => {
    channel.port1.close();
    channel.port2.close();
  };
  return { transport, posted, errors, grant, settle, close, channel };
}

test("sends beyond the worker's credit wait and leave in call order as credit returns", async () => {
  const state = await creditHarness({ messages: 2, bytes: 8 });
  try {
    for (const size of [4, 4, 2, 1])
      state.transport.send(new Uint8Array(size).fill(size));
    await state.settle();
    assert.deepEqual(
      state.posted.map(({ size }) => size),
      [4, 4],
      "only the granted window leaves",
    );
    await state.grant(1, 1);
    assert.deepEqual(
      state.posted.map(({ size }) => size),
      [4, 4],
      "the next message waits for bytes, and nothing overtakes it",
    );
    await state.grant(0, 3);
    assert.deepEqual(
      state.posted.map(({ size }) => size),
      [4, 4, 2],
    );
    await state.grant(1, 4);
    assert.deepEqual(
      state.posted.map(({ size }) => size),
      [4, 4, 2, 1],
    );
    assert.deepEqual(state.errors, []);
  } finally {
    state.close();
  }
});

test("a message larger than the whole credit window is refused before sending", async () => {
  const state = await creditHarness({ messages: 4, bytes: 8 });
  try {
    assert.throws(
      () => state.transport.send(new Uint8Array(9)),
      /exceeds the worker's ingress credit window/,
    );
    state.transport.send(new Uint8Array(8));
    await state.settle();
    assert.deepEqual(state.posted, [{ type: "data", size: 8 }]);
  } finally {
    state.close();
  }
});

test("credit the worker never granted fails the connection", async () => {
  const state = await creditHarness({ messages: 2, bytes: 8 });
  try {
    state.transport.send(new Uint8Array(4));
    await state.grant(2, 4);
    assert.match(state.errors[0]?.message ?? "", /had not granted/);
    assert.throws(() => state.transport.send(new Uint8Array(1)), /closed/);
  } finally {
    state.close();
  }
});

test("a ready envelope without ingress credit fails the connection", async () => {
  const channel = new MessageChannel();
  const transport = new PortTransport(
    channel.port1 as unknown as MessagePort,
    3n,
  );
  try {
    const failed = new Promise<Error>((resolve) =>
      transport.start({
        ready() {
          throw new Error("started without credit");
        },
        message() {},
        error: resolve,
        closed() {},
      }),
    );
    channel.port2.postMessage({ type: "ready", connection: 3n });
    assert.match((await failed).message, /no ingress credit/);
  } finally {
    channel.port1.close();
    channel.port2.close();
  }
});

test("closing discards messages still waiting for credit and ignores late credit", async () => {
  const state = await creditHarness({ messages: 1, bytes: 8 });
  try {
    state.transport.send(new Uint8Array(2));
    state.transport.send(new Uint8Array(3));
    const closing = state.transport.close();
    await state.grant(1, 2);
    await closing;
    assert.deepEqual(
      state.posted.map(({ type, size }) => size ?? type),
      [2, "close"],
    );
    assert.deepEqual(state.errors, []);
  } finally {
    state.close();
  }
});

test("sends report when a waiting message leaves, and a credit stall fails the connection", async () => {
  const state = await creditHarness({ messages: 1, bytes: 8 }, 60);
  try {
    assert.equal(state.transport.send(new Uint8Array(2)), undefined);
    const waiting = state.transport.send(new Uint8Array(2));
    assert.ok(waiting instanceof Promise, "a waiting message reports its send");
    let left = false;
    void waiting.then(() => {
      left = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 40));
    await state.grant(1, 2);
    assert.ok(left, "the waiting message left when credit returned");
    assert.deepEqual(state.errors, [], "returned credit restarts the deadline");

    state.transport.send(new Uint8Array(2));
    await new Promise((resolve) => setTimeout(resolve, 120));
    const [stalled] = state.errors as Error[];
    assert.match(
      stalled?.message ?? "",
      /connection congestion: no ingress credit returned/,
    );
  } finally {
    state.close();
  }
});
