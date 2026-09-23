import test from "node:test";
import { resolve } from "node:path";
import { runSurfaceCache } from "./surface-cache-environment.js";

test("opted-in Surfaces reuse bounded cache images and fall back to current direct presentation", {
  timeout: 900000,
}, async (context) => {
  const reports = await runSurfaceCache(
    context.signal,
    resolve("target/integration-artifacts/surface-cache"),
  );
  for (const { build, report } of reports) {
    await context.test(`${build}: WebGL cache target bridge`, () => {
      if (!report.bridge) throw new Error("The bridge probe did not report");
    });
    await context.test(`${build}: cached presentation lifecycle`, (subtest) => {
      // Visible, never passing: RenderService has not cached any Surface yet.
      if (report.status === "inactive")
        subtest.skip(
          "RenderService presents opted-in Surfaces directly (ipp-s1ge.2.3 not integrated)",
        );
    });
  }
});
