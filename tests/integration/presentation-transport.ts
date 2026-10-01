import {
  bindPresentation,
  presentationOf,
} from "../../packages/ipp-client/src/presentation.js";
import type { MessageTransport, TransportEvents } from "@ipp/client";
export { workerTransport } from "../../packages/ipp-client/src/worker.js";

export interface PresentationTransferProbe {
  holdCaptureReply(): Promise<void>;
  releaseReply(): void;
  captured(): bigint;
  reads(capture: bigint): readonly bigint[];
  releases(capture: bigint): number;
  cancelFrameWithNextClear(): void;
}

export function presentationTransport(
  wire: Readonly<Record<string, number>>,
  transport: MessageTransport,
) {
  let events: TransportEvents;
  let waiting: { resolve(): void; reject(error: Error): void } | undefined;
  let held: Uint8Array | undefined;
  let captured = 0n;
  let lastFrame: bigint | undefined;
  let cancelWithClear = false;
  const reads = new Map<bigint, bigint[]>();
  const releases = new Map<bigint, number>();
  const probe: PresentationTransferProbe = {
    holdCaptureReply() {
      if (waiting || held)
        throw new Error("Only one controlled capture may wait");
      return new Promise((resolve, reject) => {
        waiting = { resolve, reject };
      });
    },
    releaseReply() {
      const bytes = held;
      held = undefined;
      if (bytes) events.message(bytes);
    },
    captured: () => captured,
    reads: (capture) => reads.get(capture) ?? [],
    releases: (capture) => releases.get(capture) ?? 0,
    cancelFrameWithNextClear() {
      if (lastFrame === undefined) throw new Error("No frame to cancel");
      cancelWithClear = true;
    },
  };
  const controlled: MessageTransport = {
    start(value) {
      events = value;
      transport.start({
        ...value,
        message(bytes) {
          if (
            bytes[24] === wire.HOST_RESPONSE_PRESENTATION &&
            bytes[25] === wire.PRESENTATION_RESPONSE_CAPTURE
          ) {
            captured = new DataView(
              bytes.buffer,
              bytes.byteOffset,
              bytes.byteLength,
            ).getBigUint64(bytes.length - 16, true);
            if (waiting) {
              held = bytes.slice();
              waiting.resolve();
              waiting = undefined;
              return;
            }
          }
          value.message(bytes);
        },
      });
    },
    send(bytes) {
      if (bytes[24] === wire.HOST_REQUEST_PRESENTATION) {
        const view = new DataView(
          bytes.buffer,
          bytes.byteOffset,
          bytes.byteLength,
        );
        if (bytes[25] === wire.PRESENTATION_REQUEST_FRAME) {
          lastFrame = view.getBigUint64(16, true);
        } else if (
          bytes[25] === wire.PRESENTATION_REQUEST_CLEAR &&
          cancelWithClear
        ) {
          cancelWithClear = false;
          const cancelled = bytes.slice(0, 34);
          cancelled[25] = wire.PRESENTATION_REQUEST_CANCEL_FRAME!;
          new DataView(cancelled.buffer).setBigUint64(26, lastFrame!, true);
          transport.send(cancelled);
          return;
        } else if (bytes[25] === wire.PRESENTATION_REQUEST_READ_CAPTURE) {
          const capture = view.getBigUint64(26, true);
          const offsets = reads.get(capture) ?? [];
          offsets.push(view.getBigUint64(34, true));
          reads.set(capture, offsets);
        } else if (bytes[25] === wire.PRESENTATION_REQUEST_RELEASE_CAPTURE) {
          const capture = view.getBigUint64(26, true);
          releases.set(capture, (releases.get(capture) ?? 0) + 1);
        }
      }
      transport.send(bytes);
    },
    close() {
      waiting?.reject(new Error("Controlled presentation transport closed"));
      waiting = undefined;
      held = undefined;
      return transport.close();
    },
  };
  // Statistics and testing controls reach the wrapped presentation.
  bindPresentation(controlled, presentationOf(transport));
  return { transport: controlled, probe };
}
