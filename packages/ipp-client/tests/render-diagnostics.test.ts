import assert from "node:assert/strict";
import test from "node:test";
import {
  PortRenderDiagnostics,
  bindPresentation,
  type RenderStatisticsSnapshot,
} from "../src/presentation.js";
import { renderDiagnostics } from "../src/diagnostics.js";
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
  // Routing returns the received snapshot itself; its groups are not read.
  const snapshot = {
    readbackMs: 2,
    frame: { uploadedBytes: 1, totalUploadedBytes: 3, unshadowedLights: 0 },
    device: { api: "test" },
  } as RenderStatisticsSnapshot;
  assert.equal(
    diagnostics.receive({
      type: "render-statistics",
      id: 1,
      statistics: snapshot,
    }),
    true,
  );
  assert.equal(await pending, snapshot);
  assert.equal(
    diagnostics.receive({
      type: "presentation-configuration",
      instrumentation: true,
    }),
    true,
  );
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

test("testing controls fail at the call against a build without instrumentation", () => {
  const sent: unknown[] = [];
  const diagnostics = new PortRenderDiagnostics((message) =>
    sent.push(message),
  );
  const host = {};
  bindPresentation(host, diagnostics);
  assert.equal(renderDiagnostics(host), diagnostics);
  assert.equal(renderDiagnostics({}), undefined);
  assert.throws(
    () => presentationTesting(host).loseContext(),
    /until the presentation reports its build configuration/,
  );
  diagnostics.receive({
    type: "presentation-configuration",
    instrumentation: false,
  });
  for (const control of [
    (testing: ReturnType<typeof presentationTesting>) => testing.loseContext(),
    (testing: ReturnType<typeof presentationTesting>) =>
      testing.setSurfaceCacheBudget(0),
    (testing: ReturnType<typeof presentationTesting>) =>
      testing.setGlyphAtlasLimits({ maxPages: 1, idlePageFrames: 0 }),
    (testing: ReturnType<typeof presentationTesting>) =>
      testing.setExhaustiveDrawChecks(true),
  ])
    assert.throws(
      () => control(presentationTesting(host)),
      /require an instrumentation build/,
    );
  assert.deepEqual(sent, []);
  assert.throws(
    () => presentationTesting({}),
    /Expected an IPP worker presentation/,
  );
});
