/** Spatial scene invariants, shared by actual presentation drivers. */
import assert from "node:assert/strict";
import type { CockpitDriver, CockpitState } from "./cockpit.js";
import {
  assertCylinder,
  assertFlat,
  cockpitFields,
  cockpitPlate,
  surfaceSample,
  type CockpitViewport,
} from "../cockpit-oracle.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

function entity(state: CockpitState, name: string) {
  const value = state.scene.entities.find(
    (item) => item.metadata.symbolicId === name,
  );
  assert.ok(value, `${name} is authored in the scene`);
  return value;
}

function phase(state: CockpitState, name = "phase") {
  const value = entity(state, "cockpit-planet").components.find(
    (item) => item.component === state.componentIds.CustomMaterial,
  )?.properties?.[name];
  assert.equal(value?.kind, "f32");
  return Number(value!.value);
}

function spatial(state: CockpitState) {
  const bridge = entity(state, "cockpit-bridge");
  const mount = entity(state, "cockpit-carousel-mount");
  assert.equal(
    mount.link.parent,
    bridge.id,
    "the bridge owns the cylinder mount",
  );
  assert.equal(entity(state, "cockpit-carousel").link.parent, mount.id);
  assert.equal(entity(state, "cockpit-console-shell").link.parent, bridge.id);
  const transform = state.componentIds.Transform!;
  const pose = cockpitFields(
    state.scene,
    mount.metadata.symbolicId!,
    transform,
  );
  [pose.x, pose.y, pose.z].forEach((value, index) =>
    assert.ok(Math.abs(Number(value) - [0, 3.4, 8][index]!) < 0.00001),
  );
  const planet = cockpitFields(state.scene, "cockpit-planet", transform);
  assert.ok(
    Number(planet.sy) < Number(planet.sx),
    "the planet has a flattened polar axis",
  );
  const quadrants = [
    "upper-left",
    "upper-right",
    "lower-left",
    "lower-right",
  ].map((name) => entity(state, `cockpit-bridge-${name}`));
  const textures = new Set<string>();
  for (const quadrant of quadrants) {
    assert.equal(quadrant.link.parent, bridge.id);
    const material = quadrant.components.find(
      (item) => item.component === state.componentIds.CustomMaterial,
    )?.properties;
    for (const index of [0, 1]) {
      assert.ok(
        material?.[`base${index}`] && material[`contribution${index}`],
        "each sphere section binds two square base/contribution pairs",
      );
      assert.notDeepEqual(
        material[`base${index}`],
        material[`contribution${index}`],
      );
      textures.add(JSON.stringify(material[`base${index}`]));
    }
  }
  assert.equal(
    textures.size,
    8,
    "the four sphere sections retain eight distinct base tiles",
  );
  // Recess landmarks measured on the authored 1672 × 941 camera plate.
  // Check the actual exported Surface, independently of its builder metadata.
  const recess = [
    [202.96, 839.19],
    [341.92, 816.03],
    [331.11, 890.65],
    [179.8, 911.24],
  ];
  const corners = [
    [0, 0],
    [1, 0],
    [1, 1],
    [0, 1],
  ];
  for (const [side, panel] of [4, 5].entries())
    for (const [index, [u, v]] of corners.entries()) {
      const { position: p } = surfaceSample(
        state.scene,
        state.componentIds,
        panel,
        u!,
        v!,
      );
      const depth = 8 - p[2]!;
      const actual = [
        836 + (p[0]! * (1672 / 1.2)) / depth,
        470.5 - ((p[1]! - 3.4) * (1672 / 1.2)) / depth,
      ];
      const source = recess[side === 0 ? index : [1, 0, 3, 2][index]!]!;
      const expected = [
        side === 0 ? source[0]! : 1672 - source[0]!,
        source[1]!,
      ];
      assert.ok(
        Math.hypot(actual[0]! - expected[0]!, actual[1]! - expected[1]!) < 3,
        `dashboard Surface ${panel} corner ${index} fits its instrument recess`,
      );
    }
  assertCylinder(
    state.scene,
    state.componentIds,
    Number(state.options.projection),
  );
  return [
    bridge,
    mount,
    entity(state, "cockpit-console-shell"),
    ...quadrants,
  ].map((item) =>
    cockpitFields(state.scene, item.metadata.symbolicId!, transform),
  );
}

function instrumentPixels(
  frame: RgbaImage,
  right: boolean,
  viewport: CockpitViewport,
) {
  const { scale, left, top } = cockpitPlate(frame, viewport);
  const pixels: number[] = [];
  for (let y = Math.ceil(top + 846 * scale); y < top + 879 * scale; y++)
    for (
      let x = Math.ceil(left + (right ? 1362 : 240) * scale);
      x < left + (right ? 1432 : 310) * scale;
      x++
    ) {
      const offset = (y * frame.width + x) * 4;
      pixels.push(
        frame.pixels[offset]!,
        frame.pixels[offset + 1]!,
        frame.pixels[offset + 2]!,
      );
    }
  return pixels;
}

export async function exerciseCockpitSpatial(driver: CockpitDriver) {
  await driver.options({
    surface: "curved",
    carousel: 0,
    scanning: false,
    lightsPlaying: false,
    atmospherePlaying: false,
  });
  await driver.advance(0.3);
  const initial = await driver.state();
  const poses = spatial(initial);
  const still = await driver.capture("cockpit-spatial-curved");
  const viewport = await driver.cameraViewport();
  let room = 0;
  let fallback = 0;
  for (let i = 0; i < still.pixels.length; i += 4) {
    const [r, g, b] = [
      still.pixels[i]!,
      still.pixels[i + 1]!,
      still.pixels[i + 2]!,
    ];
    if (r > 90 && Math.abs(r - g) < 35 && Math.abs(g - b) < 35) room++;
    if (r > 240 && g < 16 && b < 16) fallback++;
  }
  assert.ok(
    room > still.width * still.height * 0.02,
    "the physical cabin and console render at this aspect",
  );
  assert.ok(fallback < 256, "environment shaders compile and render");
  if (still.width / still.height > 1.65) {
    await driver.options({ scanning: true });
    const before = await driver.capture("cockpit-instruments-0");
    await driver.advance(0.65);
    const after = await driver.capture("cockpit-instruments-1");
    const waveform = instrumentPixels(before, false, viewport);
    assert.ok(
      instrumentPixels(after, false, viewport).every(
        (value, i) => Math.abs(value - waveform[i]!) <= 2,
      ),
      "waveform and grid stay steady without a travelling wash",
    );
    const radar = instrumentPixels(before, true, viewport);
    assert.ok(
      instrumentPixels(after, true, viewport).reduce(
        (sum, value, i) => sum + Math.abs(value - radar[i]!),
        0,
      ) > 20,
      "radar continues scanning on the fitted dashboard surface",
    );
    await driver.options({ scanning: false });
  }
  const paused = phase(await driver.state());
  const pausedCloud = phase(await driver.state(), "cloudPhase");
  await driver.advance(0.2);
  assert.equal(
    phase(await driver.state()),
    paused,
    "exterior pause holds rotation",
  );
  assert.equal(
    phase(await driver.state(), "cloudPhase"),
    pausedCloud,
    "exterior pause holds independent cloud drift",
  );
  await driver.options({ atmospherePlaying: true });
  await driver.advance(2);
  const moving = await driver.state();
  assert.ok(
    phase(moving) > paused + 0.005,
    "Host clock advances slow planet rotation",
  );
  assert.ok(
    phase(moving, "cloudPhase") > pausedCloud &&
      phase(moving, "cloudPhase") - pausedCloud < phase(moving) - paused,
    "weather drifts relative to the rotating terrain at a slower rate",
  );
  await driver.capture("cockpit-spatial-rotating");
  await driver.options({ atmospherePlaying: false, surface: "flat" });
  const flat = await driver.state();
  assertFlat(flat.scene, flat.componentIds, Number(flat.options.projection));
  await driver.capture("cockpit-spatial-flat");
  await driver.options({ surface: "curved" });
  assert.deepEqual(
    spatial(await driver.state()),
    poses,
    "projection changes retain fixed bridge and mount poses",
  );
  return {
    viewport: [still.width, still.height],
    roomPixels: room,
    phase: phase(moving),
    mountPoses: poses,
  };
}
