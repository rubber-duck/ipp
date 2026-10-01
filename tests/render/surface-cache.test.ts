import test from "node:test";
import { resolve } from "node:path";
import {
  runSurfaceCache,
  SURFACE_CACHE_BUILDS,
} from "./surface-cache-environment.js";

for (const build of SURFACE_CACHE_BUILDS)
  test(`${build.name}: opted-in Surfaces reuse bounded cache images and fall back to current direct presentation`, {
    timeout: 900000,
  }, async (context) => {
    const reports = await runSurfaceCache(
      context.signal,
      resolve("target/integration-artifacts/surface-cache", build.name),
      [build],
    );
    for (const { build, report, density } of reports) {
      await context.test(`${build}: WebGL cache target bridge`, () => {
        if (!report.bridge) throw new Error("The bridge probe did not report");
      });
      await context.test(`${build}: cached presentation lifecycle`, () => {
        if (report.status !== "active")
          throw new Error("The cache lifecycle did not run");
      });
      await context.test(
        `${build}: cached versus direct text at DPR 1 and 2`,
        () => {
          const ratios = density.map(
            ({ devicePixelRatio }) => devicePixelRatio,
          );
          if (ratios.join() !== "1,2")
            throw new Error(`Density was measured at DPR ${ratios.join(", ")}`);
        },
      );
    }
  });
