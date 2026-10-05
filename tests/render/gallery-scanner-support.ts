/** Shared scanner navigation uses real retained GUI input and completed state. */
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { galleryEnvironment } from "./gallery-driver.js";
import { projectContent, type ContentRect } from "./gallery-gui-panel.js";
import {
  guiApplication,
  waitForGuiState,
  type Gallery,
} from "./gallery-gui-support.js";
import type { GalleryGuiControl } from "./viewer-browser-helper.js";
export const environment = (name: string) => ({
  ...galleryEnvironment,
  evidenceParent: resolve(
    `target/integration-artifacts/gallery-scanner/${name}`,
  ),
});
export async function waitApp(
  g: Gallery,
  predicate: (app: Awaited<ReturnType<typeof guiApplication>>) => boolean,
) {
  for (let attempt = 0; attempt < 100; attempt++) {
    const value = await guiApplication(g);
    if (predicate(value)) return value;
    await g.inspect();
  }
  throw new Error("Scanner app did not reach the expected state");
}
export async function find(g: Gallery, symbol: string, flush = true) {
  const state = await waitForGuiState(
    g,
    (s) => s.controls.some((c) => c.symbol === symbol && c.visible),
    flush,
  );
  return state.controls.find((c) => c.symbol === symbol)!;
}
export async function press(
  g: Gallery,
  control: GalleryGuiControl,
  depth: number,
  flush = true,
) {
  const [x, y, width, height] = control.bounds;
  const [point] = await projectContent(
    g,
    [[x + width / 2, y + height / 2]],
    depth,
    flush,
  );
  await g.page.mouse.move(point!.clientX, point!.clientY);
  const hovered = await waitForGuiState(
    g,
    (s) =>
      s.controls.some(
        (c) => c.symbol === control.symbol && c.interaction.hovered,
      ),
    flush,
  );
  assert.ok(
    hovered.controls.find((c) => c.symbol === control.symbol)?.available,
  );
  await g.page.mouse.down();
  await g.page.mouse.up();
}
export function expected(control: GalleryGuiControl, rect: ContentRect) {
  control.bounds.forEach((value, index) =>
    assert.ok(
      Math.abs(value - rect[index]!) < 0.1,
      `${control.symbol}: ${JSON.stringify(control.bounds)} vs ${JSON.stringify(rect)}`,
    ),
  );
}

/** Login through the native editor, then await the Host-clock connection sequence. */
export async function enterWorkspace(g: Gallery) {
  await g.page.waitForFunction(
    () =>
      document.querySelector<HTMLOutputElement>("#status")?.dataset.state ===
      "ready",
  );
  await waitApp(g, (value) => value.ready);
  const password = await find(g, "gui-password");
  await press(g, password, 0.1);
  await waitForGuiState(g, (s) =>
    s.controls.some((c) => c.symbol === "gui-password" && c.focused),
  );
  await g.page.waitForFunction(
    () => document.activeElement === document.querySelector("textarea"),
  );
  await g.page.keyboard.insertText("local demo");
  await waitApp(g, (value) => value.state.app.password === "local demo");
  await g.page.keyboard.press("Enter");
  await waitApp(g, (value) => value.state.app.phase === "workspace");
  await g.capture("scanner-workspace-open");
}
export async function openSettings(
  g: Gallery,
  page: "display" | "projection" | "scene" = "display",
) {
  await press(g, await find(g, "gui-settings-open"), await spacing(g));
  await waitApp(g, (value) => value.state.app.settings);
  await g.capture("scanner-settings-open");
  if (page !== "display") await selectSettingsPage(g, page);
}
export async function selectSettingsPage(
  g: Gallery,
  page: "display" | "projection" | "scene",
  flush = true,
) {
  const state = await waitForGuiState(
    g,
    (s) => s.controls.some((c) => c.label === page.toUpperCase() && c.visible),
    flush,
  );
  const control = state.controls.find(
    (c) => c.label === page.toUpperCase() && c.visible,
  )!;
  await press(g, control, 4 * (await spacing(g)), flush);
  await waitApp(g, (value) => value.state.app.settingsPage === page);
  if (flush) await g.capture(`scanner-settings-${page}-ready`);
  else await g.call("captureUnflushedViewer", `scanner-settings-${page}-ready`);
}

export async function selectPresentationPage(
  g: Gallery,
  label: "LAYERS" | "SURFACE" | "STYLE",
) {
  const s = await waitForGuiState(g, (state) =>
    state.controls.some((c) => c.label === label && c.visible),
  );
  await press(
    g,
    s.controls.find((c) => c.label === label && c.visible)!,
    5 * (await spacing(g)),
  );
  await waitApp(
    g,
    (value) =>
      value.state.presentationTab ===
      (label === "SURFACE" ? "shell" : label.toLowerCase()),
  );
  await g.capture(`scanner-presentation-${label.toLowerCase()}-ready`);
}
export async function spacing(g: Gallery) {
  const inspected = await g.inspect();
  const panel = inspected.entities.find(
    (e) => e.metadata.symbolicId === "gui-demo",
  );
  assert.ok(panel);
  const fields = panel.components.find(
    (c) => "layer_spacing" in c.fields,
  )?.fields;
  assert.ok(fields);
  return Number(fields.layer_spacing);
}
export async function waitSpacing(g: Gallery, expected: number) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (Math.abs((await spacing(g)) - expected) < 1e-5) return;
    await g.inspect();
  }
  throw new Error(`Layer spacing did not settle at ${expected}`);
}
export async function dropdown(g: Gallery, id: string, label: string) {
  await press(g, await find(g, id), 5 * (await spacing(g)));
  const state = await waitForGuiState(g, (s) =>
    s.controls.some(
      (c) => c.label === label && c.visible && c.symbol?.includes(`${id}/`),
    ),
  );
  await press(
    g,
    state.controls.find(
      (c) => c.label === label && c.visible && c.symbol?.includes(`${id}/`),
    )!,
    6 * (await spacing(g)),
  );
  await g.capture(`scanner-option-${label.toLowerCase()}-ready`);
}
