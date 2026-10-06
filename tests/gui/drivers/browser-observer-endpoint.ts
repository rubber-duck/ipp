import { PortTransport } from "../../../packages/ipp-client/src/transport.js";
import type { GuiWorldClient } from "@ipp/client";
import type { WorkerConnectionExports } from "../../../packages/ipp-client/src/worker-connections.js";

export interface DeliveryTrace {
  operation:
    | "copied"
    | "transferred"
    | "complete"
    | "close"
    | "dispose"
    | "sample"
    | "failed";
  connection: bigint;
  delivery: bigint;
  pendingBefore: number;
  pendingAfter: number;
  retainedBytes: number;
  bytes: number;
  result: number;
  reason?: string;
}

async function traceRuntime(workerScript: string, channelName: string) {
  const channel = new BroadcastChannel(channelName);
  const pendingInit: MessageEvent[] = [];
  globalThis.onmessage = (event: MessageEvent) => pendingInit.push(event);
  const instantiate = WebAssembly.instantiate.bind(WebAssembly);
  const copied = new Map<bigint, Map<bigint, number>>();
  let runtime: WorkerConnectionExports;
  const retained = (connection: bigint) =>
    [...(copied.get(connection)?.values() ?? [])].reduce(
      (sum, bytes) => sum + bytes,
      0,
    );
  const emit = (record: DeliveryTrace) => channel.postMessage(record);
  channel.onmessage = (event: MessageEvent<{ connection: bigint }>) => {
    const connection = event.data.connection;
    const pending = runtime.ipp_connection_pending(connection);
    emit({
      operation: "sample",
      connection,
      delivery: 0n,
      pendingBefore: pending,
      pendingAfter: pending,
      retainedBytes: retained(connection),
      bytes: 0,
      result: 1,
    });
  };

  const instrument = async (
    bytes: BufferSource,
    imports?: WebAssembly.Imports,
  ) => {
    const result = await instantiate(bytes, imports);
    runtime = result.instance.exports as unknown as WorkerConnectionExports;
    const exports = { ...result.instance.exports };
    exports.ipp_output_copied = (connection: bigint, delivery: bigint) => {
      const bytes = runtime.ipp_output_len();
      const pendingBefore = runtime.ipp_connection_pending(connection);
      const result = runtime.ipp_output_copied(connection, delivery);
      if (result === 1) {
        let entries = copied.get(connection);
        if (!entries) copied.set(connection, (entries = new Map()));
        entries.set(delivery, bytes);
      }
      emit({
        operation: "copied",
        connection,
        delivery,
        bytes,
        pendingBefore,
        pendingAfter: runtime.ipp_connection_pending(connection),
        retainedBytes: retained(connection),
        result,
      });
      return result;
    };
    exports.ipp_delivery_complete = (connection: bigint, delivery: bigint) => {
      const pendingBefore = runtime.ipp_connection_pending(connection);
      const bytes = copied.get(connection)?.get(delivery) ?? 0;
      const result = runtime.ipp_delivery_complete(connection, delivery);
      if (result === 1) copied.get(connection)?.delete(delivery);
      emit({
        operation: "complete",
        connection,
        delivery,
        bytes,
        pendingBefore,
        pendingAfter: runtime.ipp_connection_pending(connection),
        retainedBytes: retained(connection),
        result,
      });
      return result;
    };
    for (const [name, operation] of [
      ["ipp_connection_close", "close"],
      ["ipp_connection_dispose", "dispose"],
    ] as const) {
      exports[name] = (connection: bigint) => {
        const pendingBefore = runtime.ipp_connection_pending(connection);
        const bytes = retained(connection);
        const result = runtime[name](connection);
        if (operation === "dispose" && result === 1) copied.delete(connection);
        emit({
          operation,
          connection,
          delivery: 0n,
          bytes,
          pendingBefore,
          pendingAfter: runtime.ipp_connection_pending(connection),
          retainedBytes: retained(connection),
          result,
        });
        return result;
      };
    }
    return {
      module: result.module,
      instance: { exports } as WebAssembly.Instance,
    };
  };
  WebAssembly.instantiate = instrument as typeof WebAssembly.instantiate;
  const post = MessagePort.prototype.postMessage;
  MessagePort.prototype.postMessage = function (message: any, transfer: any) {
    const bytes =
      message?.type === "data" &&
      typeof message.delivery === "bigint" &&
      message.bytes instanceof ArrayBuffer
        ? message.bytes.byteLength
        : 0;
    post.call(this, message, transfer);
    if (message?.type === "error" && typeof message.connection === "bigint") {
      const pending = runtime.ipp_connection_pending(message.connection);
      emit({
        operation: "failed",
        connection: message.connection,
        delivery: 0n,
        pendingBefore: pending,
        pendingAfter: pending,
        retainedBytes: retained(message.connection),
        bytes: 0,
        result: 1,
        reason: message.message,
      });
    }
    if (bytes) {
      if (message.bytes.byteLength !== 0)
        throw new Error("Output was copied rather than transferred");
      const pending = runtime.ipp_connection_pending(message.connection);
      emit({
        operation: "transferred",
        connection: message.connection,
        delivery: message.delivery,
        bytes,
        pendingBefore: pending,
        pendingAfter: pending,
        retainedBytes: retained(message.connection),
        result: 1,
      });
    }
  };
  await import(workerScript);
  const receive = globalThis.onmessage as (event: MessageEvent) => void;
  for (const event of pendingInit) receive(event);
}

let clients: GuiWorldClient[] = [];
let outputBytes = 0;
let outputMessages = 0;
let measuring = false;
let disposed = false;
let resumedAt = 0;
let stalledAt = 0;

const parameters = new URL(globalThis.location.href).searchParams;
if (parameters.has("runtime")) {
  void traceRuntime(parameters.get("runtime")!, parameters.get("trace")!).catch(
    (error) => {
      throw error;
    },
  );
} else
  globalThis.onmessage = (event: MessageEvent) => {
    const input = event.data;
    if (input.type === "stall") {
      measuring = true;
      for (let index = 0; index < 7; index++) {
        void clients[index % clients.length]!.inspectPage({
          collection: "entities",
          limit: 15,
        }).catch(() => {});
      }
      stalledAt = performance.now();
      globalThis.postMessage({ type: "stalled" });
      if (input.crash) while (true) {}
      while (performance.now() - stalledAt < 10_000) {}
      resumedAt = performance.now();
      globalThis.postMessage({
        type: "resumed",
        milliseconds: resumedAt - stalledAt,
      });
      return;
    }
    if (input.type !== "init")
      throw new Error("Unexpected endpoint test control");
    void (async () => {
      const contract = await import(input.generated);
      const port = input.port as MessagePort;
      port.addEventListener("message", (message: MessageEvent) => {
        if (measuring && message.data.type === "data") {
          outputBytes += message.data.bytes.byteLength;
          outputMessages++;
        }
      });
      const transport = new PortTransport(port, input.connection, () => {
        disposed = true;
        globalThis.postMessage({ type: "disposed" });
      });
      const host = await contract.IppHostClient.connectTransport(transport, {
        timeoutMs: 20_000,
      });
      clients = [
        await host.openWorld(input.world),
        await host.openWorld(input.world),
      ];
      for (const client of clients) await client.subscribeGuiEffects(() => {});
      globalThis.postMessage({ type: "ready" });
      const closed = await clients[0]!.closed;
      await clients[1]!.closed;
      if (!disposed || resumedAt === 0)
        throw new Error(
          "Endpoint did not resume and dispose before completion",
        );
      globalThis.postMessage({
        type: "failed",
        reason: closed.reason.message,
        outputBytes,
        outputMessages,
        milliseconds: resumedAt - stalledAt,
      });
      await host.close();
    })().catch((error) =>
      globalThis.postMessage({ type: "test-error", message: String(error) }),
    );
  };
