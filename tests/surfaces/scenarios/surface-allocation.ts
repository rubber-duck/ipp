import assert from "node:assert/strict";
import { counters } from "../../gui/retained/scenarios/retained-gui.js";
import type { CacheRecord, SurfaceCacheDriver } from "./surface-cache.js";

/** Real generated authoring, transport, curved and optional image allocation,
 * camera-only motion and completed pixels. The low budget makes pressure
 * deterministic without depending on a machine's physical GPU memory. */
export async function exerciseSurfaceAllocation(driver: SurfaceCacheDriver) {
  const reports = [];
  for (const curved of [true, false]) {
    const ids = await driver.call<string[]>("allocationPanels", [curved]);
    await driver.call("allocationCamera", [-0.6]);
    await driver.call("surfaceCacheBudget", [180000]);
    const capture = async (label: string, winner: number) => {
      let last: (CacheRecord | undefined)[] = [];
      for (let attempt = 0; attempt < 30; attempt++) {
        const frame = await driver.capture(`${label}-${curved}`, true);
        assert.equal(frame.failedDrawCalls, 0);
        const records = frame.statistics!.surfaces!
          .surfaceCaches as unknown as CacheRecord[];
        const panels = ids.map((id) => records.find((r) => r.entity === id));
        last = panels;
        assert.ok(Number(counters(frame).surfaceCacheResidentBytes) <= 180000);
        if (panels.some((p) => !p)) continue;
        const high = panels[winner]!;
        const low = panels[1 - winner]!;
        if (!(high.width >= 160 && high.width <= 176)) continue;
        if (curved ? low.width >= high.width : low.mode !== "fallback")
          continue;
        const image = driver.pixels(`${label}-${curved}`);
        let red = 0,
          green = 0;
        for (let i = 0; i < image.pixels.length; i += 4) {
          if (image.pixels[i]! > 180 && image.pixels[i + 1]! < 50) red++;
          if (image.pixels[i + 1]! > 180 && image.pixels[i]! < 50) green++;
        }
        assert.ok(
          red > 5000 && green > 5000,
          `${label}: both panels must remain painted`,
        );
        return panels;
      }
      throw new Error(
        `${label}: coverage priority did not settle: ${JSON.stringify(last)}`,
      );
    };
    const left = await capture("allocation-left-wins", 0);
    const warm = await driver.capture(
      `allocation-motion-start-${curved}`,
      true,
    );
    const before = driver.pixels(`allocation-motion-start-${curved}`);
    let moved = warm;
    // Real camera changes move both curved/affine images without changing
    // content or their quality band. Check work as well as completed pixels.
    for (const x of [-0.58, -0.56, -0.54, -0.52]) {
      await driver.call("allocationCamera", [x]);
      moved = await driver.capture(`allocation-motion-${curved}`);
      assert.equal(moved.failedDrawCalls, 0);
      assert.equal(
        counters(moved).totalSurfaceCacheAllocations,
        counters(warm).totalSurfaceCacheAllocations,
        "small camera changes must reuse image storage",
      );
      assert.equal(
        counters(moved).totalSurfaceCacheRepaints,
        counters(warm).totalSurfaceCacheRepaints,
        "small camera changes must reuse context-independent content",
      );
    }
    const after = driver.pixels(`allocation-motion-${curved}`);
    let changedPixels = 0;
    for (let i = 0; i < before.pixels.length; i += 4)
      if (Math.abs(before.pixels[i]! - after.pixels[i]!) > 50) changedPixels++;
    assert.ok(
      changedPixels > 100,
      "completed pixels must follow camera motion",
    );
    await driver.call("allocationCamera", [0.6]);
    const right = await capture("allocation-camera-moved", 1);
    await driver.call("setSurfaceCache", [
      "allocation-left",
      {
        direct_distance: 0,
        resolution_scale: 1,
        max_refresh_hz: 240,
        force_cache: true,
        allocation_priority: 3,
      },
    ]);
    const hinted = await capture("allocation-hint", 0);
    reports.push({ curved, left, right, hinted });
  }
  await driver.call("allocationCamera", [0]);
  await driver.call("surfaceCacheBudget", [128 << 20]);
  await driver.call("clearWorkload", [{ width: 320, height: 240 }]);
  return reports;
}
