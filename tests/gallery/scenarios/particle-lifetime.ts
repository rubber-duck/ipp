import assert from "node:assert/strict";
import type { Inspection, WorldReference } from "@ipp/client";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export interface ParticleLifetimeDriver {
  state(): Promise<{
    options: Readonly<Record<string, unknown>>;
    world: WorldReference;
    scene: Inspection;
    worlds: readonly bigint[];
  }>;
  action(name: string, args?: unknown): Promise<unknown>;
  update(options: Readonly<Record<string, unknown>>): Promise<void>;
  capture(name: string): Promise<{ image: RgbaImage; draws: number }>;
  advance(seconds: number): Promise<void>;
}
function difference(a: RgbaImage, b: RgbaImage) {
  let changed = 0;
  for (let offset = 0; offset < a.pixels.length; offset += 4)
    if (
      [0, 1, 2].some(
        (axis) =>
          Math.abs(a.pixels[offset + axis]! - b.pixels[offset + axis]!) > 20,
      )
    )
      changed++;
  return changed;
}
export async function exerciseParticleLifetime(driver: ParticleLifetimeDriver) {
  await driver.update({
    presentation: "meshes",
    emitting: false,
    rate: 600,
    lifetime: 0.6,
    size: 0.18,
    color: "#ff5500",
  });
  // Start with an empty emitter, so its two static meshes define the draw baseline.
  const empty = await driver.capture("particles-empty");
  await driver.action("restart");
  await driver.advance(0.35);
  const populated = await driver.capture("particles-populated");
  assert.ok(
    populated.draws > empty.draws,
    "live mesh particles add actual GPU draws",
  );
  assert.ok(difference(empty.image, populated.image) > 100);
  await driver.action("toggleEmission");
  await driver.advance(1.0); // Lifetime variation is bounded by the authored 25% range.
  const drained = await driver.capture("particles-drained");
  assert.equal(
    drained.draws,
    empty.draws,
    "disabled emitter drains all particle draws",
  );
  assert.equal(
    difference(empty.image, drained.image),
    0,
    "drained output matches the empty static scene",
  );
  const before = await driver.state();
  assert.equal(before.options.emitting, false);
  await driver.action("scene", "shapes");
  const replaced = await driver.state();
  assert.ok(
    !replaced.worlds.includes(before.world.id),
    "leaving retires the particle World",
  );
  await driver.action("scene", "particles");
  const returned = await driver.state();
  assert.notDeepEqual(returned.world, before.world);
  for (const key of [
    "presentation",
    "emitting",
    "rate",
    "lifetime",
    "size",
    "color",
    "restart",
  ])
    assert.equal(
      returned.options[key],
      before.options[key],
      `${key} survives leaving and returning`,
    );
  const fountain = returned.scene.entities.find(
    (entity) => entity.metadata.symbolicId === "particle-fountain",
  );
  assert.ok(fountain);
  const emitter = fountain.components.find(
    (component) => "rate" in component.fields,
  )!.fields;
  assert.equal(emitter.enabled, false);
  assert.equal(emitter.rate, 600);
  assert.ok(Math.abs(Number(emitter.lifetime) - 0.6) < 1e-6);
  await driver.capture("particles-returned");
}
