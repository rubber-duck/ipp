import assert from "node:assert/strict";
import test from "node:test";
import { installProfiler } from "../src/profile-worker.js";
import type { ProfileCapture } from "../src/profiling.js";

interface WorkerCapture extends ProfileCapture {
  memoryBytes: number;
  frames: number[][];
}
interface ProfileApi {
  start(counters?: boolean): void;
  stop(): WorkerCapture;
  growMemory(): number;
}
type ProfileGlobal = typeof globalThis & { ippProfile?: ProfileApi };

function setup() {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const identities = ["18446744073709551615", "9223372036854775808"];
  let bytes = new Uint8Array();
  let page = new Uint8Array();
  const controls: number[] = [];
  const artifact = (): ProfileCapture => ({
    format: "ipp-profile/1",
    captureId: "7",
    hostId: "1",
    source: {
      target: "wasm",
      schemaHash: "9",
      instrumentation: true,
      scope: "evaluation-thread",
      backgroundAllocations: "excluded",
      adapter: "wasm",
    },
    window: {
      firstBoundary: "1",
      lastBoundary: "2",
      start: "1",
      end: "2",
      clockDomain: "ipp-core.monotonic",
      unit: "nanoseconds",
    },
    availability: {
      cpu: "available",
      allocations: "available",
      gpu: "not-requested",
      trace: "not-requested",
    },
    units: {
      calls: "calls",
      duration: "nanoseconds",
      allocationCalls: "allocation-or-reallocation calls",
      requestedBytes: "requested bytes, not retained memory",
    },
    stages: identities.map((compositionId, index) => ({
      kind: "system",
      name: "ipp.system.alpha",
      identity: {
        scope: "world",
        hostId: "1",
        worldId: String(index + 1),
        incarnation: String(index + 10),
        compositionId,
        system: "ipp.system.alpha",
        phase: "evaluate",
      },
      calls: "1",
      duration: "20",
      allocationCalls: "0",
      requestedBytes: "0",
    })),
    categories: [
      {
        name: "allocator🧪",
        identity: {
          scope: "unassigned",
          hostId: "0",
          worldId: "0",
          incarnation: "0",
          compositionId: "0",
          system: "",
          phase: null,
        },
        allocationCalls: "2",
        requestedBytes: "5",
      },
    ],
    allocations: { calls: "2", requestedBytes: "5" },
    retention: { artifactLimitBytes: "16777216" },
    instrumentationStorage: { counterBytes: "128", metadataBytes: "64" },
  });
  const exports = {
    ipp_profile_control(kind: number, _capture: bigint, offset: bigint) {
      controls.push(kind);
      if (kind === 2)
        bytes = new TextEncoder().encode(JSON.stringify(artifact()));
      if (kind === 3) {
        // Each page can grow the heap. The reader must refresh its buffer.
        memory.grow(1);
        page = bytes.slice(Number(offset), Number(offset) + 256);
        new Uint8Array(memory.buffer, 128, page.length).set(page);
      }
      return 0;
    },
    ipp_profile_response_capture: () => 7n,
    ipp_profile_response_total: () => BigInt(bytes.length),
    ipp_profile_response_ptr: () => 128,
    ipp_profile_response_len: () => page.length,
  } as unknown as WebAssembly.Exports;
  return { memory, identities, exports, controls };
}

test("worker copies semantic exact identities and Unicode across heap growth before release", (context) => {
  const previous = (globalThis as ProfileGlobal).ippProfile;
  context.after(() => {
    if (previous) (globalThis as ProfileGlobal).ippProfile = previous;
    else delete (globalThis as ProfileGlobal).ippProfile;
  });
  const { memory, identities, exports, controls } = setup();
  const frame = installProfiler(exports, memory, { evaluate() {}, run() {} });
  const profile = (globalThis as ProfileGlobal).ippProfile!;
  profile.start(true);
  frame.evaluate(0.1);
  frame.run(0.1);
  identities.push("8");
  profile.growMemory();
  const captured = profile.stop();
  assert.deepEqual(
    captured.stages.map((record) => record.identity.compositionId),
    identities,
  );
  assert.equal(captured.categories[0]!.name, "allocator🧪");
  assert.equal(captured.memoryBytes, memory.buffer.byteLength);
  assert.equal(captured.frames.length, 1);
  assert.equal(controls.at(-1), 4);
  assert.ok(controls.filter((kind) => kind === 3).length > 1);
});

test("worker requires owner-aware diagnostic controls", () => {
  const { memory, exports } = setup();
  const missing = Object.fromEntries(
    Object.entries(exports).filter(([name]) => name !== "ipp_profile_control"),
  ) as WebAssembly.Exports;
  assert.throws(
    () => installProfiler(missing, memory, { evaluate() {}, run() {} }),
    /ipp_profile_control/,
  );
});
