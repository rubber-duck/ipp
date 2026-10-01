import type {
  ClientAssetSource,
  MessageTransport,
  Response,
  TransportEvents,
} from "@ipp/client";
import { isAssetSourceResponse } from "../../packages/ipp-client/src/asset-sources.js";
import { HostWireReader } from "../../packages/ipp-client/src/host-protocol.js";
import type { BlenderCleanupGate } from "./blender-headless-scenario.js";

export { workerTransport } from "@ipp/client";
export {
  blenderHeadless,
  blenderCleanup,
} from "./blender-headless-scenario.js";

export function blenderReplyGate(
  contract: { decodeResponse(bytes: Uint8Array, session: bigint): Response },
  transport: MessageTransport,
): BlenderCleanupGate & { transport: MessageTransport } {
  let events: TransportEvents;
  let session = 0n;
  let heldKind: "controller" | "asset" | undefined;
  let heldResolve: (() => void) | undefined;
  const replies: Uint8Array[] = [];
  const commits = new Set<bigint>();
  const releases = new Map<bigint, ClientAssetSource>();
  const published: ClientAssetSource[] = [];
  const released: ClientAssetSource[] = [];
  return {
    published,
    released,
    hold(kind, identity) {
      session = identity;
      heldKind = kind;
      return new Promise<void>((resolve) => {
        heldResolve = resolve;
      });
    },
    release() {
      heldKind = undefined;
      heldResolve = undefined;
      for (const bytes of replies.splice(0)) events.message(bytes);
    },
    transport: {
      start(value) {
        events = value;
        transport.start({
          ...events,
          message(bytes) {
            let hold = false;
            if (isAssetSourceResponse(bytes)) {
              const reader = new HostWireReader(bytes);
              reader.raw(4);
              if (reader.u64() === session) {
                const id = reader.u64();
                const ok = reader.u8() === 0;
                const source = releases.get(id);
                if (source && ok) released.push(source);
                releases.delete(id);
                hold = commits.delete(id) && ok && heldKind === "asset";
              }
            } else if (
              bytes.length >= 25 &&
              new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(
                0,
                true,
              ) === session
            ) {
              const response = contract.decodeResponse(bytes, session);
              hold =
                replies.length > 0 ||
                (heldKind === "controller" &&
                  response.body.kind === "animationController" &&
                  response.body.id !== null);
            }
            if (hold) {
              replies.push(bytes.slice());
              heldResolve?.();
            } else events.message(bytes);
          },
        });
      },
      send(bytes) {
        if (
          bytes[0] === 73 &&
          bytes[1] === 80 &&
          bytes[2] === 65 &&
          bytes[3] === 83
        ) {
          const reader = new HostWireReader(bytes);
          reader.raw(4);
          if (reader.u64() === session) {
            const id = reader.u64();
            const tag = reader.u8();
            if (tag === 2) commits.add(id);
            if (tag === 0 || tag === 4) {
              const source = {
                kind: reader.u32(),
                source: reader.string(),
                variant: reader.u32(),
              };
              if (tag === 0) published.push(source);
              else releases.set(id, source);
            }
          }
        }
        transport.send(bytes);
      },
      close: () => transport.close(),
    },
  };
}
