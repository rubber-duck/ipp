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

function chartInk(frame: RgbaImage, state: CockpitState) {
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
  }
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
      assert.equal(policy.max_refresh_hz, 15);
    }
    const frame = await driver.capture(
      `quality-${observations.length}-${focus}`,
    );
    const ink = focus === 1 ? chartInk(frame, state) : undefined;
    const statistics = await driver.statistics();
    assert.ok(
      statistics.surfaces.surfaceCacheResidentBytes <= 32 * 1024 * 1024,
    );
    observations.push({ focus, ink, surfaces: statistics.surfaces });
  }
  return observations;
}
