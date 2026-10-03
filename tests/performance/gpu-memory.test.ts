import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm, chmod } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  accountedMemory,
  parseChromeMemory,
  parseDrmFdinfo,
  sampleDrmMemory,
  sampleChromeMemory,
} from "./gpu-memory.js";
import type { Browser } from "playwright";

const identity = {
  capture: "test-capture",
  pipelineRun: "test-run",
  device: { renderer: "fixture" },
};
const fdinfo =
  "drm-driver: amdgpu\ndrm-pdev: 0000:03:00.0\ndrm-client-id: 42\ndrm-resident-vram: 3 KiB\ndrm-memory-vram: 3 KiB\ndrm-total-vram: 9007199254740993\ndrm-shared-gtt: 2 MiB\n";

test("GPU memory preserves precise bytes and removes deprecated resident alias", () => {
  assert.deepEqual(parseDrmFdinfo(fdinfo)?.metrics, [
    { metric: "drm-resident-vram", value: "3072" },
    { metric: "drm-total-vram", value: "9007199254740993" },
    { metric: "drm-shared-gtt", value: "2097152" },
  ]);
  assert.equal(parseDrmFdinfo("pos: 0\n"), null);
  assert.deepEqual(
    parseDrmFdinfo("drm-driver: test\ndrm-total-memory: 2 KB\n")?.metrics,
    [],
  );
});

test("GPU memory deduplicates descriptors across owned processes and records availability", async () => {
  const root = await mkdtemp(join(tmpdir(), "ipp-fdinfo-"));
  try {
    for (const pid of [11, 12]) {
      await mkdir(join(root, String(pid), "fdinfo"), { recursive: true });
      for (const fd of [5, 6])
        await writeFile(join(root, String(pid), "fdinfo", String(fd)), fdinfo);
    }
    const sample = await sampleDrmMemory(identity, [11, 12], root);
    assert.equal(sample.observations.length, 3);
    assert.equal(sample.raw.length, 4);
    assert.ok(
      sample.observations.every(
        (item) => item.clientId === "42" && item.scope === "drm-client",
      ),
    );
    await mkdir(join(root, "14/fdinfo"), { recursive: true });
    await chmod(join(root, "14/fdinfo"), 0);
    assert.match(
      (await sampleDrmMemory(identity, [14], root)).observations[0]!
        .unavailableReason!,
      /EACCES/,
    );
    await chmod(join(root, "14/fdinfo"), 0o700);
    assert.match(
      (await sampleDrmMemory(identity, [13], root)).observations[0]!
        .unavailableReason!,
      /Cannot read fdinfo/,
    );
    await writeFile(
      join(root, "11/fdinfo/5"),
      "drm-driver: test\ndrm-total-memory: 1\n",
    );
    assert.match(
      (await sampleDrmMemory(identity, [11], root)).observations[0]!
        .unavailableReason!,
      /cannot be safely deduplicated/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("GPU memory keeps Chrome overlapping allocator fields and dump identities separate", () => {
  const events = [
    {
      ph: "v",
      pid: 7,
      id: "0x2",
      args: {
        dumps: {
          allocators: {
            gpu: {
              attrs: { size: { type: "scalar", units: "bytes", value: "100" } },
            },
            "gpu/gl/textures": {
              attrs: { size: { type: "scalar", units: "bytes", value: "80" } },
            },
            malloc: {
              attrs: {
                size: { type: "scalar", units: "bytes", value: "ffff" },
              },
            },
          },
        },
      },
    },
  ];
  assert.deepEqual(
    parseChromeMemory(identity, events, "2").map((item) => [
      item.metric,
      item.value,
    ]),
    [
      ["gpu/size", "256"],
      ["gpu/gl/textures/size", "128"],
    ],
  );
  assert.equal(parseChromeMemory(identity, events, "0x0002").length, 2);
  assert.equal(parseChromeMemory(identity, events, "3").length, 0);
  const counters = accountedMemory(identity, { gui: { guiResidentBytes: 0 } });
  assert.equal(counters[0]!.value, "0");
  assert.equal(counters[1]!.value, null);
  assert.ok(counters[1]!.unavailableReason);
});

test("GPU memory never ends a trace whose start was rejected", async () => {
  const calls: string[] = [];
  const browser = {
    newBrowserCDPSession: async () => ({
      on() {},
      async send(method: string) {
        calls.push(method);
        if (method === "SystemInfo.getProcessInfo") return { processInfo: [] };
        throw new Error("Tracing already started");
      },
      async detach() {
        calls.push("detach");
      },
    }),
  } as unknown as Browser;
  const result = await sampleChromeMemory(identity, browser);
  assert.deepEqual(calls, [
    "SystemInfo.getProcessInfo",
    "Tracing.start",
    "detach",
  ]);
  assert.match(result.observations[0]!.unavailableReason!, /already started/);
});

test("GPU memory ends its owned trace and waits for final chunks even after dump failure", async () => {
  const calls: string[] = [];
  const listeners = new Map<string, (event?: unknown) => void>();
  const browser = {
    newBrowserCDPSession: async () => ({
      on(name: string, callback: (event?: unknown) => void) {
        listeners.set(name, callback);
      },
      async send(method: string) {
        calls.push(method);
        if (method === "SystemInfo.getProcessInfo") return { processInfo: [] };
        if (method === "Tracing.requestMemoryDump")
          throw new Error("Dump failed");
        if (method === "Tracing.end") {
          listeners.get("Tracing.dataCollected")?.({
            value: [{ ph: "v", id: "0x1" }],
          });
          listeners.get("Tracing.tracingComplete")?.();
        }
        return {};
      },
      async detach() {
        calls.push("detach");
      },
    }),
  } as unknown as Browser;
  const result = await sampleChromeMemory(identity, browser);
  assert.deepEqual(calls, [
    "SystemInfo.getProcessInfo",
    "Tracing.start",
    "Tracing.requestMemoryDump",
    "Tracing.end",
    "detach",
  ]);
  assert.equal(result.raw.memoryEvents.length, 1);
  assert.match(result.observations[0]!.unavailableReason!, /Dump failed/);
});
