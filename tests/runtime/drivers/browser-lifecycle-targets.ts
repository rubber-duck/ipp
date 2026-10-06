import type { MessageTransport, Response, TransportEvents } from "@ipp/client";
import type { Client } from "@ipp/client";
import { isAssetSourceResponse } from "../../../packages/ipp-client/src/asset-sources.js";
import {
  lifecycleDiagnostics,
  type LifecycleDiagnosticSample,
} from "../../../packages/ipp-client/src/diagnostics.js";
export { createWorkerHost } from "../../../packages/ipp-client/src/worker.js";

export interface LifecycleTransportProbe {
  statistics(
    client: Client,
    output: bigint,
  ): Promise<LifecycleDiagnosticSample>;
  records(session: bigint): { messages: number; bytes: number };
  requests(session: bigint): { adds: number; removes: number };
  afterRemovalAck(session: bigint, callback: () => void): void;
  /**
   * Answer the session's next lifecycle removal request with the Host's
   * enqueue rejection (code 1) instead of sending it, so the SDK sees a page
   * that was definitely not applied.
   */
  refuseNextRemoval(session: bigint): void;
  hold(
    session: bigint,
    snapshots?: boolean,
  ): {
    waitFor(count: number): Promise<void>;
    release(): number;
  };
}

interface DispatchHold {
  session: bigint;
  snapshots: boolean;
  messages: Uint8Array[];
  waiter?: { count: number; resolve(): void; reject(error: Error): void };
}

/** Delays SDK dispatch, not physical delivery completion or Host credit. */
export function lifecycleTargetTransport(
  codec: {
    WIRE: Readonly<Record<string, number>>;
    decodeResponse(bytes: Uint8Array, session: bigint): Response;
  },
  transport: MessageTransport,
): { transport: MessageTransport; probe: LifecycleTransportProbe } {
  let events: TransportEvents;
  const totals = new Map<bigint, { messages: number; bytes: number }>();
  const requests = new Map<bigint, { adds: number; removes: number }>();
  let afterRemoval: { session: bigint; callback(): void } | undefined;
  let refuseRemoval: bigint | undefined;
  /**
   * The latest World response tick of each session that has sent a request
   * (its bootstrap precedes that); asset source replies carry no tick.
   */
  const ticks = new Map<bigint, bigint>();
  let held: DispatchHold | undefined;
  const fail = (error: Error) => {
    held?.waiter?.reject(error);
  };
  const dispatch = (bytes: Uint8Array) => {
    let callback: (() => void) | undefined;
    if (
      afterRemoval &&
      bytes.length >= 25 &&
      bytes[24] === codec.WIRE.RESPONSE_LIFECYCLE_WATCH
    ) {
      const session = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      ).getBigUint64(0, true);
      const response = codec.decodeResponse(bytes, session);
      if (
        session === afterRemoval.session &&
        response.body.kind === "lifecycleWatch" &&
        response.body.record.kind === "ack" &&
        response.body.record.action === "remove" &&
        response.body.record.result.kind === "applied"
      ) {
        callback = afterRemoval.callback;
        afterRemoval = undefined;
      }
    }
    events.message(bytes);
    callback?.();
  };
  return {
    probe: {
      statistics(client, output) {
        return lifecycleDiagnostics(client).statistics(output);
      },
      records(session) {
        return { ...(totals.get(session) ?? { messages: 0, bytes: 0 }) };
      },
      requests(session) {
        return { ...(requests.get(session) ?? { adds: 0, removes: 0 }) };
      },
      afterRemovalAck(session, callback) {
        if (afterRemoval) throw new Error("Removal ACK hook already installed");
        afterRemoval = { session, callback };
      },
      refuseNextRemoval(session) {
        if (refuseRemoval !== undefined)
          throw new Error("Removal refusal already installed");
        refuseRemoval = session;
      },
      hold(session, snapshots = false) {
        if (held) throw new Error("Lifecycle dispatch already held");
        const current: DispatchHold = { session, snapshots, messages: [] };
        held = current;
        return {
          waitFor(count) {
            if (
              !Number.isInteger(count) ||
              count < 1 ||
              count > 16 ||
              current.waiter
            )
              throw new Error("Invalid lifecycle hold");
            if (current.messages.length >= count) return Promise.resolve();
            return new Promise<void>((resolve, reject) => {
              current.waiter = { count, resolve, reject };
            });
          },
          release() {
            if (held !== current) throw new Error("Stale lifecycle hold");
            held = undefined;
            const messages = current.messages.splice(0);
            for (const bytes of messages) dispatch(bytes);
            return messages.length;
          },
        };
      },
    },
    transport: {
      start(receiver) {
        events = receiver;
        transport.start({
          ready: () => events.ready(),
          error(error) {
            fail(error);
            events.error(error);
          },
          closed() {
            fail(new Error("Transport closed"));
            events.closed();
          },
          message(bytes) {
            if (bytes.length >= 25 && !isAssetSourceResponse(bytes)) {
              const view = new DataView(
                bytes.buffer,
                bytes.byteOffset,
                bytes.byteLength,
              );
              const session = view.getBigUint64(0, true);
              const tick = view.getBigUint64(16, true);
              const latest = ticks.get(session);
              if (latest !== undefined && tick > latest)
                ticks.set(session, tick);
            }
            if (
              bytes.length >= 25 &&
              (bytes[24] === codec.WIRE.RESPONSE_LIFECYCLE_WATCH ||
                (held?.snapshots && bytes[24] === codec.WIRE.RESPONSE_INSPECT))
            ) {
              const session = new DataView(
                bytes.buffer,
                bytes.byteOffset,
                bytes.byteLength,
              ).getBigUint64(0, true);
              const response = codec.decodeResponse(bytes, session);
              if (bytes[24] === codec.WIRE.RESPONSE_LIFECYCLE_WATCH) {
                if (
                  response.body.kind !== "lifecycleWatch" ||
                  response.tick !== 0n
                )
                  throw new Error("Invalid lifecycle wire provenance");
                const total = totals.get(session) ?? { messages: 0, bytes: 0 };
                total.messages++;
                total.bytes += bytes.byteLength;
                totals.set(session, total);
              }
              if (held?.session === session) {
                if (held.messages.length >= 16)
                  throw new Error("Lifecycle test dispatch hold exceeded");
                held.messages.push(bytes.slice());
                if (held.waiter && held.messages.length >= held.waiter.count) {
                  held.waiter.resolve();
                  delete held.waiter;
                }
                return;
              }
            }
            dispatch(bytes);
          },
        });
      },
      send(bytes) {
        if (bytes.length >= 8) {
          const session = new DataView(
            bytes.buffer,
            bytes.byteOffset,
            bytes.byteLength,
          ).getBigUint64(0, true);
          if (!ticks.has(session)) ticks.set(session, 0n);
        }
        const request =
          bytes.length >= 34 && bytes[16] === codec.WIRE.REQUEST_LIFECYCLE_WATCH
            ? {
                session: new DataView(
                  bytes.buffer,
                  bytes.byteOffset,
                  bytes.byteLength,
                ).getBigUint64(0, true),
                action: bytes[33],
              }
            : undefined;
        if (
          request &&
          request.session === refuseRemoval &&
          request.action === codec.WIRE.LIFECYCLE_WATCH_REMOVE
        ) {
          refuseRemoval = undefined;
          const view = new DataView(
            bytes.buffer,
            bytes.byteOffset,
            bytes.byteLength,
          );
          const refused = hostRejection(
            codec.WIRE.RESPONSE_ERROR!,
            request.session,
            view.getBigUint64(8, true),
            ticks.get(request.session) ?? 0n,
          );
          queueMicrotask(() => dispatch(refused));
          return;
        }
        transport.send(bytes);
        if (request) {
          const total = requests.get(request.session) ?? {
            adds: 0,
            removes: 0,
          };
          if (request.action === codec.WIRE.LIFECYCLE_WATCH_ADD) total.adds++;
          else if (request.action === codec.WIRE.LIFECYCLE_WATCH_REMOVE)
            total.removes++;
          else throw new Error("Invalid lifecycle request in probe");
          requests.set(request.session, total);
        }
      },
      async close() {
        fail(new Error("Probe closed"));
        await transport.close();
      },
    },
  };
}

/**
 * A World error response with the Host's enqueue rejection code 1: session,
 * request identity and tick (u64), the response tag, the code (u16) and a
 * length-prefixed UTF-8 message. It carries the session's latest response
 * tick, since World responses never move a session's tick backwards.
 */
function hostRejection(
  tag: number,
  session: bigint,
  request: bigint,
  tick: bigint,
): Uint8Array<ArrayBuffer> {
  const message = new TextEncoder().encode(
    "lifecycle removal refused by probe",
  );
  const bytes = new Uint8Array(25 + 2 + 4 + message.length);
  const view = new DataView(bytes.buffer);
  view.setBigUint64(0, session, true);
  view.setBigUint64(8, request, true);
  view.setBigUint64(16, tick, true);
  bytes[24] = tag;
  view.setUint16(25, 1, true);
  view.setUint32(27, message.length, true);
  bytes.set(message, 31);
  return bytes;
}
