import {
  resourceUrlMappings,
  type ResourceUrlMapping,
} from "./resource-urls.js";
import { PortTransport, type MessageTransport } from "./transport.js";
import { validateViewport } from "./presentation.js";
import type { LogLevel } from "./logging.js";
import { validateOptions } from "./client.js";

export interface WorkerOptions {
  /** Also bounds graceful worker shutdown after an in-progress frame. */
  timeoutMs?: number;
  canvas?: OffscreenCanvas;
  logLevel?: LogLevel;
  assetCacheBytes?: number;
  resourceUrls?: readonly ResourceUrlMapping[];
}

const MAX_ASSET_CACHE_BYTES = 0xffff_ffff;

/** Owns one dedicated worker and its fresh world until close or failure. */
export function workerTransport(
  workerUrl: string | URL,
  wasmUrl: string | URL,
  options: WorkerOptions = {},
): MessageTransport {
  validateOptions(options);
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
  const channel = new MessageChannel();
  const visibilityOwner =
    typeof document === "undefined" ? undefined : document;
  const transport = new PortTransport(
    channel.port1,
    () => {
      worker.removeEventListener("error", failed);
      worker.removeEventListener("messageerror", messageFailed);
      visibilityOwner?.removeEventListener(
        "visibilitychange",
        visibilityChanged,
      );
      worker.terminate();
    },
    options.canvas !== undefined,
    options.timeoutMs ?? 10_000,
  );
  const failed = (event: ErrorEvent) =>
    transport.fail(new Error(event.message || "Worker failed"));
  const messageFailed = () => transport.fail(new Error("Worker message error"));
  const visibilityChanged = () => {
    channel.port1.postMessage({
      type: "visibility",
      hidden: visibilityOwner?.hidden ?? false,
    });
  };
  visibilityOwner?.addEventListener("visibilitychange", visibilityChanged);
  worker.addEventListener("error", failed);
  worker.addEventListener("messageerror", messageFailed);
  try {
    worker.postMessage(
      {
        type: "init",
        resourceUrls,
        wasmUrl: new URL(wasmUrl, globalThis.location.href).href,
        port: channel.port2,
        hidden: visibilityOwner?.hidden ?? false,
        logLevel: options.logLevel ?? "info",
        ...(options.assetCacheBytes !== undefined
          ? { assetCacheBytes: options.assetCacheBytes }
          : {}),
        ...(options.canvas ? { canvas: options.canvas } : {}),
      },
      [channel.port2, ...(options.canvas ? [options.canvas] : [])],
    );
    return transport;
  } catch (error) {
    transport.fail(error instanceof Error ? error : new Error(String(error)));
    channel.port2.close();
    throw error;
  }
}
