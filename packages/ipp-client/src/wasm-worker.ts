import {
  WorkerConnections,
  type WorkerConnectionExports,
} from "./worker-connections.js";
import { resourceUrlMappings } from "./resource-urls.js";
import type { RenderWorkerService } from "./render-worker.js";
import type { IngressStatistics } from "./presentation.js";
import type {
  AssetHostExports,
  AssetWorkerService,
} from "./resource-worker.js";
import {
  DiagnosticLogger,
  logLevelValue,
  writeDiagnostic,
  type LogLevel,
} from "./logging.js";

/**
 * Whether this distribution was assembled for an `instrumentation` runtime. The
 * assembler defines it from the distribution's build configuration; only then
 * does the distribution ship the profiler and the worker side of the testing
 * controls, and only then are they loaded.
 */
declare const IPP_INSTRUMENTATION: boolean;

/** Dedicated worker owns ingress, the frame clock, and optional presentation. */
const FRAME_INTERVAL_MS = 1_000 / 60;
const MAX_FRAME_DELTA_SECONDS = 0.25;
const MAX_ASSET_CACHE_BYTES = 0xffff_ffff;

interface WasmHostExports extends WorkerConnectionExports {
  memory: WebAssembly.Memory;
  ipp_schema_hash(): bigint;
  ipp_host_open(epoch: bigint): number;
  ipp_host_set_identity_namespace(namespace: bigint): number;
  ipp_host_set_asset_cache_bytes(bytes: number): number;
  ipp_host_close(): void;
  ipp_task_timer_ready(id: bigint): void;
  ipp_tick(dt: number): number;
  ipp_host_time(seconds: number): number;
  ipp_progress_resources(): number;
  ipp_service_resources(): number;
  ipp_output_ptr(): number;
  ipp_output_len(): number;
  ipp_diagnostics_set_level(level: number): number;
}

function runtimeExports(instance: WebAssembly.Instance): WasmHostExports {
  const exports = instance.exports;
  if (!(exports.memory instanceof WebAssembly.Memory)) {
    throw new Error("WASM runtime has no linear memory");
  }
  for (const name of [
    "ipp_schema_hash",
    "ipp_host_open",
    "ipp_host_set_identity_namespace",
    "ipp_host_set_asset_cache_bytes",
    "ipp_host_close",
    "ipp_task_timer_ready",
    "ipp_input_reserve",
    "ipp_receive",
    "ipp_tick",
    "ipp_host_time",
    "ipp_progress_resources",
    "ipp_service_resources",
    "ipp_accepts_input",
    "ipp_connection_limit",
    "ipp_delivery_limit",
    "ipp_request_window",
    "ipp_connection_open",
    "ipp_connection_close",
    "ipp_connection_dispose",
    "ipp_connection_pending",
    "ipp_connection_failed",
    "ipp_connection_poll",
    "ipp_output_delivery_id",
    "ipp_output_copied",
    "ipp_delivery_complete",
    "ipp_output_ptr",
    "ipp_output_len",
    "ipp_diagnostics_set_level",
  ]) {
    if (typeof exports[name] !== "function") {
      throw new Error(`WASM runtime is missing ${name}`);
    }
  }
  return exports as unknown as WasmHostExports;
}

function freshSession(): bigint {
  const words = crypto.getRandomValues(new BigUint64Array(1));
  return words[0] === 0n ? freshSession() : words[0]!;
}

globalThis.onmessage = (event: MessageEvent<unknown>) => {
  const init = event.data;
  if (
    typeof init !== "object" ||
    init === null ||
    !("type" in init) ||
    init.type !== "init" ||
    !("port" in init) ||
    !(init.port instanceof MessagePort)
  ) {
    globalThis.close();
    return;
  }
  globalThis.onmessage = null;
  const port = init.port;
  let logger = new DiagnosticLogger("worker");
  let runtime: WasmHostExports | undefined;
  let presentation: RenderWorkerService | undefined;
  let resources: AssetWorkerService | undefined;
  // Counted by presenting workers, which report them with renderer statistics.
  let ingress: IngressStatistics | undefined;
  // The paired target contract's message budget arrives with init and is
  // validated before the runtime loads; nothing is accepted until then.
  let maxMessageBytes = 0;
  const assetTimers = new Map<bigint, ReturnType<typeof setTimeout>>();
  let holdAssetTimers = false;
  const heldAssetTimers = new Map<bigint, () => void>();
  let closed = false;
  let initialized = false;
  let paused = "hidden" in init && init.hidden === true;
  let lastFrame = performance.now();
  let nextMaintenanceFrame = lastFrame + FRAME_INTERVAL_MS;
  let frameRequest: number | undefined;
  let frameTimer: ReturnType<typeof setTimeout> | undefined;
  let taskTimer: ReturnType<typeof setTimeout> | undefined;
  let connections: WorkerConnections | undefined;
  const pendingPorts = new Map<bigint, MessagePort>();
  const loading = new AbortController();

  const disposePending = (connection: bigint) => {
    const pending = pendingPorts.get(connection);
    if (!pending) return;
    pendingPorts.delete(connection);
    pending.onmessage = pending.onmessageerror = null;
    pending.close();
  };

  const holdPending = (connection: bigint, pending: MessagePort) => {
    pendingPorts.set(connection, pending);
    const finishPending = (error?: string) => {
      if (pendingPorts.get(connection) !== pending) return;
      try {
        pending.postMessage(
          error
            ? { type: "error", connection, message: error }
            : { type: "closed", connection },
        );
      } catch {
      } finally {
        disposePending(connection);
      }
    };
    pending.onmessageerror = () =>
      finishPending("Worker message decode failed");
    pending.onmessage = (event: MessageEvent<unknown>) => {
      const data = event.data;
      finishPending(
        typeof data === "object" &&
          data !== null &&
          "type" in data &&
          data.type === "close" &&
          "connection" in data &&
          data.connection === connection
          ? undefined
          : "Invalid pending worker connection envelope",
      );
    };
    pending.start();
  };

  const cancelFrame = () => {
    if (frameRequest !== undefined) cancelAnimationFrame(frameRequest);
    clearTimeout(frameTimer);
    frameRequest = undefined;
    frameTimer = undefined;
  };

  const finalize = () => {
    if ((connections?.size ?? 0) !== 0 || pendingPorts.size !== 0) return;
    runtime?.ipp_host_close();
    port.postMessage({ type: "closed" });
    port.close();
    globalThis.close();
  };

  const finish = (error?: Error) => {
    if (!closed) {
      closed = true;
      for (const timer of assetTimers.values()) clearTimeout(timer);
      assetTimers.clear();
      heldAssetTimers.clear();
      cancelFrame();
      clearTimeout(taskTimer);
      taskTimer = undefined;
      loading.abort();
      resources?.close();
      presentation?.close();
      if (error) connections?.failAll(error);
      for (const [connection, pending] of pendingPorts) {
        pending.postMessage({
          type: "error",
          connection,
          message: error?.message ?? "Host closed",
        });
        disposePending(connection);
      }
      pendingPorts.clear();
      logger.log(
        error ? "error" : "info",
        error ? "worker.failed" : "worker.stopped",
        () => ({ reason: error?.message }),
      );
      if (error) port.postMessage({ type: "error", message: error.message });
    }
    finalize();
  };

  const output = (): Uint8Array<ArrayBuffer> => {
    if (!runtime) throw new Error("WASM runtime is not ready");
    // Wasm i32 exports arrive signed; linear-memory addresses are u32.
    const pointer = runtime.ipp_output_ptr() >>> 0;
    const length = runtime.ipp_output_len() >>> 0;
    if (length > maxMessageBytes)
      throw new Error("WASM response exceeds bounds");
    return new Uint8Array(runtime.memory.buffer, pointer, length);
  };

  const checkResult = (ok: number) => {
    if (ok !== 1)
      throw new Error(
        new TextDecoder("utf-8", { fatal: true }).decode(output()),
      );
  };

  const pumpInputs = () => connections?.pumpInputs();
  const publish = () => connections?.publish();

  // Rust wakers only enqueue. A timer gives frames and peer/control messages a
  // turn even when a task continually wakes itself; no synchronous WASM reentry.
  const requestTaskUpdate = () => {
    if (closed || taskTimer !== undefined) return;
    taskTimer = setTimeout(() => {
      taskTimer = undefined;
      if (closed || !runtime) return;
      try {
        checkResult(runtime.ipp_progress_resources());
        resources?.pumpAfterFrame();
        publish();
      } catch (error) {
        finish(error instanceof Error ? error : new Error(String(error)));
      }
    }, 0);
  };

  const evaluateFrame = (dt: number) => checkResult(runtime!.ipp_tick(dt));
  const runFrame = (dt: number) => {
    presentation?.beforeFrame();
    evaluate(dt);
    pumpInputs();
    resources?.pumpAfterFrame();
    publish();
  };
  // Instrumentation builds replace both with timed steps; see profile-worker.ts.
  let evaluate = evaluateFrame;
  let step = runFrame;
  const frame = () => {
    if (frameTimer !== undefined) nextMaintenanceFrame += FRAME_INTERVAL_MS;
    frameRequest = undefined;
    frameTimer = undefined;
    if (closed || !runtime) return;
    try {
      const now = performance.now();
      checkResult(runtime.ipp_host_time(now / 1_000));
      connections?.maintain(now);
      pumpInputs();
      const dt = paused
        ? 0
        : Math.min(
            MAX_FRAME_DELTA_SECONDS,
            Math.max(0, (now - lastFrame) / 1_000),
          );
      lastFrame = now;
      step(dt);
      scheduleFrame();
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  };

  const scheduleFrame = () => {
    if (closed || !initialized) return;
    if (!paused) {
      frameRequest = requestAnimationFrame(frame);
      return;
    }
    // rAF stops in hidden documents. Keep ingress, assets and outcomes live
    // with zero simulation delta, retaining deadlines across work and jitter.
    const now = performance.now();
    if (nextMaintenanceFrame <= now) {
      nextMaintenanceFrame +=
        (Math.floor((now - nextMaintenanceFrame) / FRAME_INTERVAL_MS) + 1) *
        FRAME_INTERVAL_MS;
    }
    frameTimer = setTimeout(frame, nextMaintenanceFrame - now);
  };

  port.onmessageerror = () => finish(new Error("Worker message decode failed"));
  port.onmessage = (message: MessageEvent<unknown>) => {
    const data = message.data;
    try {
      if (typeof data !== "object" || data === null || !("type" in data)) {
        throw new Error("Invalid worker envelope");
      }
      if (
        (data.type === "buffer-source-mount" ||
          data.type === "buffer-source-revoke") &&
        "source" in data &&
        typeof data.source === "string"
      ) {
        try {
          if (!resources) throw new Error("Worker source service is not ready");
          if (data.type === "buffer-source-mount") {
            if (!("publication" in data))
              throw new Error("Missing generated source publication");
            resources.mountBuffer(
              data.source,
              data.publication as Parameters<
                AssetWorkerService["mountBuffer"]
              >[1],
            );
          } else resources.revokeBuffer(data.source);
          port.postMessage({
            type: "buffer-source-result",
            source: data.source,
          });
        } catch (error) {
          port.postMessage({
            type: "buffer-source-result",
            source: data.source,
            error: error instanceof Error ? error.message : String(error),
          });
        }
        return;
      }
      if (data.type === "shutdown") {
        if ((connections?.size ?? 0) !== 0 || pendingPorts.size !== 0)
          throw new Error(
            "Dispose connection endpoints before closing the Host",
          );
        finish();
        return;
      }
      if (
        data.type === "dispose" &&
        "connection" in data &&
        typeof data.connection === "bigint"
      ) {
        disposePending(data.connection);
        connections?.dispose(data.connection);
        if (closed) finalize();
        return;
      }
      if (
        data.type === "connect" &&
        "connection" in data &&
        typeof data.connection === "bigint" &&
        "port" in data &&
        data.port instanceof MessagePort
      ) {
        // Capacity is the runtime's; ports that arrive before it loads wait
        // and are admitted or refused against it once it does.
        if (closed || pendingPorts.has(data.connection)) {
          data.port.postMessage({
            type: "error",
            connection: data.connection,
            message: "Host closed or connection capacity exhausted",
          });
          data.port.close();
        } else if (connections) {
          connections.open(data.connection, data.port);
        } else {
          holdPending(data.connection, data.port);
        }
        return;
      }
      if (
        data.type === "visibility" &&
        "hidden" in data &&
        typeof data.hidden === "boolean"
      ) {
        if (paused === data.hidden) return;
        logger.log("info", data.hidden ? "worker.paused" : "worker.resumed");
        cancelFrame();
        paused = data.hidden;
        // Resume begins a new timing baseline; paused wall time is not simulated.
        lastFrame = performance.now();
        nextMaintenanceFrame = lastFrame + FRAME_INTERVAL_MS;
        scheduleFrame();
        return;
      }
      if (presentation?.receive(data as Record<string, unknown>)) return;
      throw new Error("Unexpected worker Host control");
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  };
  port.start();

  void (async () => {
    try {
      const budget = "maxMessageBytes" in init ? init.maxMessageBytes : 0;
      if (
        typeof budget !== "number" ||
        !Number.isSafeInteger(budget) ||
        budget <= 0
      )
        throw new RangeError(
          "maxMessageBytes must be the target contract's positive message budget",
        );
      maxMessageBytes = budget;
      const resourceUrls = resourceUrlMappings(
        "resourceUrls" in init ? init.resourceUrls : undefined,
      );
      const assetCacheBytes =
        "assetCacheBytes" in init ? init.assetCacheBytes : undefined;
      if (
        assetCacheBytes !== undefined &&
        (typeof assetCacheBytes !== "number" ||
          !Number.isInteger(assetCacheBytes) ||
          assetCacheBytes < 0 ||
          assetCacheBytes > MAX_ASSET_CACHE_BYTES)
      )
        throw new RangeError(
          `assetCacheBytes must be an integer in [0, ${MAX_ASSET_CACHE_BYTES}]`,
        );
      const level = "logLevel" in init ? init.logLevel : "info";
      const threshold = logLevelValue(level);
      logger = new DiagnosticLogger("worker", level as LogLevel);
      logger.log("info", "worker.starting");
      if (!("wasmUrl" in init) || typeof init.wasmUrl !== "string")
        throw new Error("Missing WASM URL");
      if ("canvas" in init) {
        if (!(init.canvas instanceof OffscreenCanvas))
          throw new Error("Expected a transferred OffscreenCanvas");
        const { RenderWorkerService } = await import("./render-worker.js");
        const testing = IPP_INSTRUMENTATION
          ? await import("./render-testing.js")
          : undefined;
        if (closed) return;
        const created = await RenderWorkerService.create(
          init.canvas,
          init.wasmUrl,
          (message, transfer) => port.postMessage(message, transfer),
          finish,
          level as LogLevel,
          testing,
        );
        if (closed) {
          created.close();
          return;
        }
        presentation = created;
      }
      const response = await fetch(init.wasmUrl, { signal: loading.signal });
      if (!response.ok)
        throw new Error(`WASM fetch failed: ${response.status}`);
      const { instance } = await WebAssembly.instantiate(
        await response.arrayBuffer(),
        {
          ...presentation?.imports,
          ipp_tasks: {
            request_update: requestTaskUpdate,
            timer_start(id: bigint, milliseconds: number) {
              if (IPP_INSTRUMENTATION && holdAssetTimers) {
                heldAssetTimers.set(id, () => {
                  runtime?.ipp_task_timer_ready(id);
                });
                return;
              }
              if (closed) return;
              const timer = setTimeout(() => {
                if (assetTimers.get(id) !== timer) return;
                assetTimers.delete(id);
                if (!closed) runtime?.ipp_task_timer_ready(id);
              }, milliseconds >>> 0);
              assetTimers.set(id, timer);
            },
            timer_cancel(id: bigint) {
              heldAssetTimers.delete(id);
              const timer = assetTimers.get(id);
              if (timer !== undefined) clearTimeout(timer);
              assetTimers.delete(id);
            },
          },
          ...(IPP_INSTRUMENTATION
            ? { ipp_profiling: { now: () => performance.now() } }
            : {}),
          ipp_diagnostics: {
            write(severity: number, pointer: number, length: number) {
              if (!runtime || severity === 0 || severity > threshold) return;
              // Borrowed only for this synchronous import; never re-enter Rust.
              const bytes = new Uint8Array(
                runtime.memory.buffer,
                pointer >>> 0,
                length >>> 0,
              );
              writeDiagnostic(severity, new TextDecoder().decode(bytes));
            },
          },
        },
      );
      if (closed) return;
      runtime = runtimeExports(instance);
      if (IPP_INSTRUMENTATION) {
        const { installProfiler, installTaskTesting } = await import(
          "./profile-worker.js"
        );
        if (closed) return;
        const timed = installProfiler(instance.exports, runtime.memory, {
          evaluate: evaluateFrame,
          run: runFrame,
        });
        installTaskTesting(instance.exports, cancelFrame);
        Object.assign(globalThis, {
          ippAssetExportTasks: {
            hold() {
              holdAssetTimers = true;
              (
                instance.exports.ipp_test_asset_export_staging_gate as (
                  enabled: number,
                ) => void
              )(1);
            },
            waiting: () => heldAssetTimers.size,
            suspend() {
              cancelFrame();
            },
            release() {
              (
                instance.exports.ipp_test_asset_export_staging_gate as (
                  enabled: number,
                ) => void
              )(0);
              holdAssetTimers = false;
              const held = [...heldAssetTimers.values()];
              heldAssetTimers.clear();
              for (const ready of held) ready();
            },
            tick: (world: bigint) =>
              (
                instance.exports.ipp_test_asset_export_world_tick as (
                  world: bigint,
                ) => bigint
              )(world),
            resume() {
              scheduleFrame();
            },
          },
        });
        evaluate = timed.evaluate;
        step = timed.run;
      }
      if (presentation)
        ingress = {
          messages: 0,
          wasmCopyBytes: 0,
          partsMessages: 0,
          transferredAssetBytes: 0,
          sourceBytes: 0,
          sourceBufferedBytes: 0,
          sourcePeakBufferedBytes: 0,
        };
      if (runtime.ipp_diagnostics_set_level(threshold) !== 1)
        throw new Error("WASM diagnostic level configuration failed");
      const session = freshSession();
      checkResult(runtime.ipp_host_open(session));
      if (assetCacheBytes !== undefined)
        checkResult(runtime.ipp_host_set_asset_cache_bytes(assetCacheBytes));
      const identity = crypto.getRandomValues(new BigUint64Array(1))[0] || 1n;
      checkResult(runtime.ipp_host_set_identity_namespace(identity));
      if (typeof instance.exports.ipp_resource_poll === "function") {
        const { AssetWorkerService } = await import("./resource-worker.js");
        if (closed) return;
        if (
          typeof instance.exports.ipp_resource_chunk !== "function" ||
          typeof instance.exports.ipp_resource_end !== "function" ||
          typeof instance.exports.ipp_resource_chunk_reserve !== "function" ||
          typeof instance.exports.ipp_resource_chunk_ptr !== "function" ||
          typeof instance.exports.ipp_buffer_source_register !== "function" ||
          typeof instance.exports.ipp_buffer_source_revoke !== "function" ||
          typeof instance.exports.ipp_resource_input_reserve !== "function" ||
          typeof instance.exports.ipp_resource_buffered_bytes !== "function"
        ) {
          throw new Error("WASM resource ingress exports are missing");
        }
        resources = new AssetWorkerService(
          runtime as WasmHostExports & AssetHostExports,
          session,
          {
            logLevel: level as LogLevel,
            resourceUrls,
            failed: finish,
            prepareProgress: () => presentation?.beforeFrame(),
            ...(ingress ? { statistics: ingress } : {}),
          },
        );
      }
      presentation?.initialize(runtime, session, ingress);
      connections = new WorkerConnections(runtime, maxMessageBytes, ingress);
      for (const [connection, endpoint] of pendingPorts) {
        pendingPorts.delete(connection);
        endpoint.onmessage = endpoint.onmessageerror = null;
        connections.open(connection, endpoint);
      }
      pendingPorts.clear();
      lastFrame = performance.now();
      nextMaintenanceFrame = lastFrame + FRAME_INTERVAL_MS;
      initialized = true;
      scheduleFrame();
      logger.log("info", "worker.ready", () => ({ session }));
      port.postMessage({ type: "ready" });
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  })();
};

export {};
