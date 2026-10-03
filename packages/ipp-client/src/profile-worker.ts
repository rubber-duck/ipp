import { parseProfileCapture } from "./profiling.js";

/**
 * Benchmark profiler of `instrumentation` runtime builds. Only instrumentation
 * distributions ship this module and their worker imports it by build
 * configuration; ordinary distributions neither ship nor load it, and their
 * frame loop carries no profiling branches.
 */

/** Frame work the profiler times; it returns wrapped replacements. */
export interface ProfiledFrame {
  /** World evaluation of one frame. */
  evaluate(dt: number): void;
  /** One whole worker frame, including evaluation, I/O and presentation. */
  run(dt: number): void;
}

/** Frames retained per capture; later frames are counted by neither table. */
const MAX_PROFILED_FRAMES = 32_768;

type Export = (...args: (number | bigint)[]) => number | bigint;

function requireExport(exports: WebAssembly.Exports, name: string): Export {
  const candidate = exports[name];
  if (typeof candidate !== "function")
    throw new Error(`Profiling runtime is missing ${name}`);
  return candidate as Export;
}

/** Instrumentation-only trusted worker adapter; no simulation controls. */
export function installProfiler(
  exports: WebAssembly.Exports,
  memory: WebAssembly.Memory,
  frame: ProfiledFrame,
): ProfiledFrame {
  const read = (name: string) => requireExport(exports, name);
  const control = read("ipp_profile_control");
  const capture = read("ipp_profile_response_capture");
  const total = read("ipp_profile_response_total");
  const pointer = read("ipp_profile_response_ptr");
  const length = read("ipp_profile_response_len");
  const shadowDrawCalls = exports.ipp_profile_shadow_draw_calls;
  let active = 0n;
  const request = (
    kind: number,
    offset = 0n,
    counters = false,
    limit = 16_777_216n,
  ) => {
    const status = Number(
      control(kind, active, offset, Number(counters), limit),
    );
    if (status !== 0)
      throw new Error(`Worker profiling control status ${status}`);
  };

  // Evaluation and whole-frame milliseconds, interleaved per captured frame.
  const frames = new Float64Array(MAX_PROFILED_FRAMES * 2);
  let count = 0;
  let droppedFrames = 0n;
  let capturing = false;
  let evaluationMs = 0;

  Object.assign(globalThis, {
    ippProfile: {
      start(profile = false, options: { maxArtifactBytes?: number } = {}) {
        const limit = options.maxArtifactBytes ?? 16_777_216;
        if (!Number.isSafeInteger(limit) || limit <= 0)
          throw new RangeError("Invalid maxArtifactBytes");
        request(1, 0n, profile, BigInt(limit));
        active = BigInt.asUintN(64, BigInt(capture()));
        count = 0;
        droppedFrames = 0n;
        capturing = true;
      },
      count: () => count,
      growMemory: () => memory.grow(1),
      stop() {
        capturing = false;
        request(2);
        const size = BigInt.asUintN(64, BigInt(total()));
        if (size > BigInt(Number.MAX_SAFE_INTEGER))
          throw new Error("Profile size not representable");
        const bytes = new Uint8Array(Number(size));
        let offset = 0n;
        while (offset < size) {
          request(3, offset);
          const len = Number(length());
          if (len === 0 || offset + BigInt(len) > size)
            throw new Error("Invalid profile page");
          bytes.set(
            new Uint8Array(memory.buffer, Number(pointer()), len),
            Number(offset),
          );
          offset += BigInt(len);
        }
        const artifact = parseProfileCapture(
          new TextDecoder("utf-8", { fatal: true }).decode(bytes),
        );
        request(4);
        active = 0n;
        return {
          ...artifact,
          memoryBytes: memory.buffer.byteLength,
          frameSamples: {
            unit: "milliseconds",
            columns: ["evaluation", "worker-frame"],
            capacity: MAX_PROFILED_FRAMES,
            droppedFrames: droppedFrames.toString(),
          },
          shadowDrawCalls:
            typeof shadowDrawCalls === "function"
              ? Number((shadowDrawCalls as Export)())
              : null,
          frames: Array.from({ length: count }, (_, i) => [
            frames[i * 2]!,
            frames[i * 2 + 1]!,
          ]),
        };
      },
      release() {
        capturing = false;
        request(4);
        active = 0n;
      },
    },
  });

  return {
    evaluate(dt) {
      if (!capturing) return frame.evaluate(dt);
      const started = performance.now();
      frame.evaluate(dt);
      evaluationMs = performance.now() - started;
    },
    run(dt) {
      if (!capturing) return frame.run(dt);
      const started = performance.now();
      frame.run(dt);
      if (count < MAX_PROFILED_FRAMES) {
        frames[count * 2] = evaluationMs;
        frames[count * 2 + 1] = performance.now() - started;
        count++;
      } else {
        droppedFrames++;
      }
    },
  };
}
