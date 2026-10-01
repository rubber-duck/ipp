import type {
  BatchOutcome,
  MessageTransport,
  Response,
  TransportEvents,
} from "@ipp/client";
export { webSocketTransport } from "../../packages/ipp-client/src/transport.js";
export { workerTransport } from "../../packages/ipp-client/src/worker.js";

export interface GuiTransportProbe {
  holdPage(session: bigint): Promise<void>;
  releasePage(): void;
  /** Replace the next successful batch outcome of `session` with a
   * well-formed reply of another kind or a Host error; resolve with the
   * outcome the Host actually sent. */
  corruptApplied(
    session: bigint,
    kind: "outcome" | "error",
  ): Promise<BatchOutcome>;
  /** Final batch pages the session submitted. */
  batches(session: bigint): number;
  holdObservations(session: bigint): {
    first: Promise<void>;
    waitFor(count: number): Promise<void>;
    release(controlErrorCode?: number): number;
  };
}

interface ObservationHold {
  session: bigint;
  bytes: Uint8Array[];
  first(): void;
  fail(error: Error): void;
  waiter?: { count: number; resolve(): void; reject(error: Error): void };
}

export function guiLocalTransport(
  codec: {
    WIRE: Readonly<Record<string, number>>;
    decodeResponse(bytes: Uint8Array, session: bigint): Response;
  },
  transport: MessageTransport,
): { transport: MessageTransport; probe: GuiTransportProbe } {
  let events: TransportEvents;
  let page:
    | {
        session: bigint;
        request?: bigint;
        resolve(): void;
        reject(error: Error): void;
      }
    | undefined;
  let held: Uint8Array | undefined;
  let corruption:
    | {
        session: bigint;
        kind: "outcome" | "error";
        heldInspection?: Uint8Array;
        resolve(outcome: BatchOutcome): void;
        reject(error: Error): void;
      }
    | undefined;
  const batches = new Map<bigint, number>();
  let observationHold: ObservationHold | undefined;
  const probe: GuiTransportProbe = {
    holdObservations(session) {
      if (observationHold) throw new Error("Observation delivery already held");
      let first!: () => void;
      let fail!: (error: Error) => void;
      const ready = new Promise<void>((resolve, reject) => {
        first = resolve;
        fail = reject;
      });
      const held: ObservationHold = { session, bytes: [], first, fail };
      observationHold = held;
      return {
        first: ready,
        waitFor(count) {
          if (
            !Number.isInteger(count) ||
            count < 1 ||
            count > 16 ||
            held.waiter
          )
            throw new Error("Invalid observation wait");
          if (held.bytes.length >= count) return Promise.resolve();
          return new Promise<void>((resolve, reject) => {
            held.waiter = { count, resolve, reject };
          });
        },
        release(controlErrorCode) {
          if (observationHold !== held)
            throw new Error("Observation hold is stale");
          observationHold = undefined;
          const bytes = held.bytes.splice(0);
          let corrupted = false;
          for (const message of bytes) {
            const view = new DataView(
              message.buffer,
              message.byteOffset,
              message.byteLength,
            );
            if (
              controlErrorCode !== undefined &&
              !corrupted &&
              view.getBigUint64(8, true) !== 0n
            ) {
              const reason = new TextEncoder().encode(
                "control outcome unavailable",
              );
              const failure = new Uint8Array(31 + reason.length);
              failure.set(message.subarray(0, 24));
              failure[24] = codec.WIRE.RESPONSE_ERROR!;
              new DataView(failure.buffer).setUint16(
                25,
                controlErrorCode,
                true,
              );
              new DataView(failure.buffer).setUint32(27, reason.length, true);
              failure.set(reason, 31);
              events.message(failure);
              corrupted = true;
            } else events.message(message);
          }
          if (controlErrorCode !== undefined && !corrupted)
            throw new Error("No correlated control ACK to corrupt");
          return bytes.length;
        },
      };
    },
    holdPage(session) {
      if (page || held) throw new Error("A page ACK is already held");
      return new Promise((resolve, reject) => {
        page = { session, resolve, reject };
      });
    },
    releasePage() {
      const bytes = held;
      held = undefined;
      if (bytes) events.message(bytes);
    },
    corruptApplied(session, kind) {
      if (corruption)
        throw new Error("A batch outcome corruption is already pending");
      return new Promise((resolve, reject) => {
        corruption = { session, kind, resolve, reject };
      });
    },
    batches: (session) => batches.get(session) ?? 0,
  };
  return {
    probe,
    transport: {
      start(value) {
        events = value;
        transport.start({
          ...value,
          message(bytes) {
            if (bytes.length >= 25) {
              const view = new DataView(
                bytes.buffer,
                bytes.byteOffset,
                bytes.byteLength,
              );
              const session = view.getBigUint64(0, true);
              const request = view.getBigUint64(8, true);
              if (
                observationHold &&
                session === observationHold.session &&
                bytes[24] === codec.WIRE.RESPONSE_GUI_OBSERVATION
              ) {
                if (observationHold.bytes.length >= 16)
                  throw new Error("Observation hold overflow");
                observationHold.bytes.push(bytes.slice());
                observationHold.first();
                if (
                  observationHold.waiter &&
                  observationHold.bytes.length >= observationHold.waiter.count
                ) {
                  observationHold.waiter.resolve();
                  delete observationHold.waiter;
                }
                return;
              }
              if (
                page &&
                session === page.session &&
                request === page.request
              ) {
                held = bytes.slice();
                page.resolve();
                page = undefined;
                return;
              }
              if (
                corruption &&
                session === corruption.session &&
                bytes[24] === codec.WIRE.RESPONSE_INSPECT
              ) {
                if (corruption.heldInspection)
                  throw new Error(
                    "Corruption pending-inspection hold overflow",
                  );
                corruption.heldInspection = bytes.slice();
                return;
              }
              if (
                corruption &&
                session === corruption.session &&
                request !== 0n &&
                bytes[24] === codec.WIRE.RESPONSE_BATCH
              ) {
                const response = codec.decodeResponse(bytes, session);
                if (
                  response.body.kind === "batch" &&
                  response.body.outcome.ok
                ) {
                  const message = new TextEncoder().encode(
                    "committed outcome unavailable",
                  );
                  const corrupted = new Uint8Array(
                    corruption.kind === "error" ? 31 + message.length : 34,
                  );
                  corrupted.set(bytes.subarray(0, 24));
                  if (corruption.kind === "error") {
                    corrupted[24] = codec.WIRE.RESPONSE_ERROR!;
                    const output = new DataView(corrupted.buffer);
                    output.setUint16(25, 3, true);
                    output.setUint32(27, message.length, true);
                    corrupted.set(message, 31);
                  } else {
                    // A well-formed correlated reply of another kind.
                    corrupted[24] = codec.WIRE.RESPONSE_ATTACHMENT_RECEIPT!;
                  }
                  const pending = corruption;
                  corruption = undefined;
                  events.message(corrupted);
                  if (pending.heldInspection)
                    events.message(pending.heldInspection);
                  pending.resolve(response.body.outcome);
                  return;
                }
              }
            }
            events.message(bytes);
          },
        });
      },
      send(bytes) {
        if (bytes.length >= 17) {
          const view = new DataView(
            bytes.buffer,
            bytes.byteOffset,
            bytes.byteLength,
          );
          const session = view.getBigUint64(0, true);
          if (
            page &&
            session === page.session &&
            page.request === undefined &&
            bytes[16] === codec.WIRE.REQUEST_SUBMIT_BATCH &&
            view.getBigUint64(8, true) !== 0n
          )
            page.request = view.getBigUint64(8, true);
          if (
            bytes[16] === codec.WIRE.REQUEST_SUBMIT_BATCH &&
            view.getBigUint64(8, true) !== 0n
          )
            batches.set(session, (batches.get(session) ?? 0) + 1);
        }
        transport.send(bytes);
      },
      close() {
        const error = new Error("Controlled GUI transport closed");
        page?.reject(error);
        corruption?.reject(error);
        observationHold?.fail(error);
        observationHold?.waiter?.reject(error);
        observationHold = undefined;
        page = undefined;
        corruption = undefined;
        held = undefined;
        return transport.close();
      },
    },
  };
}
