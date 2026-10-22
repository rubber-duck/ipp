import assert from "node:assert/strict";
import type { RenderStatisticsSnapshot } from "@ipp/client/diagnostics";
import type { CockpitDriver, CockpitState } from "./cockpit.js";
import {
  cameraCoordinates,
  cockpitFields,
  surfaceSample,
  rotate,
} from "../cockpit-oracle.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

function clearance(state: CockpitState) {
  const camera = cockpitFields(
    state.scene,
    "cockpit-camera",
    state.componentIds.Transform!,
  );
  for (let panel = 0; panel < 3; panel++) {
    for (const u of [0, 0.25, 0.5, 0.75, 1]) {
      const point = cameraCoordinates(
        surfaceSample(state.scene, state.componentIds, panel, u, 1).position,
        camera,
      );
      // The exported console's upper strip reaches radius 8.60 at y=-1.35.
      // Flat tangents and curved sheets must both stay outside that envelope.
      assert.ok(
        Math.hypot(point[0]!, point[2]!) >= 8.799,
        "panel bottom clears the front dashboard",
      );
      assert.ok(
        point[1]! >= -1.351,
        "panel bottom stays above the lower console",
      );
    }
  }
}

export function assertCockpitChartPixels(
  frame: RgbaImage,
  state: Pick<CockpitState, "panels" | "scene" | "componentIds">,
) {
  const panel = state.panels[1]!;
  const rect = cockpitFields(
    panel,
    "cockpit-fps-chart",
    state.componentIds.CanvasBounds!,
  );
  const camera = cockpitFields(
    state.scene,
    "cockpit-camera",
    state.componentIds.Transform!,
  );
  const projection = cockpitFields(
    state.scene,
    "cockpit-camera",
    state.componentIds.Camera!,
  );
  const points = [0, 1].flatMap((u) =>
    [0, 1].map((v) => {
      const world = surfaceSample(
        state.scene,
        state.componentIds,
        1,
        (Number(rect.x) + Number(rect.width) * u) / 960,
        (Number(rect.y) + Number(rect.height) * v) / 800,
      ).position;
      const p = rotate(
        world.map((x, i) => x - Number(camera[["x", "y", "z"][i]!])),
        camera,
        true,
      );
      const halfHeight = -p[2]! * Math.tan(Number(projection.fov_y) / 2);
      return [
        frame.width / 2 + (p[0]! * frame.height) / (2 * halfHeight),
        frame.height / 2 - (p[1]! * frame.height) / (2 * halfHeight),
      ];
    }),
  );
  const left = Math.max(0, Math.ceil(Math.min(...points.map((p) => p[0]!))));
  const right = Math.min(
    frame.width,
    Math.floor(Math.max(...points.map((p) => p[0]!))),
  );
  const top = Math.max(0, Math.ceil(Math.min(...points.map((p) => p[1]!))));
  const bottom = Math.min(
    frame.height,
    Math.floor(Math.max(...points.map((p) => p[1]!))),
  );
  let labels = 0,
    trace = 0,
    backdrop = 0;
  for (let y = top; y < bottom; y++)
    for (let x = left; x < right; x++) {
      const i = (y * frame.width + x) * 4;
      const r = frame.pixels[i]!;
      const g = frame.pixels[i + 1]!;
      const b = frame.pixels[i + 2]!;
      if (Math.min(r, g, b) > 160) labels++;
      if (b > r + 20 && g > r + 10 && g > 80) trace++;
      if (Math.max(r, g, b) < 75) backdrop++;
    }
  assert.ok(
    labels > 30 && trace > 30 && backdrop > 1000,
    `live chart has readable labels, a trace and dark backdrop: ${labels}/${trace}/${backdrop}`,
  );
  return { labels, trace, backdrop };
}

export async function exerciseCockpitQuality(
  driver: CockpitDriver & { statistics(): Promise<RenderStatisticsSnapshot> },
) {
  const initial = await driver.state();
  assert.equal(initial.options.surfaceCaching, "off");
  const panelWorlds = initial.panelWorlds;
  for (const surface of ["flat", "curved"]) {
    await driver.options({ surface, projection: 100 });
    assert.equal((await driver.state()).options.projection, 12);
    await driver.options({ projection: -100 });
    const state = await driver.state();
    assert.equal(
      state.options.projection,
      8.8,
      "all distance paths clamp to dashboard clearance",
    );
    clearance(state);
    await driver.capture(`clearance-${surface}`);
    await driver.press(0, "cockpit-control-tab/gui");
    const deadline = performance.now() + 10_000;
    while (
      !(await driver.state()).panels[0]!.entities.some(
        (entity) =>
          entity.metadata.symbolicId === "cockpit-surface-caching/off",
      )
    ) {
      assert.ok(performance.now() < deadline, "GUI tab mounts cache controls");
      await driver.advance(0.025);
    }
    for (const caching of ["auto", "force", "off"]) {
      await driver.press(0, `cockpit-surface-caching/${caching}`);
      await driver.advance(0.5);
      const changed = await driver.state();
      assert.equal(changed.options.surfaceCaching, caching);
      assert.deepEqual(
        changed.panelWorlds,
        panelWorlds,
        "cache controls preserve the panel Worlds and their state",
      );
      for (let panel = 0; panel < 3; panel++) {
        const policy = cockpitFields(
          changed.scene,
          `cockpit-screen-${panel}`,
          changed.componentIds.SurfaceCache!,
        );
        const live = caching === "off" || (caching === "auto" && panel === 0);
        const sharp = caching !== "auto" || panel === 0;
        assert.equal(
          policy.resolution_scale,
          sharp ? 1.5 : 0.75,
          "Off preserves focused quality on every panel; Automatic restores side-panel quality",
        );
        assert.ok(
          live
            ? Number(policy.direct_distance) > 12
            : policy.direct_distance === 0,
          "the native cache policy follows the routed GUI selection",
        );
      }
      await driver.capture(`caching-${surface}-${caching}`);
    }
    await driver.press(0, "cockpit-exploded");
    await driver.advance(0.2);
    const exploded = await driver.state();
    assert.equal(exploded.options.exploded, true);
    assert.deepEqual(exploded.panelWorlds, panelWorlds);
    assert.ok(
      Number(
        cockpitFields(
          exploded.scene,
          "cockpit-screen-0",
          exploded.componentIds[
            surface === "flat" ? "FlatSurface" : "CylinderSurface"
          ]!,
        ).layer_spacing,
      ) > 0,
    );
    await driver.capture(`exploded-${surface}`);
    await driver.press(0, "cockpit-exploded");
    assert.equal((await driver.state()).options.exploded, false);
    await driver.press(0, "cockpit-surface-refresh/slider", 0.25);
    const refresh = Number((await driver.state()).options.surfaceRefreshHz);
    assert.ok(refresh >= 6 && refresh <= 10, `slider selected ${refresh} Hz`);
    let smallAllocation = 0;
    for (const cap of [256, 2048]) {
      await driver.options({ surfaceCaching: "force" });
      await driver.press(0, "cockpit-surface-texture-size");
      await driver.advance(0.1);
      await driver.press(0, `cockpit-surface-texture-size/options/${cap}`);
      await driver.advance(0.5);
      const changed = await driver.state();
      assert.deepEqual(changed.panelWorlds, panelWorlds);
      for (let panel = 0; panel < 3; panel++) {
        const policy = cockpitFields(
          changed.scene,
          `cockpit-screen-${panel}`,
          changed.componentIds.SurfaceCache!,
        );
        assert.equal(policy.max_texture_size, cap);
        assert.equal(policy.max_refresh_hz, refresh);
        assert.equal(policy.force_cache, true);
      }
      await driver.capture(`texture-cap-${surface}-${cap}`);
      const bytes = (await driver.statistics()).surfaces
        .surfaceCacheResidentBytes;
      if (surface === "curved") {
        assert.ok(bytes > 0, "curved panels have actual image allocations");
        if (cap === 256) smallAllocation = bytes;
        else
          assert.ok(
            bytes > smallAllocation,
            "raising the cap restores image resolution and allocation",
          );
      }
    }
  }
  await driver.options({ surfaceCaching: "auto", surfaceRefreshHz: 5 });
  const observations = [];
  for (const focus of [0, 0.5, 1, 1.5, 2, 1]) {
    await driver.options({ carousel: focus });
    await driver.advance(0.5);
    const state = await driver.state();
    clearance(state);
    for (let index = 0; index < 3; index++) {
      const policy = cockpitFields(
        state.scene,
        `cockpit-screen-${index}`,
        state.componentIds.SurfaceCache!,
      );
      const near = Math.abs(focus - index) < 1;
      assert.equal(
        policy.resolution_scale,
        near ? 1.5 : 0.75,
        "incoming and outgoing panels retain sharp quality",
      );
      assert.ok(
        near
          ? Number(policy.direct_distance) > 12
          : policy.direct_distance === 0,
        "focused images refresh with current content",
      );
      assert.equal(policy.max_refresh_hz, 5);
    }
    const frame = await driver.capture(
      `quality-${observations.length}-${focus}`,
    );
    const ink =
      focus === 1 ? assertCockpitChartPixels(frame, state) : undefined;
    const statistics = await driver.statistics();
    assert.ok(
      statistics.surfaces.surfaceCacheResidentBytes <= 128 * 1024 * 1024,
    );
    observations.push({ focus, ink, surfaces: statistics.surfaces });
  }
  return observations;
}
