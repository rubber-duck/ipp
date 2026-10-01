import assert from "node:assert/strict";
import test from "node:test";
import {
  WorkerConnections,
  type WorkerConnectionExports,
} from "../src/worker-connections.js";

class Endpoint {
  onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  onmessageerror: (() => void) | null = null;
  sent: Record<string, unknown>[] = [];
  throws = false;
  closed = false;

  start() {}

  postMessage(data: Record<string, unknown>) {
    if (this.throws) throw new Error("postMessage failed");
    this.sent.push(data);
  }

  close() {
    this.closed = true;
  }

  receive(data: Record<string, unknown>) {
    this.onmessage?.({ data } as MessageEvent);
  }
}

function fixture() {
  const pending = new Map<bigint, bigint[]>();
  const closed = new Set<bigint>();
  const disposed: bigint[] = [];
  const completed: [bigint, bigint][] = [];
  const ready = new Map<bigint, number>();
  const received: [bigint, number][] = [];
  const accepting = new Set<bigint>();
  let reserved = 0;
  let next = 0n;
  const runtime: WorkerConnectionExports = {
    memory: new WebAssembly.Memory({ initial: 1 }),
    ipp_connection_limit: () => 64,
    ipp_delivery_limit: () => 64,
    ipp_request_window: () => 64,
    ipp_connection_open(connection) {
      pending.set(connection, []);
      return 1;
    },
    ipp_connection_close(connection) {
      closed.add(connection);
      return 1;
    },
    ipp_connection_dispose(connection) {
      disposed.push(connection);
      pending.delete(connection);
      return 1;
    },
    ipp_connection_failed() {
      return 0;
    },
    ipp_connection_pending(connection) {
      return pending.get(connection)?.length ?? 0;
    },
    ipp_connection_poll(connection) {
      const count = ready.get(connection) ?? 0;
      if (count === 0) return 0;
      ready.set(connection, count - 1);
      pending.get(connection)!.push(++next);
      return 1;
    },
    ipp_output_delivery_id() {
      return next;
    },
    ipp_output_copied() {
      return 1;
    },
    ipp_delivery_complete(connection, delivery) {
      if (pending.get(connection)?.[0] !== delivery) return 0;
      completed.push([connection, delivery]);
      pending.get(connection)!.shift();
      return 1;
    },
    ipp_accepts_input(connection) {
      return accepting.has(connection) ? 0 : 1;
    },
    ipp_input_reserve(length) {
      reserved = length;
      return 16;
    },
    ipp_receive(connection, length) {
      received.push([connection, length]);
      return length === reserved ? 1 : 0;
    },
    ipp_output_ptr() {
      return 16;
    },
    ipp_output_len() {
      return 32;
    },
  };
  const connections = new WorkerConnections(runtime, 1024);
  const open = (connection: bigint) => {
    const endpoint = new Endpoint();
    connections.open(connection, endpoint as unknown as MessagePort);
    return endpoint;
  };
  /** Model a Host that throttles this connection's input. */
  const throttle = (connection: bigint, throttled: boolean) => {
    if (throttled) accepting.add(connection);
    else accepting.delete(connection);
  };
  return {
    connections,
    open,
    pending,
    closed,
    disposed,
    completed,
    ready,
    received,
    throttle,
  };
}

test("worker delivery window and timeout retain credit until exact completion or endpoint disposal", () => {
  const state = fixture();
  const endpoint = state.open(1n);
  state.ready.set(1n, 65);
  state.connections.publish();
  assert.equal(state.pending.get(1n)!.length, 64);
  assert.equal(state.ready.get(1n), 1);
  state.connections.maintain(performance.now() + 30_001);
  assert.ok(state.closed.has(1n));
  assert.equal(state.pending.get(1n)!.length, 64);
  assert.deepEqual(state.disposed, []);
  endpoint.receive({ type: "ack", connection: 1n, delivery: 1n });
  assert.equal(state.pending.get(1n)!.length, 63);
  assert.deepEqual(state.completed, [[1n, 1n]]);
  state.connections.dispose(1n);
  assert.deepEqual(state.disposed, [1n]);
  assert.equal(state.connections.size, 0);
});

test("failed transfer retains tickets and leaves another physical connection live", () => {
  const state = fixture();
  const first = state.open(1n);
  const peer = state.open(2n);
  first.throws = true;
  state.ready.set(1n, 1);
  state.ready.set(2n, 1);
  state.connections.publish();
  assert.ok(state.closed.has(1n));
  assert.ok(!state.closed.has(2n));
  assert.equal(state.pending.get(1n)!.length, 1);
  assert.deepEqual(state.disposed, []);
  peer.receive({ type: "ack", connection: 2n, delivery: 2n });
  assert.deepEqual(state.completed, [[2n, 2n]]);
  state.connections.dispose(1n);
  assert.deepEqual(state.disposed, [1n]);
});

test("foreign and duplicate ACKs never release another delivery", () => {
  const state = fixture();
  const first = state.open(1n);
  const second = state.open(2n);
  state.ready.set(1n, 1);
  state.ready.set(2n, 1);
  state.connections.publish();
  first.receive({ type: "ack", connection: 2n, delivery: 2n });
  assert.deepEqual(state.completed, []);
  assert.equal(state.pending.get(2n)!.length, 1);
  second.receive({ type: "ack", connection: 2n, delivery: 2n });
  second.receive({ type: "ack", connection: 2n, delivery: 2n });
  assert.deepEqual(state.completed, [[2n, 2n]]);
  assert.equal(state.pending.get(1n)!.length, 1);
});

test("closing records bound endpoint churn even without any ACK progress", () => {
  const state = fixture();
  for (let index = 1n; index <= 64n; index++) {
    const endpoint = state.open(index);
    state.ready.set(index, 1);
    state.connections.publish();
    endpoint.receive({ type: "close", connection: index });
  }
  assert.equal(state.connections.size, 64);
  assert.ok(state.open(65n).closed);
  assert.equal(state.pending.size, 64);
  state.connections.dispose(1n);
  assert.ok(!state.open(66n).closed);
  assert.equal(state.connections.size, 64);
});

test("a throttled Host holds input within the credit it granted and returns it on admission", () => {
  const state = fixture();
  const endpoint = state.open(1n);
  const peer = state.open(2n);
  const readiness = endpoint.sent.find((message) => message.type === "ready");
  assert.deepEqual(readiness, {
    type: "ready",
    connection: 1n,
    credit: { messages: 64, bytes: 8 * 1024 },
  });
  state.throttle(1n, true);
  for (let index = 0; index < 64; index++)
    endpoint.receive({
      type: "data",
      connection: 1n,
      bytes: new ArrayBuffer(index < 7 ? 1024 : 16),
    });
  assert.deepEqual(state.received, [], "a throttled Host admits nothing");
  assert.ok(!state.closed.has(1n));
  assert.deepEqual(
    endpoint.sent.filter((message) => message.type === "credit"),
    [],
  );

  state.throttle(1n, false);
  state.connections.pumpInputs();
  assert.equal(state.received.length, 64);
  assert.deepEqual(
    endpoint.sent.filter((message) => message.type === "credit"),
    [
      {
        type: "credit",
        connection: 1n,
        messages: 64,
        bytes: 7 * 1024 + 57 * 16,
      },
    ],
  );
  assert.ok(!state.closed.has(2n));
  assert.deepEqual(
    peer.sent.filter((message) => message.type !== "ready"),
    [],
  );
});

for (const excess of ["messages", "bytes"] as const) {
  test(`a sender exceeding its ${excess} credit fails only its own connection`, () => {
    const state = fixture();
    const endpoint = state.open(1n);
    const peer = state.open(2n);
    state.throttle(1n, true);
    const count = excess === "messages" ? 65 : 9;
    for (let index = 0; index < count; index++)
      endpoint.receive({
        type: "data",
        connection: 1n,
        bytes: new ArrayBuffer(excess === "messages" ? 1 : 1024),
      });
    assert.ok(state.closed.has(1n));
    assert.match(
      String(
        endpoint.sent.find((message) => message.type === "error")?.message,
      ),
      /exceeded its worker ingress credit/,
    );
    peer.receive({ type: "data", connection: 2n, bytes: new ArrayBuffer(4) });
    assert.ok(!state.closed.has(2n));
    assert.deepEqual(state.received, [[2n, 4]]);
  });
}
