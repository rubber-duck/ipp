import {
  resourceUrlMappings,
  type ResourceUrlMapping,
} from "./resource-urls.js";
import { PortTransport, type MessageTransport } from "./transport.js";
import { PortRenderDiagnostics, validateViewport } from "./presentation.js";
import type { LogLevel } from "./logging.js";
import { validateOptions } from "./client.js";

export interface WorkerOptions {
  timeoutMs?: number;
  canvas?: OffscreenCanvas;
  logLevel?: LogLevel;
  /** Unused-asset cache target in bytes; omitted keeps the Host default (64 MiB). */
  assetCacheBytes?: number;
  resourceUrls?: readonly ResourceUrlMapping[];
}

/** Transferable receiving endpoint. Dispose only after closing it or stopping its owner. */
export interface WorkerEndpoint {
  readonly connection: bigint;
  readonly port: MessagePort;
  dispose(): void;
}

/** One physical Host/surface and clock, independently of all authoring connections. */
export interface WorkerHost {
  connect(): MessageTransport;
  openPort(): WorkerEndpoint;
  close(): Promise<void>;
}

const MAX_ASSET_CACHE_BYTES = 0xffff_ffff;

export function createWorkerHost(
  workerUrl: string | URL,
  wasmUrl: string | URL,
  maxMessageBytes: number,
  options: WorkerOptions = {},
): WorkerHost {
  validateOptions(options);
  if (!Number.isSafeInteger(maxMessageBytes) || maxMessageBytes <= 0)
    throw new RangeError(
      "maxMessageBytes must be the target contract's positive message budget",
    );
  if (
    options.assetCacheBytes !== undefined &&
    (!Number.isInteger(options.assetCacheBytes) ||
      options.assetCacheBytes < 0 ||
      options.assetCacheBytes > MAX_ASSET_CACHE_BYTES)
  )
    throw new RangeError(
      `assetCacheBytes must be an integer in [0, ${MAX_ASSET_CACHE_BYTES}]`,
    );
  const resourceUrls = resourceUrlMappings(options.resourceUrls);
  if (options.canvas)
    validateViewport(options.canvas.width, options.canvas.height);
  const worker = new Worker(workerUrl, { type: "module", name: "ipp-runtime" });
  const control = new MessageChannel();
  const records = new Map<
    bigint,
    { endpoint: WorkerEndpoint; transport?: PortTransport }
  >();
  const timeoutMs = options.timeoutMs ?? 10_000;
  let nextConnection = 1n;
  let stopped = false;
  let failure: Error | undefined;
  let closing: Promise<void> | undefined;
  let finishClose: (() => void) | undefined;
  const visibilityOwner =
    typeof document === "undefined" ? undefined : document;
  const diagnostics = options.canvas
    ? new PortRenderDiagnostics((message) => {
        if (failure) throw failure;
        if (stopped || closing) throw new Error("Worker Host is closed");
        control.port1.postMessage(message);
      })
    : undefined;
  const visibilityChanged = () =>
    control.port1.postMessage({
      type: "visibility",
      hidden: visibilityOwner?.hidden ?? false,
    });
  const terminate = () => {
    if (stopped) return;
    stopped = true;
    visibilityOwner?.removeEventListener("visibilitychange", visibilityChanged);
    worker.removeEventListener("error", failed);
    worker.removeEventListener("messageerror", messageFailed);
    diagnostics?.close(new Error("Worker Host closed"));
    control.port1.onmessage = control.port1.onmessageerror = null;
    control.port1.close();
    worker.terminate();
    finishClose?.();
  };
  const fail = (error: Error) => {
    if (stopped) return;
    failure ??= error;
    for (const record of [...records.values()]) record.transport?.fail(error);
    diagnostics?.close(error);
    if (records.size === 0) terminate();
  };
  const failed = (event: ErrorEvent) =>
    fail(new Error(event.message || "Worker failed"));
  const messageFailed = () => fail(new Error("Worker message error"));
  control.port1.onmessageerror = messageFailed;
  control.port1.onmessage = (event: MessageEvent<unknown>) => {
    const data = event.data;
    if (typeof data !== "object" || data === null || !("type" in data)) {
      fail(new Error("Invalid worker Host envelope"));
      return;
    }
    try {
      if (diagnostics?.receive(data as Record<string, unknown>)) return;
      if (data.type === "ready") return;
      if (data.type === "closed") {
        terminate();
      } else if (
        data.type === "error" &&
        "message" in data &&
        typeof data.message === "string"
      ) {
        fail(new Error(data.message));
      } else fail(new Error("Unexpected worker Host envelope"));
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)));
    }
  };
  control.port1.start();
  visibilityOwner?.addEventListener("visibilitychange", visibilityChanged);
  worker.addEventListener("error", failed);
  worker.addEventListener("messageerror", messageFailed);

  const owner: WorkerHost = {
    openPort() {
      if (failure) throw failure;
      if (stopped || closing) throw new Error("Worker Host is closed");
      // The worker's runtime owns connection capacity and refuses a port
      // beyond it; only identities are bounded here.
      if (nextConnection > 0xffff_ffff_ffff_ffffn)
        throw new Error("Worker connection identities exhausted");
      const connection = nextConnection++;
      const channel = new MessageChannel();
      let disposed = false;
      const endpoint: WorkerEndpoint = {
        connection,
        port: channel.port1,
        dispose() {
          if (disposed) return;
          disposed = true;
          channel.port1.onmessage = channel.port1.onmessageerror = null;
          channel.port1.close();
          records.delete(connection);
          if (!stopped) {
            try {
              control.port1.postMessage({ type: "dispose", connection });
            } catch (error) {
              fail(error instanceof Error ? error : new Error(String(error)));
            }
            if (failure && records.size === 0) terminate();
          }
        },
      };
      records.set(connection, { endpoint });
      try {
        control.port1.postMessage(
          { type: "connect", connection, port: channel.port2 },
          [channel.port2],
        );
      } catch (error) {
        channel.port2.close();
        endpoint.dispose();
        throw error;
      }
      return endpoint;
    },
    connect() {
      const endpoint = owner.openPort();
      const transport = new PortTransport(
        endpoint.port,
        endpoint.connection,
        () => endpoint.dispose(),
        diagnostics,
        timeoutMs,
      );
      records.get(endpoint.connection)!.transport = transport;
      return transport;
    },
    close() {
      if (closing) return closing;
      if (stopped) return Promise.resolve();
      if ([...records.values()].some((record) => !record.transport))
        return Promise.reject(
          new Error("Dispose transferred endpoints before closing their Host"),
        );
      closing = (async () => {
        await Promise.allSettled(
          [...records.values()].map((record) => record.transport!.close()),
        );
        if (stopped) return;
        await new Promise<void>((resolve) => {
          const timer = setTimeout(terminate, timeoutMs);
          finishClose = () => {
            clearTimeout(timer);
            resolve();
          };
          control.port1.postMessage({ type: "shutdown" });
        });
      })();
      return closing;
    },
  };
  try {
    worker.postMessage(
      {
        type: "init",
        resourceUrls,
        wasmUrl: new URL(wasmUrl, globalThis.location.href).href,
        port: control.port2,
        maxMessageBytes,
        hidden: visibilityOwner?.hidden ?? false,
        logLevel: options.logLevel ?? "info",
        ...(options.assetCacheBytes !== undefined
          ? { assetCacheBytes: options.assetCacheBytes }
          : {}),
        ...(options.canvas ? { canvas: options.canvas } : {}),
      },
      [control.port2, ...(options.canvas ? [options.canvas] : [])],
    );
  } catch (error) {
    control.port2.close();
    terminate();
    throw error;
  }
  return owner;
}

/** Convenience owner of one private Host and its initial physical connection. */
export function workerTransport(
  workerUrl: string | URL,
  wasmUrl: string | URL,
  maxMessageBytes: number,
  options: WorkerOptions = {},
): MessageTransport {
  const owner = createWorkerHost(workerUrl, wasmUrl, maxMessageBytes, options);
  const transport = owner.connect();
  return {
    ...(transport.renderDiagnostics
      ? { renderDiagnostics: transport.renderDiagnostics }
      : {}),
    start(events) {
      transport.start({
        ...events,
        error(error) {
          events.error(error);
          void owner.close().catch(() => {});
        },
        closed() {
          events.closed();
          void owner.close().catch(() => {});
        },
      });
    },
    send: (bytes) => transport.send(bytes),
    sendParts: (parts) => transport.sendParts!(parts),
    async close() {
      try {
        await transport.close();
      } finally {
        await owner.close();
      }
    },
  };
}
