import assert from "node:assert/strict";
import test from "node:test";
import {
  ADVANCED_POSES,
  RECOVERY_POSE,
  imageExpectations,
} from "./gui-projected-advanced.js";
import {
  pixelFor,
  projectedSurfacePoint,
  projectedSurfaceRay,
} from "./gui-projected-surfaces.js";

test("advanced independent oracle provides interior overlap/parallax regions and transformed chart inverses", () => {
  for (const pose of [...ADVANCED_POSES, RECOVERY_POSE]) {
    const regions = imageExpectations(
      pose,
      Math.abs(pose.placement?.yaw ?? 0) > 2,
    );
    assert.ok(regions.overlap.length > 30);
    assert.ok(regions.revealed.length > 3);
    for (const rank of [0, 1])
      for (const point of [
        [30, 35],
        [110, 65],
      ] as const) {
        const pixel = pixelFor(projectedSurfacePoint(pose, point, rank));
        const inverse = projectedSurfaceRay(
          pose,
          [pixel[0] - 0.5, pixel[1] - 0.5],
          rank,
        );
        assert.ok(inverse, JSON.stringify(pose));
        assert.ok(
          Math.hypot(inverse[0] - point[0], inverse[1] - point[1]) < 1e-6,
          JSON.stringify({ pose, rank, point, inverse }),
        );
      }
  }
});
