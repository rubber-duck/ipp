/**
 * Presentation of a native GLES testing host, for `@ipp/client/testing`.
 *
 * The native WebSocket carries only the IPP wire. The `gles_host` example of
 * `ipp-server` serves a second loopback WebSocket whose messages mirror the
 * worker's presentation envelope in a small binary layout; the Rust half lives
 * in `crates/ipp-server/examples/gles_presentation/channel.rs`. Requests start
 * with a kind byte followed by little-endian fields:
 *
 * | Kind | Request | Fields |
 * | --- | --- | --- |
 * | 1 | frame | id u32, session u64, afterTick u64, readback u8 |
 * | 2 | frame-cancel | id u32 |
 * | 3 | resize | width u32, height u32 |
 * | 4 | glyph-atlas-limits | maxPages u32, idlePagePublications u32 |
 * | 5 | surface-cache-budget | bytes u32 |
 * | 6 | exhaustive-draw-checks | enabled u8 |
 * | 7 | context-loss | |
 * | 8 | context-restore | |
 *
 * Responses are viewport limits (1: maxWidth u32, maxHeight u32), a frame
 * error (2: id u32, UTF-8 message), a frame (3: id u32, header length u32,
 * JSON header, then the RGBA pixels of a capture) and a failure of a request
 * the host build cannot honour (4: UTF-8 message), which fails the connection.
 */
import {
  PortPresentation,
  SURFACE_CACHE_MODES,
  type FrameCapture,
  type PresentationTestingMessage,
} from "./presentation.js";
import { webSocketTransport, type MessageTransport } from "./transport.js";

type PresentationRequest =
  | {
      type: "frame";
      id: number;
      session: bigint;
      afterTick: bigint;
      readback: boolean;
    }
  | { type: "frame-cancel"; id: number }
  | { type: "resize"; width: number; height: number }
  | PresentationTestingMessage;

function encodeRequest(message: PresentationRequest): ArrayBuffer {
  const bytes = new ArrayBuffer(22);
  const view = new DataView(bytes);
  const kind = (value: number, length: number) => {
    view.setUint8(0, value);
    return bytes.slice(0, length);
  };
  switch (message.type) {
    case "frame":
      view.setUint32(1, message.id, true);
      view.setBigUint64(5, message.session, true);
      view.setBigUint64(13, message.afterTick, true);
      view.setUint8(21, Number(message.readback));
      return kind(1, 22);
    case "frame-cancel":
      view.setUint32(1, message.id, true);
      return kind(2, 5);
    case "resize":
      view.setUint32(1, message.width, true);
      view.setUint32(5, message.height, true);
      return kind(3, 9);
    case "glyph-atlas-limits":
      view.setUint32(1, message.maxPages, true);
      view.setUint32(5, message.idlePagePublications, true);
      return kind(4, 9);
    case "surface-cache-budget":
      view.setUint32(1, message.bytes, true);
      return kind(5, 5);
    case "exhaustive-draw-checks":
      view.setUint8(1, Number(message.enabled));
      return kind(6, 2);
    case "context-loss":
      return kind(7, 1);
    case "context-restore":
      return kind(8, 1);
  }
}

/** Translate one host response into the worker envelope `PortPresentation` reads. */
function decodeResponse(data: unknown): Record<string, unknown> {
  if (!(data instanceof ArrayBuffer) || data.byteLength < 1)
    throw new Error("Expected a binary presentation response");
  const view = new DataView(data);
  const text = (offset: number) =>
    new TextDecoder("utf-8", { fatal: true }).decode(
      new Uint8Array(data, offset),
    );
  switch (view.getUint8(0)) {
    case 1:
      return {
        type: "viewport-limits",
        limits: {
          maxWidth: view.getUint32(1, true),
          maxHeight: view.getUint32(5, true),
        },
      };
    case 2:
      return {
        type: "frame-error",
        id: view.getUint32(1, true),
        message: text(5),
      };
    case 3: {
      const length = view.getUint32(5, true);
      const header = JSON.parse(
        new TextDecoder("utf-8", { fatal: true }).decode(
          new Uint8Array(data, 9, length),
        ),
      ) as Record<string, unknown>;
      const frame: Record<string, unknown> = {
        ...header,
        session: BigInt(header.session as string),
        tick: BigInt(header.tick as string),
      };
      if (9 + length < data.byteLength) frame.pixels = data.slice(9 + length);
      const surfaces = (header.statistics as FrameCapture["statistics"])
        ?.surfaces as { surfaceCaches: Record<string, unknown>[] } | undefined;
      if (surfaces)
        surfaces.surfaceCaches = surfaces.surfaceCaches.map((record) => {
          const mode = SURFACE_CACHE_MODES[record.mode as number];
          if (!mode) throw new Error("Unknown Surface cache presentation code");
          return { ...record, entity: BigInt(record.entity as string), mode };
        });
      return { type: "frame-result", id: view.getUint32(1, true), frame };
    }
    case 4:
      throw new Error(text(1));
    default:
      throw new Error("Unknown presentation response");
  }
}

/**
 * Connect to a native GLES testing host: the IPP WebSocket at `url` paired
 * with its presentation channel at `presentationUrl`. Pass the transport to a
 * generated `IppHostClient.connectTransport`; attached World clients then
 * expose `presentation` exactly as on a worker, including the controls of
 * {@link presentationTesting}.
 */
export function nativePresentationTransport(
  url: string,
  presentationUrl: string,
): MessageTransport {
  const ipp = webSocketTransport(url);
  const channel = new WebSocket(presentationUrl);
  channel.binaryType = "arraybuffer";
  const presentation = new PortPresentation((message) => {
    if (channel.readyState !== WebSocket.OPEN)
      throw new Error("Presentation channel is closed");
    channel.send(encodeRequest(message as PresentationRequest));
  });
  const closed = () => new Error("Presentation channel closed");

  return {
    presentation,
    start(events) {
      let ippReady = false;
      let channelReady = false;
      let stopped = false;
      const fail = (error: Error) => {
        if (stopped) return;
        stopped = true;
        presentation.close(error);
        events.error(error);
      };
      const ready = () => {
        if (ippReady && channelReady && !stopped) events.ready();
      };
      channel.onopen = () => {
        channelReady = true;
        ready();
      };
      channel.onmessage = (event: MessageEvent<unknown>) => {
        try {
          presentation.receive(decodeResponse(event.data));
        } catch (error) {
          fail(error instanceof Error ? error : new Error(String(error)));
        }
      };
      channel.onerror = () => fail(new Error("Presentation channel error"));
      channel.onclose = () => presentation.close(closed());
      ipp.start({
        ready: () => {
          ippReady = true;
          ready();
        },
        message: (bytes) => events.message(bytes),
        error: fail,
        closed: () => {
          stopped = true;
          presentation.close(closed());
          events.closed();
        },
      });
    },
    send: (bytes) => ipp.send(bytes),
    async close() {
      presentation.close(closed());
      channel.onopen =
        channel.onmessage =
        channel.onerror =
        channel.onclose =
          null;
      channel.close();
      await ipp.close();
    },
  };
}
