/** Native-window observations, independent of process launch and wire layout. */
import assert from "node:assert/strict";
import { setTimeout } from "node:timers/promises";

export async function exerciseNativeWindow(driver) {
  const { inspect, post, wait } = driver;
  const capture = async (name) => {
    const result = await driver.capture(name);
    const { frame } = result;
    // These demo scenes have an opaque background across the whole drawable.
    // In particular, a Retina/render-scale mismatch must not return zero pixels
    // outside a smaller underlying EGL framebuffer.
    for (const [x, y] of [
      [0, 0],
      [frame.width - 1, 0],
      [0, frame.height - 1],
      [frame.width - 1, frame.height - 1],
    ])
      assert.equal(
        frame.pixels[(y * frame.width + x) * 4 + 3],
        255,
        `${name}: full drawable is rendered`,
      );
    let bright = 0,
      different = 0;
    for (let y = Math.floor(frame.height / 3); y < frame.height; y++)
      for (let x = 0; x < frame.width; x++) {
        const i = (y * frame.width + x) * 4;
        if (Math.max(...frame.pixels.subarray(i, i + 3)) > 100) bright++;
        if (
          [0, 1, 2].some(
            (c) => Math.abs(frame.pixels[i + c] - frame.pixels[c]) > 24,
          )
        )
          different++;
      }
    assert.ok(
      bright > frame.width * frame.height * 0.01,
      "scene has visible geometry",
    );
    assert.ok(
      different > frame.width * frame.height * 0.02,
      "scene contributes non-background pixels",
    );
    return result;
  };
  const initial = await capture("initial");
  assert.ok(initial.frame.width >= 960);
  const press = async (id) => {
    const snapshot = await inspect();
    const entity = snapshot.application.shell.entities.find(
      (entity) => entity.metadata.symbolicId === id,
    );
    const rect = entity?.components.find(({ fields }) =>
      ["x", "y", "width", "height"].every((name) => name in fields),
    )?.fields;
    assert.ok(rect, `${id} has evaluated bounds`);
    const { viewport } = await capture(`before-${id.replaceAll("/", "-")}`);
    const x =
      ((Number(rect.x) + Number(rect.width) / 2) * viewport.devicePixelRatio) /
      viewport.width;
    const y =
      ((Number(rect.y) + Number(rect.height) / 2) * viewport.devicePixelRatio) /
      viewport.height;
    await post(2, x, y);
    await post(3, x, y);
    await setTimeout(100);
  };
  await press("demo-scene-picker");
  await press("demo-scene-picker/options/lighting");
  await wait(async () => (await inspect()).options.scene === "lighting");
  await capture("clicked-lighting");
  await post(8, 720, 520);
  await wait(async () => {
    const reply = await driver.capture("resize");
    return reply.viewport.width / reply.viewport.devicePixelRatio === 720;
  });
  const resized = await capture("resized");
  assert.equal(resized.frame.height / resized.viewport.devicePixelRatio, 520);
  await driver.scale(1.5);
  const scaled = await capture("scaled");
  assert.equal(scaled.frame.width, Math.round(resized.frame.width * 1.5));
  await driver.reload();
  const reloaded = await capture("reloaded");
  assert.equal(reloaded.frame.width, scaled.frame.width);
  assert.equal((await inspect()).options.scene, "lighting");
  await driver.closeWindow();
  return resized.viewport;
}
