import assert from "node:assert/strict";
import test from "node:test";
import {
  PortRenderDiagnostics,
  type RenderStatisticsSnapshot,
} from "../src/presentation.js";
import { presentationTesting } from "../src/testing.js";

test("render diagnostics cannot authorize frame, capture or implicit resize", async () => {
  const sent: unknown[] = [];
  const diagnostics = new PortRenderDiagnostics((message) =>
    sent.push(message),
  );
  for (const method of ["frame", "capture", "resize"])
    assert.equal(method in diagnostics, false);
  const pending = diagnostics.statistics();
  assert.deepEqual(sent, [{ type: "render-statistics", id: 1 }]);
  const snapshot: RenderStatisticsSnapshot = {
    readbackMs: 2,
    frame: { uploadedBytes: 1, totalUploadedBytes: 3, unshadowedLights: 0 },
    device: { api: "test" },
  };
  assert.equal(
    diagnostics.receive({
      type: "render-statistics",
      id: 1,
      statistics: snapshot,
    }),
    true,
  );
  assert.equal(await pending, snapshot);
  presentationTesting(diagnostics).loseContext();
  assert.deepEqual(sent[1], { type: "context-loss" });
  diagnostics.close(new Error("closed"));
  assert.throws(
    () => presentationTesting(diagnostics).restoreContext(),
    /closed/,
  );
  await assert.rejects(diagnostics.statistics(), /closed/);
});

test("diagnostic failures reject all known waits without frame fallback", async () => {
  const diagnostics = new PortRenderDiagnostics(() => {});
  const pending = Array.from({ length: 4 }, () => diagnostics.statistics());
  await assert.rejects(diagnostics.statistics(), /queue full/);
  assert.throws(
    () =>
      diagnostics.receive({ type: "render-statistics", id: 5, statistics: {} }),
    /Invalid/,
  );
  const checks = pending.map((promise) =>
    assert.rejects(promise, /invalid diagnostics/),
  );
  diagnostics.close(new Error("invalid diagnostics"));
  await Promise.all(checks);
});
