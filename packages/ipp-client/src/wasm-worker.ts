import { resourceUrlMappings } from "./resource-urls.js";
import type { RenderWorkerService } from "./render-worker.js";
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

/** Dedicated worker owns ingress, the frame clock, and optional presentation. */
const MAX_MESSAGE_BYTES = 1_048_576;
const MAX_IN_FLIGHT_MESSAGES = 64;
const FRAME_INTERVAL_MS = 1_000 / 60;
const MAX_FRAME_DELTA_SECONDS = 0.25;
const MAX_ASSET_CACHE_BYTES = 0xffff_ffff;

interface WasmHostExports {
  memory: WebAssembly.Memory;
  ipp_schema_hash(): bigint;
  ipp_session_open(session: bigint): number;
  ipp_host_set_identity_namespace(namespace: bigint): number;
  ipp_host_set_asset_cache_bytes(bytes: number): number;
  ipp_session_close(): void;
  ipp_input_reserve(length: number): number;
  ipp_receive(length: number): number;
  ipp_tick(dt: number): number;
  ipp_host_time(seconds: number): number;
  ipp_progress_resources(): number;
  ipp_service_resources(): number;
  ipp_accepts_input(): number;
  ipp_poll(): number;
  ipp_output_ptr(): number;
  ipp_output_len(): number;
}

function runtimeExports(instance: WebAssembly.Instance): WasmHostExports {
  const exports = instance.exports;
  if (!(exports.memory instanceof WebAssembly.Memory)) {
    throw new Error("WASM runtime has no linear memory");
  }
  for (const name of [
    "ipp_schema_hash",
    "ipp_session_open",
    "ipp_host_set_identity_namespace",
    "ipp_host_set_asset_cache_bytes",
    "ipp_session_close",
    "ipp_input_reserve",
    "ipp_receive",
    "ipp_tick",
    "ipp_host_time",
    "ipp_progress_resources",
    "ipp_service_resources",
    "ipp_accepts_input",
    "ipp_poll",
    "ipp_output_ptr",
    "ipp_output_len",
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
  const ingress = {
    messages: 0,
    wasmCopyBytes: 0,
    partsMessages: 0,
    transferredAssetBytes: 0,
  };
  let closed = false;
  let initialized = false;
  let paused = "hidden" in init && init.hidden === true;
  let lastFrame = performance.now();
  let nextMaintenanceFrame = lastFrame + FRAME_INTERVAL_MS;
  let frameRequest: number | undefined;
  let frameTimer: ReturnType<typeof setTimeout> | undefined;
  let inFlight = 0;
  const loading = new AbortController();

  const cancelFrame = () => {
    if (frameRequest !== undefined) cancelAnimationFrame(frameRequest);
    clearTimeout(frameTimer);
    frameRequest = undefined;
    frameTimer = undefined;
  };

  const finish = (error?: Error) => {
    if (closed) return;
    closed = true;
    cancelFrame();
    loading.abort();
    resources?.close();
    presentation?.close();
    runtime?.ipp_session_close();
    logger.log(
      error ? "error" : "info",
      error ? "worker.failed" : "worker.stopped",
      () => ({ reason: error?.message }),
    );
    port.postMessage(
      error ? { type: "error", message: error.message } : { type: "closed" },
    );
    port.close();
    globalThis.close();
  };

  const output = (): Uint8Array<ArrayBuffer> => {
    if (!runtime) throw new Error("WASM runtime is not ready");
    // Wasm i32 exports arrive signed; linear-memory addresses are u32.
    const pointer = runtime.ipp_output_ptr() >>> 0;
    const length = runtime.ipp_output_len() >>> 0;
    if (length > MAX_MESSAGE_BYTES)
      throw new Error("WASM response exceeds bounds");
    return new Uint8Array(runtime.memory.buffer, pointer, length).slice();
  };

  const checkResult = (ok: number) => {
    if (ok !== 1)
      throw new Error(
        new TextDecoder("utf-8", { fatal: true }).decode(output()),
      );
  };

  let lastDelivery = performance.now();
  const pendingInputs: { parts: ArrayBuffer[]; multipart: boolean }[] = [];
  const pumpInputs = () => {
    while (pendingInputs.length && runtime?.ipp_accepts_input() === 1) {
      const { parts, multipart } = pendingInputs.shift()!;
      const length = parts.reduce((sum, part) => sum + part.byteLength, 0);
      const pointer = runtime.ipp_input_reserve(length) >>> 0;
      if (pointer === 0) throw new Error("WASM input reservation failed");
      const destination = new Uint8Array(
        runtime.memory.buffer,
        pointer,
        length,
      );
      let offset = 0;
      for (const part of parts) {
        destination.set(new Uint8Array(part), offset);
        offset += part.byteLength;
      }
      ingress.messages++;
      ingress.wasmCopyBytes += length;
      if (multipart) {
        ingress.partsMessages++;
        ingress.transferredAssetBytes += parts[1]?.byteLength ?? 0;
      }
      checkResult(runtime.ipp_receive(length));
    }
  };

  const publish = () => {
    // Delivery acknowledgements bound the browser queue as well as Rust's outbox.
    // A stalled receiver leaves output in Rust, where saturation fails explicitly.
    // Each poll invalidates the previous WASM view; transfer an exclusive copy.
    while (inFlight < MAX_IN_FLIGHT_MESSAGES && runtime?.ipp_poll() === 1) {
      const bytes = output();
      port.postMessage({ type: "data", bytes: bytes.buffer }, [bytes.buffer]);
      if (inFlight === 0) lastDelivery = performance.now();
      inFlight++;
    }
  };

  // Host-only profiler: the hook exists only in profiling WASM builds.
  let profileFrames = new Float64Array(0);
  let profileFrameCount = 0;
  let profileCapture = false;
  let profileApi: WebAssembly.Exports | undefined;
  const frame = () => {
    if (frameTimer !== undefined) nextMaintenanceFrame += FRAME_INTERVAL_MS;
    frameRequest = undefined;
    frameTimer = undefined;
    if (closed || !runtime) return;
    try {
      const now = performance.now();
      checkResult(runtime.ipp_host_time(now / 1_000));
      if (inFlight > 0 && now - lastDelivery >= 30_000)
        throw new Error(
          "connection congestion: no delivery progress for 30 seconds",
        );
      pumpInputs();
      const dt = paused
        ? 0
        : Math.min(
            MAX_FRAME_DELTA_SECONDS,
            Math.max(0, (now - lastFrame) / 1_000),
          );
      lastFrame = now;
      const started = profileCapture ? performance.now() : 0;
      presentation?.beforeFrame();
      const beforeTick = profileCapture ? performance.now() : 0;
      checkResult(runtime.ipp_tick(dt));
      const afterTick = profileCapture ? performance.now() : 0;
      pumpInputs();
      resources?.pumpAfterFrame();
      presentation?.afterFrame();
      publish();
      if (profileCapture && profileFrameCount * 2 < profileFrames.length) {
        profileFrames[profileFrameCount * 2] = afterTick - beforeTick;
        profileFrames[profileFrameCount * 2 + 1] = performance.now() - started;
        profileFrameCount++;
      }
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
      if (data.type === "close") {
        finish();
        return;
      }
      if (data.type === "ack") {
        if (inFlight === 0)
          throw new Error("Unexpected delivery acknowledgement");
        inFlight--;
        lastDelivery = performance.now();
        pumpInputs();
        publish();
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
      if (!runtime || (data.type !== "data" && data.type !== "data-parts")) {
        throw new Error("Expected binary data after worker readiness");
      }
      const parts: ArrayBuffer[] =
        data.type === "data" &&
        "bytes" in data &&
        data.bytes instanceof ArrayBuffer
          ? [data.bytes]
          : data.type === "data-parts" &&
              "parts" in data &&
              Array.isArray(data.parts) &&
              data.parts.length > 0 &&
              data.parts.length <= 2 &&
              data.parts.every((part: unknown) => part instanceof ArrayBuffer)
            ? data.parts
            : [];
      const length = parts.reduce((total, part) => total + part.byteLength, 0);
      if (length === 0 || length > MAX_MESSAGE_BYTES) {
        throw new Error("Message exceeds WASM ingress bounds");
      }
      if (pendingInputs.length >= 128)
        throw new Error(
          "connection congestion: worker ingress capacity exhausted",
        );
      pendingInputs.push({ parts, multipart: data.type === "data-parts" });
      pumpInputs();
      publish();
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  };
  port.start();

  void (async () => {
    try {
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
        if (closed) return;
        const created = await RenderWorkerService.create(
          init.canvas,
          init.wasmUrl,
          (message, transfer) => port.postMessage(message, transfer),
          finish,
          ingress,
          level as LogLevel,
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
          ipp_profiling: { now: () => performance.now() },
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
      if (typeof instance.exports.ipp_profile_reset === "function") {
        profileApi = instance.exports;
        profileFrames = new Float64Array(65536);
        Object.assign(globalThis, {
          ippProfile: {
            start(profile = false) {
              (profileApi!.ipp_profile_reset as Function)(Number(profile));
              profileFrameCount = 0;
              profileCapture = true;
            },
            count: () => profileFrameCount,
            growMemory: () => runtime!.memory.grow(1),
            stop() {
              profileCapture = false;
              (profileApi!.ipp_profile_pause as Function)();
              return {
                memoryBytes: runtime!.memory.buffer.byteLength,
                shadowDrawCalls:
                  typeof profileApi!.ipp_profile_shadow_draw_calls ===
                  "function"
                    ? Number(
                        (
                          profileApi!.ipp_profile_shadow_draw_calls as Function
                        )(),
                      )
                    : null,
                frames: Array.from({ length: profileFrameCount }, (_, i) => [
                  profileFrames[i * 2]!,
                  profileFrames[i * 2 + 1]!,
                ]),
                names: Array.from({ length: 32 }, (_, i) =>
                  new TextDecoder().decode(
                    new Uint8Array(
                      runtime!.memory.buffer,
                      (profileApi!.ipp_profile_name_ptr as Function)(i) >>> 0,
                      (profileApi!.ipp_profile_name_len as Function)(i),
                    ),
                  ),
                ),
                stages: Array.from({ length: 768 }, (_, i) =>
                  Number((profileApi!.ipp_profile_counter as Function)(i)),
                ),
                categories: Array.from({ length: 256 }, (_, i) => ({
                  name: new TextDecoder().decode(
                    new Uint8Array(
                      runtime!.memory.buffer,
                      (profileApi!.ipp_profile_category_name_ptr as Function)(
                        i,
                      ) >>> 0,
                      (profileApi!.ipp_profile_category_name_len as Function)(
                        i,
                      ),
                    ),
                  ),
                  calls: Number(
                    (profileApi!.ipp_profile_category_counter as Function)(
                      i * 2,
                    ),
                  ),
                  bytes: Number(
                    (profileApi!.ipp_profile_category_counter as Function)(
                      i * 2 + 1,
                    ),
                  ),
                })),
                allocations: [0, 1].map((i) =>
                  Number((profileApi!.ipp_profile_allocations as Function)(i)),
                ),
              };
            },
          },
        });
      }
      const configure = instance.exports.ipp_diagnostics_set_level;
      if (typeof configure === "function" && configure(threshold) !== 1) {
        throw new Error("WASM diagnostic level configuration failed");
      }
      const session = freshSession();
      checkResult(runtime.ipp_session_open(session));
      if (assetCacheBytes !== undefined)
        checkResult(runtime.ipp_host_set_asset_cache_bytes(assetCacheBytes));
      const identity = crypto.getRandomValues(new BigUint64Array(1))[0] || 1n;
      checkResult(runtime.ipp_host_set_identity_namespace(identity));
      if (typeof instance.exports.ipp_resource_poll === "function") {
        const { AssetWorkerService } = await import("./resource-worker.js");
        if (closed) return;
        if (
          typeof instance.exports.ipp_resource_buffered_bytes !== "function" ||
          typeof instance.exports.ipp_resource_chunk !== "function" ||
          typeof instance.exports.ipp_resource_end !== "function" ||
          typeof instance.exports.ipp_resource_input_reserve !== "function"
        ) {
          throw new Error("WASM resource ingress exports are missing");
        }
        resources = new AssetWorkerService(
          runtime as WasmHostExports & AssetHostExports,
          session,
          level as LogLevel,
          ingress,
          resourceUrls,
          finish,
          () => presentation?.beforeFrame(),
        );
      }
      presentation?.initialize(runtime, session);
      lastFrame = performance.now();
      nextMaintenanceFrame = lastFrame + FRAME_INTERVAL_MS;
      initialized = true;
      scheduleFrame();
      logger.log("info", "worker.ready", () => ({
        session,
        diagnostics: typeof configure === "function",
      }));
      port.postMessage({ type: "ready" });
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  })();
};

export {};
