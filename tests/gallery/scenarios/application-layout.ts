/** Independent geometry and image assertions for the shared engine GUI shell. */
import assert from "node:assert/strict";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export interface LayoutRect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}
export interface ApplicationLayoutState {
  readonly viewport: readonly [number, number];
  readonly rects: Readonly<Record<string, LayoutRect>>;
  readonly scrollbars: Readonly<
    Record<string, Readonly<Record<string, unknown>>>
  >;
  readonly skins: Readonly<Record<string, string>>;
  readonly layouts: Readonly<Record<string, Readonly<Record<string, unknown>>>>;
}
export interface ApplicationLayoutDriver {
  layout(ids: readonly string[]): Promise<ApplicationLayoutState>;
  press(...ids: string[]): Promise<void>;
  scroll(id: string, delta: number): Promise<void>;
  action(name: string, value?: unknown): Promise<unknown>;
  capture(name: string): Promise<RgbaImage>;
  waitForOption(name: string, value: unknown): Promise<void>;
  interact(section: () => Promise<void>): Promise<void>;
}
const TOOLBAR = [
  "demo-toolbar-panel",
  "demo-toolbar-row",
  "demo-scene-picker",
  "demo-scene-picker/value",
  "demo-scene-picker/chevron",
  "demo-display",
  "demo-controls-toggle",
];
const close = (a: number, b: number, message: string) =>
  assert.ok(Math.abs(a - b) < 0.1, `${message}: ${a} vs ${b}`);
function inside(child: LayoutRect, parent: LayoutRect, inset = 0) {
  assert.ok(
    child.x >= parent.x + inset - 0.1 &&
      child.y >= parent.y + inset - 0.1 &&
      child.x + child.width <= parent.x + parent.width - inset + 0.1 &&
      child.y + child.height <= parent.y + parent.height - inset + 0.1,
    `contained geometry: ${JSON.stringify({ child, parent, inset })}`,
  );
}
function ordered(a: LayoutRect, b: LayoutRect) {
  assert.ok(
    a.x + a.width < b.x - 1,
    "adjacent controls have a real nonoverlapping gap",
  );
}
function cropInk(frame: RgbaImage, rect: LayoutRect) {
  let ink = 0;
  for (let y = Math.ceil(rect.y); y < rect.y + rect.height - 1; y++)
    for (let x = Math.ceil(rect.x); x < rect.x + rect.width - 1; x++) {
      const p = (y * frame.width + x) * 4;
      if (frame.pixels[p]! + frame.pixels[p + 1]! + frame.pixels[p + 2]! > 240)
        ink++;
    }
  assert.ok(
    ink > 30,
    "each toolbar control crop contains visible native GUI ink",
  );
}
export async function exerciseApplicationLayout(
  driver: ApplicationLayoutDriver,
) {
  for (const skin of ["glass", "cybertron"]) {
    await driver.action("skin", skin);
    const initial = await driver.layout(TOOLBAR);
    const r = initial.rects;
    const panel = r["demo-toolbar-panel"]!;
    const controlIds = [
      "demo-scene-picker",
      "demo-display",
      "demo-controls-toggle",
    ];
    const padding = Number(initial.layouts["demo-toolbar-panel"]!.padding_left);
    assert.ok(
      padding >= 8,
      "ordinary Panel has a content inset beyond its border",
    );
    for (const edge of [
      "padding_top",
      "padding_bottom",
      "padding_left",
      "padding_right",
    ])
      close(
        Number(initial.layouts["demo-toolbar-panel"]![edge]),
        padding,
        "balanced panel padding",
      );
    inside(panel, {
      x: 0,
      y: 0,
      width: initial.viewport[0],
      height: initial.viewport[1],
    });
    for (const id of controlIds) {
      inside(r[id]!, panel, padding);
      close(
        r[id]!.height,
        r["demo-scene-picker"]!.height,
        "toolbar regular control heights match",
      );
    }
    assert.ok(
      r["demo-scene-picker/value"]!.width > 40,
      "the ordinary scene label has visible text room",
    );
    inside(r["demo-scene-picker/value"]!, r["demo-scene-picker"]!);
    inside(r["demo-scene-picker/chevron"]!, r["demo-scene-picker"]!);
    const actions = [r["demo-display"]!, r["demo-controls-toggle"]!];
    close(
      actions[0]!.y + actions[0]!.height / 2,
      actions[1]!.y + actions[1]!.height / 2,
      "action centerlines match",
    );
    ordered(actions[0]!, actions[1]!);
    if (initial.viewport[0] >= 400) {
      close(
        r["demo-scene-picker"]!.y + r["demo-scene-picker"]!.height / 2,
        actions[0]!.y + actions[0]!.height / 2,
        "wide toolbar centerlines match",
      );
      ordered(r["demo-scene-picker"]!, actions[0]!);
    } else {
      assert.ok(
        r["demo-scene-picker"]!.y + r["demo-scene-picker"]!.height <
          actions[0]!.y - 1,
        "narrow toolbar flows into two separated rows",
      );
      assert.ok(
        r["demo-scene-picker"]!.width > 200,
        "narrow scene labels retain usable full-row width",
      );
    }
    const toolbarFrame = await driver.capture(`${skin}-toolbar`);
    for (const id of controlIds) cropInk(toolbarFrame, r[id]!);
    await driver.interact(async () => {
      await driver.press("demo-display");
      const display = await driver.layout([
        "demo-display/popover",
        "demo-display/header",
        "demo-display/content",
        "demo-display-content",
        "demo-skin-picker",
        "demo-render-scale",
        "demo-reset-view",
      ]);
      const box = display.rects["demo-display/popover"]!;
      inside(box, {
        x: 0,
        y: 0,
        width: display.viewport[0],
        height: display.viewport[1],
      });
      const fields = [
        "demo-skin-picker",
        "demo-render-scale",
        "demo-reset-view",
      ].map((id) => display.rects[id]!);
      for (const field of fields) inside(field, box, 8);
      close(fields[0]!.width, fields[1]!.width, "settings stretch uniformly");
      close(
        fields[1]!.width,
        fields[2]!.width,
        "settings button shares field width",
      );
      assert.ok(
        fields[0]!.y >
          display.rects["demo-display/header"]!.y +
            display.rects["demo-display/header"]!.height +
            4,
        "settings begin below padded header",
      );
      await driver.capture(`${skin}-display`);
      const nextSkin = skin === "glass" ? "cybertron" : "glass";
      await driver.press(
        "demo-skin-picker",
        `demo-skin-picker/options/${nextSkin}`,
      );
      await driver.waitForOption("skin", nextSkin);
      await driver.press("demo-display/close");
    });
    await driver.action("skin", skin);
    await driver.press("demo-controls-toggle");
    const controls = await driver.layout([
      "demo-toolbar-panel",
      "demo-controls-panel",
      "demo-controls-scroll",
      "demo-controls-scroll/body",
      "demo-controls-heading",
      "demo-controls-fields",
      "demo-scene-title",
      "demo-scene-description",
      "geometry-shape",
    ]);
    const cr = controls.rects;
    assert.ok(
      cr["demo-controls-panel"]!.height <= 560.1,
      "the controls panel respects its height cap at tall viewports",
    );
    assert.ok(
      cr["demo-toolbar-panel"]!.y + cr["demo-toolbar-panel"]!.height <
        cr["demo-controls-panel"]!.y - 1,
      "controls flow below measured toolbar",
    );
    inside(cr["demo-controls-panel"]!, {
      x: 0,
      y: 0,
      width: controls.viewport[0],
      height: controls.viewport[1],
    });
    inside(
      cr["demo-controls-scroll"]!,
      cr["demo-controls-panel"]!,
      Number(controls.layouts["demo-controls-panel"]!.padding_left),
    );
    inside(cr["demo-scene-title"]!, cr["demo-controls-scroll"]!);
    inside(cr["geometry-shape"]!, cr["demo-controls-scroll"]!);
    assert.ok(
      cr["demo-scene-description"]!.height < 64,
      "description uses measured wrapped text height",
    );
    const bar = controls.scrollbars["demo-controls-scroll"]!;
    const inset = Number(controls.layouts["demo-controls-panel"]!.padding_left);
    const scroll = cr["demo-controls-scroll"]!;
    const short = cr["demo-controls-panel"]!;
    close(
      short.height,
      scroll.height + 2 * inset,
      "one padded surface fits short content",
    );
    close(
      scroll.height,
      cr["demo-controls-scroll/body"]!.height,
      "short viewport has no unused vertical fill",
    );
    close(
      short.y +
        short.height -
        cr["geometry-shape"]!.y -
        cr["geometry-shape"]!.height,
      inset,
      "last short-form field ends at the panel inset",
    );
    assert.ok(short.height < 260, "short Geometry controls stay compact");
    assert.ok(
      cr["demo-scene-title"]!.height <= 20,
      "controls use a compact section heading",
    );
    assert.ok(
      cr["demo-controls-fields"]!.y >=
        cr["demo-controls-heading"]!.y +
          cr["demo-controls-heading"]!.height +
          8,
      "heading and fields have an intentional gap",
    );
    assert.equal(
      Number(bar.capacity_y),
      0,
      "short forms have no scroll capacity",
    );
    assert.equal(
      Number(bar.bar_thickness),
      0,
      "short forms show no empty scrollbar track",
    );
    assert.ok(
      controls.skins["demo-controls-scroll"]!.endsWith("/gridBody"),
      "the scroll viewport reuses its enclosing Panel surface",
    );
    await driver.capture(`${skin}-controls`);
    await driver.interact(async () => {
      await driver.press("geometry-shape");
      const popup = await driver.layout(["geometry-shape/list"]);
      inside(popup.rects["geometry-shape/list"]!, {
        x: 0,
        y: 0,
        width: controls.viewport[0],
        height: controls.viewport[1],
      });
      assert.ok(
        popup.rects["geometry-shape/list"]!.y +
          popup.rects["geometry-shape/list"]!.height >
          short.y + short.height,
        "shape options escape the short scroll viewport",
      );
      await driver.capture(`${skin}-shape-options`);
      await driver.press("geometry-shape/options/cube");
      await driver.waitForOption("shape", "cube");
    });
    const cube = await driver.layout([
      "demo-controls-panel",
      "demo-controls-scroll",
      "demo-controls-scroll/body",
      "geometry-finish",
      "geometry-width",
      "geometry-height",
      "geometry-length",
      "geometry-color",
    ]);
    assert.ok(
      cube.rects["demo-controls-panel"]!.height > short.height + 100,
      "the panel grows with the selected shape's form",
    );
    await driver.capture(`${skin}-selected-shape`);
    await driver.press("geometry-shape", "geometry-shape/options/plane");
    await driver.waitForOption("shape", "plane");
    const long = await driver.layout([
      "demo-controls-panel",
      "demo-controls-scroll",
      "demo-controls-scroll/body",
      "geometry-color",
    ]);
    const longPanel = long.rects["demo-controls-panel"]!;
    const longScroll = long.rects["demo-controls-scroll"]!;
    const longBody = long.rects["demo-controls-scroll/body"]!;
    assert.ok(
      longPanel.height <= 560.1,
      "long forms obey the authored height cap",
    );
    inside(longPanel, {
      x: 0,
      y: 0,
      width: long.viewport[0],
      height: long.viewport[1],
    });
    close(
      longPanel.height,
      Math.min(
        longBody.height + 2 * inset,
        560,
        long.viewport[1] - longPanel.y - 16,
      ),
      "the long panel fits content up to its available cap",
    );
    const longBar = long.scrollbars["demo-controls-scroll"]!;
    const overflowing = longBody.height > longScroll.height + 0.1;
    assert.equal(
      Number(longBar.capacity_y) > 0,
      overflowing,
      "only capped content has scroll capacity",
    );
    assert.equal(
      Number(longBar.bar_thickness) > 0,
      overflowing,
      "only overflow enables the scrollbar",
    );
    if (long.viewport[1] <= 480)
      assert.ok(overflowing, "the long form scrolls in a short viewport");
    if (overflowing) {
      const trackLeft =
        longScroll.x +
        longScroll.width -
        Number(longBar.bar_thickness) -
        Number(longBar.bar_inset);
      assert.ok(
        long.rects["geometry-color"]!.x + long.rects["geometry-color"]!.width <
          trackLeft - 1,
        "form fields leave real scrollbar clearance",
      );
    }
    await driver.capture(`${skin}-long-controls`);
    if (overflowing) await driver.scroll("demo-controls-scroll", 10000);
    const bottom = await driver.layout([
      "demo-controls-scroll",
      "geometry-color",
    ]);
    inside(
      bottom.rects["geometry-color"]!,
      bottom.rects["demo-controls-scroll"]!,
    );
    await driver.press("geometry-color", "geometry-color/options/#55ddff");
    await driver.capture(`${skin}-long-scrolled`);
    if (overflowing) await driver.scroll("demo-controls-scroll", -10000);
    const restored = await driver.layout([
      "demo-controls-scroll",
      "geometry-shape",
    ]);
    inside(
      restored.rects["geometry-shape"]!,
      restored.rects["demo-controls-scroll"]!,
    );
    close(
      Number(restored.scrollbars["demo-controls-scroll"]!.offset_y),
      0,
      "returning to the top reveals the shape selector before pointer input",
    );
    await driver.press("geometry-shape", "geometry-shape/options/gallery");
    await driver.waitForOption("shape", "gallery");
    await driver.press("demo-controls-toggle");
  }
}
