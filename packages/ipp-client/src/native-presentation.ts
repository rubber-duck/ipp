/**
 * Testing companion to the ordinary native Host WebSocket of the GLES test
 * host: renderer statistics and, in its `instrumentation` build, testing
 * controls. Frames and pixels use the ordinary Host protocol.
 */
import {
  PortRenderDiagnostics,
  SURFACE_CACHE_MODES,
  bindPresentation,
  type RenderStatisticsSnapshot,
  type PresentationTestingMessage,
} from "./presentation.js";
import { webSocketTransport, type MessageTransport } from "./transport.js";

type PresentationRequest =
  | PresentationTestingMessage
  | { type: "render-statistics"; id: number };

function encodeRequest(message: PresentationRequest): ArrayBuffer {
  const bytes = new ArrayBuffer(9);
  const view = new DataView(bytes);
  const kind = (value: number, length: number) => {
    view.setUint8(0, value);
    return bytes.slice(0, length);
  };
  switch (message.type) {
    case "glyph-atlas-limits":
      view.setUint32(1, message.maxPages, true);
      view.setUint32(5, message.idlePageFrames, true);
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
    case "render-statistics":
      view.setUint32(1, message.id, true);
      return kind(9, 5);
  }
}

function decodeResponse(data: unknown): Record<string, unknown> {
  if (!(data instanceof ArrayBuffer) || data.byteLength < 1)
    throw new Error("Expected binary renderer diagnostics");
  const view = new DataView(data);
  const text = (offset: number) =>
    new TextDecoder("utf-8", { fatal: true }).decode(
      new Uint8Array(data, offset),
    );
  switch (view.getUint8(0)) {
    case 1:
      if (data.byteLength !== 9) throw new Error("Invalid viewport limits");
      return {
        type: "viewport-limits",
        limits: {
          maxWidth: view.getUint32(1, true),
          maxHeight: view.getUint32(5, true),
        },
      };
    case 2:
      if (data.byteLength !== 2 || view.getUint8(1) > 1)
        throw new Error("Invalid presentation configuration");
      return {
        type: "presentation-configuration",
        instrumentation: view.getUint8(1) === 1,
      };
    case 4:
      throw new Error(text(1));
    case 5: {
      const id = view.getUint32(1, true);
      const statistics = JSON.parse(text(5)) as RenderStatisticsSnapshot;
      for (const cache of statistics.surfaces.surfaceCaches) {
        cache.entity = BigInt(cache.entity);
        if (typeof cache.mode === "number")
          cache.mode = SURFACE_CACHE_MODES[cache.mode]!;
      }
      return { type: "render-statistics", id, statistics };
    }
    default:
      throw new Error("Unknown renderer diagnostics");
  }
}

export function nativePresentationTransport(
  url: string,
  presentationUrl: string,
): MessageTransport {
  const ipp = webSocketTransport(url);
  const channel = new WebSocket(presentationUrl);
  channel.binaryType = "arraybuffer";
  const presentation = new PortRenderDiagnostics((message) => {
    if (channel.readyState !== WebSocket.OPEN)
      throw new Error("Presentation channel is closed");
    channel.send(encodeRequest(message as PresentationRequest));
  });
  const closed = () => new Error("Presentation channel closed");

  const transport: MessageTransport = {
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
  return bindPresentation(transport, presentation);
}
