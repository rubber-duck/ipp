import assert from "node:assert/strict";
import type { ApplicationFontsState } from "../support/application-fonts.js";

export interface ApplicationFontsDriver {
  fonts(): Promise<ApplicationFontsState>;
  press(...ids: string[]): Promise<void>;
  action(name: string, value?: unknown): Promise<unknown>;
  waitForOption(name: string, value: unknown): Promise<unknown>;
  capture(name: string): Promise<unknown>;
  changedPixels(before: string, after: string): Promise<number>;
}

/** The same physical dropdown scenario runs through native GLES and browser WASM. */
export async function exerciseApplicationFonts(driver: ApplicationFontsDriver) {
  await driver.press("demo-display");
  const mono = await driver.fonts();
  assert.equal(mono.options.font, "shure-tech-mono");
  await driver.capture("mono-display");
  await driver.press("demo-font-picker", "demo-font-picker/options/noto-sans");
  await driver.waitForOption("font", "noto-sans");
  const noto = await driver.fonts();
  await driver.capture("noto-display");
  assert.notEqual(noto.source, mono.source);
  assert.equal(noto.icon, mono.icon, "icon glyphs retain their font");
  assert.equal(
    noto.button,
    mono.button,
    "font switching preserves control identity",
  );
  assert.deepEqual(noto.root, mono.root);
  assert.deepEqual(noto.sceneWorlds, mono.sceneWorlds);
  assert.ok(
    Math.abs(noto.width - mono.width) > 2,
    "server font metrics resize a content-sized button",
  );
  assert.ok(
    (await driver.changedPixels("mono-display", "noto-display")) > 100,
    "switching fonts changes rendered glyphs",
  );
  await driver.press("demo-skin-picker", "demo-skin-picker/options/cybertron");
  await driver.waitForOption("skin", "cybertron");
  assert.equal(
    (await driver.fonts()).source,
    noto.source,
    "theme changes preserve font selection",
  );
  await driver.capture("noto-cybertron");
  await assert.rejects(
    driver.action("font", "missing-font"),
    /Unknown demo font/,
  );
  assert.equal((await driver.fonts()).options.font, "noto-sans");
  await driver.action("skin", "glass");
  await driver.press(
    "demo-font-picker",
    "demo-font-picker/options/shure-tech-mono",
  );
  await driver.waitForOption("font", "shure-tech-mono");
  const restored = await driver.fonts();
  assert.equal(restored.source, mono.source);
  assert.ok(Math.abs(restored.width - mono.width) < 0.1);
  assert.equal(restored.button, mono.button);
  await driver.action("font", "noto-sans");
  await driver.action("scene", "lighting");
  await driver.waitForOption("scene", "lighting");
  const navigated = await driver.fonts();
  assert.equal(navigated.options.font, "noto-sans");
  assert.equal(navigated.options.skin, "glass");
  assert.equal(navigated.source, noto.source);
  assert.deepEqual(navigated.root, mono.root);
  assert.notDeepEqual(navigated.sceneWorlds, mono.sceneWorlds);
}
