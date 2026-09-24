/**
 * Benchmark profiler of `profiling` runtime builds. The worker imports this
 * module only when the runtime exports `ipp_profile_reset`; ordinary builds
 * never load it, and their frame loop carries no profiling branches.
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

type Export = (...args: number[]) => number | bigint;

function requireExport(exports: WebAssembly.Exports, name: string): Export {
  const candidate = exports[name];
  if (typeof candidate !== "function")
    throw new Error(`Profiling runtime is missing ${name}`);
  return candidate as Export;
}

/**
 * Install `globalThis.ippProfile` for the benchmark driver and return the
 * timed frame steps. Table sizes come from the runtime's exports.
 */
export function installProfiler(
  exports: WebAssembly.Exports,
  memory: WebAssembly.Memory,
  frame: ProfiledFrame,
): ProfiledFrame {
  const read = (name: string) => requireExport(exports, name);
  const reset = read("ipp_profile_reset");
  const pause = read("ipp_profile_pause");
  const nameCount = Number(read("ipp_profile_name_count")());
  const counterCount = Number(read("ipp_profile_counter_count")());
  const categoryCount = Number(read("ipp_profile_category_count")());
  const namePointer = read("ipp_profile_name_ptr");
  const nameLength = read("ipp_profile_name_len");
  const counter = read("ipp_profile_counter");
  const categoryNamePointer = read("ipp_profile_category_name_ptr");
  const categoryNameLength = read("ipp_profile_category_name_len");
  const categoryCounter = read("ipp_profile_category_counter");
  const allocations = read("ipp_profile_allocations");
  const shadowDrawCalls = exports.ipp_profile_shadow_draw_calls;
  const decoder = new TextDecoder();
  const text = (pointer: number | bigint, length: number | bigint) =>
    decoder.decode(
      new Uint8Array(memory.buffer, Number(pointer) >>> 0, Number(length)),
    );

  // Evaluation and whole-frame milliseconds, interleaved per captured frame.
  const frames = new Float64Array(MAX_PROFILED_FRAMES * 2);
  let count = 0;
  let capturing = false;
  let evaluationMs = 0;

  Object.assign(globalThis, {
    ippProfile: {
      start(profile = false) {
        reset(Number(profile));
        count = 0;
        capturing = true;
      },
      count: () => count,
      growMemory: () => memory.grow(1),
      stop() {
        capturing = false;
        pause();
        return {
          memoryBytes: memory.buffer.byteLength,
          shadowDrawCalls:
            typeof shadowDrawCalls === "function"
              ? Number((shadowDrawCalls as Export)())
              : null,
          frames: Array.from({ length: count }, (_, i) => [
            frames[i * 2]!,
            frames[i * 2 + 1]!,
          ]),
          names: Array.from({ length: nameCount }, (_, i) =>
            text(namePointer(i), nameLength(i)),
          ),
          stages: Array.from({ length: counterCount }, (_, i) =>
            Number(counter(i)),
          ),
          categories: Array.from({ length: categoryCount }, (_, i) => ({
            name: text(categoryNamePointer(i), categoryNameLength(i)),
            calls: Number(categoryCounter(i * 2)),
            bytes: Number(categoryCounter(i * 2 + 1)),
          })),
          allocations: [0, 1].map((i) => Number(allocations(i))),
        };
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
      }
    },
  };
}
