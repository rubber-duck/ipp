import assert from "node:assert/strict";
import type { openApplication } from "../drivers/browser-application.js";

export async function exercisePlatformerStartup(
  app: Awaited<ReturnType<typeof openApplication>>,
) {
  const initial = await app.state();
  assert.equal(initial.options.scene, "platformer");
  assert.equal(initial.worlds.length, 3, "overlay, scene and movement GUI");
  const verify = async (mode: string, label: string) => {
    const state = await app.state();
    assert.equal(state.options.mode, mode);
    const controllers = state.scene.controllers ?? [];
    assert.equal(controllers.length, 4, "preparation controllers are retired");
    const origin = controllers.find(({ description }) =>
      description.drivers.some(({ source }) =>
        String(source).includes(`platformer-pose-origin-${mode}`),
      ),
    );
    assert.ok(origin, "the selected gait has its pose-origin controller");
    const gait = controllers.find(({ description }) =>
      description.drivers.some(
        ({ source, property }) =>
          String(source).startsWith("https://platformer.ipp.invalid/") &&
          "joints" in property,
      ),
    );
    assert.ok(
      gait && origin.id < gait.id,
      "base pose is applied before gait motion",
    );
    for (const controller of controllers)
      for (const driver of controller.description.drivers)
        assert.ok(
          state.scene.resources.some(
            (resource) =>
              resource.source === driver.source && resource.status === "loaded",
          ),
          "playback only uses loaded clips",
        );
    const frame = await app.capture(label);
    assert.ok(frame.frame.triangles > 1000, "the authored course is drawn");
    assert.ok(
      frame.scenePixels > frame.frame.width * frame.frame.height * 0.02,
      "the scene is visibly rendered below the overlay",
    );
  };
  await verify("walk", "platformer-walk");
  for (const mode of ["run", "crawl", "walk"]) {
    await app.action("setMode", mode);
    await verify(mode, `platformer-switched-${mode}`);
  }
  await app.action("reverse");
  await app.action("playback", false);
  assert.ok(
    (await app.inspect()).controllers?.every(({ state }) => state === "paused"),
  );
  await app.navigate("shapes");
  assert.equal(
    (await app.state()).worlds.length,
    2,
    "robot Worlds are retired",
  );
  await app.navigate("platformer");
  const reopened = await app.state();
  assert.equal(reopened.worlds.length, 3);
  assert.notDeepEqual(reopened.sceneWorlds, initial.sceneWorlds);
  await verify("walk", "platformer-reopened");
  assert.deepEqual(app.errors, []);
}
