import assert from "node:assert/strict";
import { MessageChannel } from "node:worker_threads";
import test from "node:test";
import { PortTransport } from "../src/transport.js";

test("worker close tolerates a frame longer than one second within its configured deadline", async () => {
  const channel = new MessageChannel();
  let disposed = 0;
  const transport = new PortTransport(
    channel.port1 as unknown as MessagePort,
    () => {
      disposed++;
    },
    false,
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
    setTimeout(() => channel.port2.postMessage({ type: "closed" }), 1200);
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
    () => {
      disposed++;
    },
    false,
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
