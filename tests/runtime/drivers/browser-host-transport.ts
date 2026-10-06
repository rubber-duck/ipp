import type {
  Client,
  ConnectOptions,
  WorldPersistenceHostClient,
  MessageTransport,
  TransportEvents,
} from "@ipp/client";
import { HostWireWriter } from "../../../packages/ipp-client/src/host-protocol.js";
import type { HostLifecycleParticipant } from "../scenarios/host-lifecycle.js";

export { workerTransport } from "../../../packages/ipp-client/src/worker.js";
export { hostLifecycle } from "../scenarios/host-lifecycle.js";
export { graphTransfers } from "../scenarios/graph-transfers.js";
export { attachmentReceipts } from "../scenarios/attachment-receipts.js";
export { streamedWorldCommands } from "../scenarios/command-batches.js";

export interface HostLifecycleContract {
  readonly WIRE: Readonly<Record<string, number>>;
  readonly IppHostClient: {
    connectTransport(
      transport: MessageTransport,
      options?: ConnectOptions,
    ): Promise<WorldPersistenceHostClient<Client>>;
  };
  readonly IppClient: {
    connectTransport(
      transport: MessageTransport,
      options?: ConnectOptions,
    ): Promise<Client>;
  };
}

export function hostLifecycleParticipant(
  contract: HostLifecycleContract,
  transport: MessageTransport,
): HostLifecycleParticipant {
  let events: TransportEvents;
  let heldTag: number | undefined;
  let heldResolve: (() => void) | undefined;
  let truncatedTag: number | undefined;
  let failAfterSend = false;
  let latestTransferJob = 0n;
  let transferOverride:
    | { kind: "cancel" | "read" | "ack"; job: bigint }
    | undefined;
  let rejectDestroy = false;
  const replies: Uint8Array[] = [];
  let closing: Promise<void> | undefined;
  let closeCalls = 0;
  let resolveClosed: () => void;
  let rejectClosed: (error: unknown) => void;
  const closed = new Promise<void>((resolve, reject) => {
    resolveClosed = resolve;
    rejectClosed = reject;
  });
  void closed.catch(() => {});
  const tag = (name: string): number => {
    const value =
      contract.WIRE[
        `HOST_RESPONSE_${name.replace(/([a-z])([A-Z])/g, "$1_$2").toUpperCase()}`
      ];
    if (value === undefined)
      throw new Error(`Missing Host response contract ${name}`);
    return value;
  };
  const controlled: MessageTransport = {
    start(value) {
      events = value;
      transport.start({
        ...events,
        message(bytes) {
          const hostResponse =
            bytes[0] === 73 &&
            bytes[1] === 80 &&
            bytes[2] === 80 &&
            bytes[3] === 65 &&
            bytes.length >= 25;
          if (
            hostResponse &&
            bytes[24] === contract.WIRE.HOST_RESPONSE_TRANSFER
          ) {
            latestTransferJob = new DataView(
              bytes.buffer,
              bytes.byteOffset,
              bytes.byteLength,
            ).getBigUint64(25, true);
          }
          if (hostResponse && bytes[24] === truncatedTag) {
            truncatedTag = undefined;
            events.message(bytes.slice(0, 25));
          } else if (hostResponse && bytes[24] === heldTag) {
            replies.push(bytes.slice());
            heldResolve?.();
          } else events.message(bytes);
        },
      });
    },
    send(bytes) {
      if (
        rejectDestroy &&
        bytes[24] === contract.WIRE.HOST_REQUEST_DESTROY_WORLD
      ) {
        rejectDestroy = false;
        bytes = bytes.slice();
        const view = new DataView(
          bytes.buffer,
          bytes.byteOffset,
          bytes.byteLength,
        );
        view.setBigUint64(33, view.getBigUint64(33, true) + 1n, true);
      }
      if (transferOverride) {
        const { kind, job } = transferOverride;
        transferOverride = undefined;
        const writer = new HostWireWriter();
        writer.raw(bytes.subarray(0, 24));
        writer.u8(
          contract.WIRE[
            kind === "cancel"
              ? "HOST_REQUEST_CANCEL_WORLD_TRANSFER"
              : kind === "read"
                ? "HOST_REQUEST_READ_WORLD_LOAD_BINDINGS"
                : "HOST_REQUEST_ACKNOWLEDGE_WORLD_LOAD"
          ]!,
        );
        writer.u64(job);
        if (kind === "read") writer.u32(0);
        bytes = writer.finish();
      }
      transport.send(bytes);
      if (failAfterSend) {
        failAfterSend = false;
        throw new Error("Injected post-send transport failure");
      }
    },
    close() {
      if (!closing) {
        closeCalls++;
        closing = transport.close();
        void closing.then(resolveClosed, rejectClosed);
      }
      return closing;
    },
  };
  return {
    connectHost: () =>
      contract.IppHostClient.connectTransport(controlled, { logLevel: "off" }),
    connectClient: (options) =>
      contract.IppClient.connectTransport(controlled, {
        ...options,
        logLevel: "off",
      }),
    holdReplies(name) {
      heldTag = tag(name);
      return new Promise<void>((resolve) => {
        heldResolve = resolve;
      });
    },
    releaseReplies() {
      heldTag = undefined;
      for (const bytes of replies.splice(0)) events.message(bytes);
    },
    truncateReply(name) {
      truncatedTag = tag(name);
    },
    throwAfterSend() {
      failAfterSend = true;
    },
    get latestTransferJob() {
      return latestTransferJob;
    },
    replaceNextHostRequestWithTransfer(kind, job) {
      transferOverride = { kind, job };
    },
    rejectNextWorldDestroy() {
      rejectDestroy = true;
    },
    closed,
    get closeCalls() {
      return closeCalls;
    },
    close: () => controlled.close(),
  };
}
