import assert from "node:assert/strict";
import test from "node:test";
import { installProfiler } from "../src/profile-worker.js";

interface ProfileCapture {
  memoryBytes: number;
  shadowDrawCalls: number | null;
  frames: number[][];
  names: string[];
  compositionIds: string[];
  stages: number[];
  categories: { name: string; calls: number; bytes: number }[];
  allocations: number[];
}

interface ProfileApi {
  start(profile?: boolean): void;
  stop(): ProfileCapture;
  count(): number;
  growMemory(): number;
}

type ProfileGlobal = typeof globalThis & { ippProfile?: ProfileApi };

interface ProfilerOptions {
  names?: string[];
  compositionIds?: bigint[];
}

function setupProfiler(options: ProfilerOptions = {}) {
  const names = options.names ?? ["ipp.system.alpha", "ipp.system.alpha"];
  const compositionIds = options.compositionIds ?? [101n, 202n];
  const memory = new WebAssembly.Memory({ initial: 1 });
  const encoder = new TextEncoder();
  const namePointer = (index: number) => 128 + index * 64;
  const categoryPointer = 2048;

  names.forEach((name, index) => {
    new Uint8Array(memory.buffer, namePointer(index)).set(encoder.encode(name));
  });
  new Uint8Array(memory.buffer, categoryPointer).set(
    encoder.encode("allocator🧪"),
  );

  const exports = {
    ipp_profile_reset(_enabled: number) {},
    ipp_profile_pause() {},
    ipp_profile_name_count: () => names.length,
    ipp_profile_counter_count: () => names.length * 4,
    ipp_profile_category_count: () => 1,
    ipp_profile_name_ptr: namePointer,
    ipp_profile_name_len: (index: number) =>
      encoder.encode(names[index]!).length,
    ipp_profile_composition_id: (index: number) => compositionIds[index]!,
    ipp_profile_counter: (index: number) => index,
    ipp_profile_category_name_ptr: () => categoryPointer,
    ipp_profile_category_name_len: () => encoder.encode("allocator🧪").length,
    ipp_profile_category_counter: (index: number) => index + 10,
    ipp_profile_allocations: (bytes: number) => bytes + 20,
    ipp_profile_shadow_draw_calls: () => 37,
  } as unknown as WebAssembly.Exports;
  const frame = { evaluate(_dt: number) {}, run(_dt: number) {} };
  const timedFrame = installProfiler(exports, memory, frame);

  return { memory, timedFrame, exports, names, compositionIds };
}

function profileApi(): ProfileApi {
  return (globalThis as ProfileGlobal).ippProfile!;
}

function restoreProfiler(previous: ProfileApi | undefined) {
  const target = globalThis as ProfileGlobal;
  if (previous === undefined) delete target.ippProfile;
  else target.ippProfile = previous;
}

test("profiler keeps repeated system names distinct by exact composition id", (context) => {
  const previous = (globalThis as ProfileGlobal).ippProfile;
  context.after(() => restoreProfiler(previous));

  const { memory, timedFrame } = setupProfiler({
    names: ["ipp.system.alpha", "ipp.system.alpha"],
    compositionIds: [
      BigInt.asIntN(64, (1n << 64n) - 1n),
      BigInt.asIntN(64, 1n << 63n),
    ],
  });
  const profile = profileApi();
  profile.start(true);
  timedFrame.evaluate(1 / 60);
  timedFrame.run(1 / 60);
  assert.equal(profile.growMemory(), 1);

  const capture = profile.stop();

  assert.deepEqual(capture.names, ["ipp.system.alpha", "ipp.system.alpha"]);
  assert.deepEqual(capture.compositionIds, [
    "18446744073709551615",
    "9223372036854775808",
  ]);
  assert.equal(capture.categories[0]?.name, "allocator🧪");
  assert.equal(capture.memoryBytes, memory.buffer.byteLength);
  assert.equal(capture.frames.length, 1);
  assert.equal(capture.frames[0]?.length, 2);
  assert.equal(capture.stages.length, 8);
  assert.deepEqual(capture.allocations, [20, 21]);
});

test("profiler reads System tables that grew after installation", (context) => {
  const previous = (globalThis as ProfileGlobal).ippProfile;
  context.after(() => restoreProfiler(previous));

  const { timedFrame, names, compositionIds } = setupProfiler({
    names: ["ipp.system.alpha"],
    compositionIds: [7n],
  });
  const profile = profileApi();
  profile.start(true);
  timedFrame.run(1 / 60);
  // A World constructed during the capture registers another System.
  names.push("ipp.system.alpha");
  compositionIds.push(8n);

  const capture = profile.stop();
  assert.deepEqual(capture.compositionIds, ["7", "8"]);
  assert.equal(capture.stages.length, 8);
});

test("profiler requires composition exports", (context) => {
  const previous = (globalThis as ProfileGlobal).ippProfile;
  context.after(() => restoreProfiler(previous));

  const { memory, exports } = setupProfiler();
  const withoutExport = (name: string) =>
    Object.fromEntries(
      Object.entries(exports).filter(([exportName]) => exportName !== name),
    ) as WebAssembly.Exports;

  assert.throws(
    () =>
      installProfiler(withoutExport("ipp_profile_composition_id"), memory, {
        evaluate() {},
        run() {},
      }),
    /ipp_profile_composition_id/,
  );
});
