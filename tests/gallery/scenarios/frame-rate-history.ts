/** Full-history rendering assertions shared by real application drivers. */
import assert from "node:assert/strict";
import { assertCockpitChartPixels } from "./cockpit-quality.js";
import type * as Fixture from "../pages/frame-rate.js";
import type * as Application from "../pages/application.js";

type History = Awaited<ReturnType<typeof Fixture.append>>;
export interface FrameRateHistoryDriver {
  app: {
    call<T>(name: string, ...args: unknown[]): Promise<T>;
    capture(
      name: string,
    ): Promise<Awaited<ReturnType<typeof Application.capture>>>;
    navigate(scene: string): Promise<unknown>;
    state(): Promise<{ worlds: readonly unknown[] }>;
    errors: string[];
  };
  call(name: string, ...args: unknown[]): Promise<History>;
  activity(): Promise<Awaited<ReturnType<typeof Fixture.activity>>>;
  resize(width: number, height: number): Promise<void>;
  turnCamera(): Promise<void>;
  idle(milliseconds: number): Promise<void>;
  record(name: string, value: unknown): Promise<void>;
}

export async function exerciseFrameRateHistory({
  app,
  call,
  activity,
  resize,
  turnCamera,
  idle,
  record,
}: FrameRateHistoryDriver) {
  for (const surface of ["flat", "curved"]) {
    await app.call("options", { surface });
    try {
      const small = await call("open");
      assert.equal(small.rows, 3);
      // Let the carousel finish its Host-clock transition before comparing work.
      await idle(1000);
      const initial = await app.capture(`short-history-${surface}`);
      const full = await call("append", 597);
      assert.equal(full.rows, 600, "all history samples reach the renderer");
      assert.equal(full.availability, "Ready");
      const x = full.columns.indexOf("x");
      assert.ok(x >= 0);
      assert.deepEqual(full.first!.values[x], {
        valid: true,
        value: { kind: "f32", value: 0 },
      });
      const history = await call("append", 10);
      assert.equal(
        history.rows,
        600,
        "live appends keep the full rolling window",
      );
      assert.notDeepEqual(history.last, full.last);
      const frame = await app.capture(`full-history-${surface}`);
      assert.equal(frame.frame.width, 2448);
      assert.equal(frame.frame.height, 1512);
      assert.ok(
        frame.frame.drawCalls < initial.frame.drawCalls + 64,
        `history growth must not add a draw per sample: ${initial.frame.drawCalls} → ${frame.frame.drawCalls}`,
      );
      const pixels =
        await app.call<ReturnType<typeof Application.image>>("image");
      const geometry =
        await app.call<Awaited<ReturnType<typeof Application.cockpitGeometry>>>(
          "cockpitGeometry",
        );
      const ink = assertCockpitChartPixels(pixels, geometry);
      await record("dense-history", {
        surface,
        rows: history.rows,
        frame: frame.frame,
        ink,
      });
      const waitActive = async (active: boolean) => {
        const deadline = performance.now() + 15000;
        for (;;) {
          const state = await activity();
          if (state.active === active) return state;
          assert.ok(
            performance.now() < deadline,
            `Monitor did not become active=${active}`,
          );
          await idle(100);
        }
      };
      await app.call("options", { carousel: 3 });
      await waitActive(false);
      // Drain the one transition before observing unchanged presentation work.
      await idle(500);
      const hidden = await activity();
      const updated = await call("append", 10);
      await idle(700);
      assert.equal(
        updated.rows,
        600,
        "hidden binding retains full rolling history",
      );
      assert.deepEqual(
        (await activity()).layout,
        hidden.layout,
        "offscreen monitor does no text measurement or layout work on data updates",
      );
      await resize(2400, 756);
      await waitActive(true);
      await resize(1224, 756);
      await waitActive(false);
      await turnCamera();
      await waitActive(true);
      await app.call("action", "resetCamera");
      await waitActive(false);
      await app.call("options", { carousel: 1 });
      await waitActive(true);
      await app.capture(`restored-history-${surface}`);
      const restoredPixels =
        await app.call<ReturnType<typeof Application.image>>("image");
      const restoredGeometry =
        await app.call<Awaited<ReturnType<typeof Application.cockpitGeometry>>>(
          "cockpitGeometry",
        );
      assertCockpitChartPixels(restoredPixels, restoredGeometry);
      await record("hidden-history", { surface, hidden, updated });
      assert.deepEqual(app.errors, []);
    } finally {
      await call("close");
    }
  }
  await app.navigate("shapes");
  assert.equal(
    (await app.state()).worlds.length,
    2,
    "cockpit Worlds are released",
  );
}
