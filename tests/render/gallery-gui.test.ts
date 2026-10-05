/** Direct/texture image agreement for the current scanner application. */
import assert from "node:assert/strict";
import type { Inspection } from "@ipp/client";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { openGallery } from "./gallery-driver.js";
import { projectContent } from "./gallery-gui-panel.js";
import {
  decodeRegion,
  guiApplication,
  waitForGuiState,
  sceneEntity,
} from "./gallery-gui-support.js";
import {
  environment,
  enterWorkspace,
  openSettings,
  selectPresentationPage,
  dropdown,
  find,
  press,
  waitApp,
  spacing,
} from "./gallery-scanner-support.js";
import {
  compareFrames,
  differenceImage,
  encodePng,
  type RgbaFrame,
} from "./retained-gui-images.js";

/** Existing maintained cache quality thresholds, unchanged for the new app. */
const CHANNEL_THRESHOLD = 64,
  MAX_CHANGED_FRACTION = 0.035,
  BRIGHT_LEVEL = 170,
  TEXT_TOLERANCE = 20,
  MIN_TEXT_AGREEMENT = 0.95;
function brightAgreement(a: RgbaFrame, b: RgbaFrame) {
  let bright = 0,
    agreed = 0;
  for (let at = 0; at < a.pixels.length; at += 4) {
    const levelA = Math.max(
      a.pixels[at]!,
      a.pixels[at + 1]!,
      a.pixels[at + 2]!,
    );
    const levelB = Math.max(
      b.pixels[at]!,
      b.pixels[at + 1]!,
      b.pixels[at + 2]!,
    );
    if (Math.max(levelA, levelB) < BRIGHT_LEVEL) continue;
    bright++;
    if (Math.min(levelA, levelB) >= BRIGHT_LEVEL - TEXT_TOLERANCE) agreed++;
  }
  return { bright, agreement: bright ? agreed / bright : 1 };
}

/** Unequal raster quality may move AA coverage within one output pixel.
 * Retain ink area and contrast separately so nearby pixels cannot hide
 * missing text, thickened strokes or a filled rectangle. */
function glyphQuality(expected: RgbaFrame, actual: RgbaFrame) {
  assert.equal(expected.width, actual.width);
  assert.equal(expected.height, actual.height);
  const levels = (frame: RgbaFrame) =>
    Array.from({ length: frame.width * frame.height }, (_, index) =>
      Math.max(...frame.pixels.subarray(index * 4, index * 4 + 3)),
    );
  const a = levels(expected),
    b = levels(actual);
  const area = (values: readonly number[]) =>
    values.reduce(
      (total, value) =>
        total + Math.min(1, Math.max(0, (value - 40) / (BRIGHT_LEVEL - 40))),
      0,
    );
  let bright = 0,
    matched = 0;
  for (const [from, to] of [
    [a, b],
    [b, a],
  ])
    for (let at = 0; at < from!.length; at++) {
      if (from![at]! < BRIGHT_LEVEL) continue;
      bright++;
      const x = at % expected.width,
        y = Math.floor(at / expected.width);
      let near = 0;
      for (
        let row = Math.max(0, y - 1);
        row <= Math.min(expected.height - 1, y + 1);
        row++
      )
        for (
          let column = Math.max(0, x - 1);
          column <= Math.min(expected.width - 1, x + 1);
          column++
        )
          near = Math.max(near, to![row * expected.width + column]!);
      if (near >= BRIGHT_LEVEL - TEXT_TOLERANCE) matched++;
    }
  return {
    bright,
    matched,
    coveredAreaRatio: area(b) / area(a),
    peakRatio: Math.max(...b) / Math.max(...a),
  };
}

function acceptableGlyphs(
  regions: readonly ReturnType<typeof glyphQuality>[],
  minified = false,
) {
  const bright = regions.reduce((sum, r) => sum + r.bright, 0);
  const matched = regions.reduce((sum, r) => sum + r.matched, 0);
  return (
    bright >= 100 &&
    matched / bright >= MIN_TEXT_AGREEMENT &&
    regions.every(
      (r) =>
        r.coveredAreaRatio >= 0.9 &&
        (minified || r.coveredAreaRatio <= 1.25) &&
        r.peakRatio >= 0.85,
    )
  );
}

/** Reference perturbations verify that the oracle rejects lost strokes,
 * missing text, material blur, dimming and filled glyph bounds. */
function corruptGlyphs(
  frame: RgbaFrame,
  kind: "half" | "strokes" | "blur" | "dim" | "fill",
): RgbaFrame {
  const pixels = frame.pixels.slice();
  const inkRows = Array.from({ length: frame.height }, (_, y) => y).filter(
    (y) =>
      Array.from({ length: frame.width }, (_, x) => x).some(
        (x) =>
          Math.max(
            ...frame.pixels.subarray(
              (y * frame.width + x) * 4,
              (y * frame.width + x) * 4 + 3,
            ),
          ) >= BRIGHT_LEVEL,
      ),
  );
  // Damage a quarter of the actual ink height, beyond the admitted one-pixel
  // AA footprint; layout padding does not set this negative perturbation.
  const blurRadius = Math.max(
    2,
    Math.ceil(((inkRows.at(-1) ?? 0) - (inkRows[0] ?? 0) + 1) / 4),
  );
  for (let y = 0; y < frame.height; y++)
    for (let x = 0; x < frame.width; x++)
      for (let channel = 0; channel < 3; channel++) {
        const at = (y * frame.width + x) * 4 + channel;
        if (
          (kind === "half" && x >= frame.width / 2) ||
          (kind === "strokes" && x % 3 === 1)
        )
          pixels[at] = 0;
        if (kind === "fill") pixels[at] = 255;
        if (kind === "dim") pixels[at] = Math.floor(frame.pixels[at]! * 0.85);
        if (kind === "blur") {
          let value = 0,
            count = 0;
          for (
            let row = Math.max(0, y - blurRadius);
            row <= Math.min(frame.height - 1, y + blurRadius);
            row++
          )
            for (
              let column = Math.max(0, x - blurRadius);
              column <= Math.min(frame.width - 1, x + blurRadius);
              column++
            ) {
              value +=
                frame.pixels[(row * frame.width + column) * 4 + channel]!;
              count++;
            }
          pixels[at] = Math.round(value / count);
        }
      }
  return { ...frame, pixels };
}

test("Gallery GUI panel caches curved scanner presentation within direct-rendering tolerance", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Scanner texture presentation",
    environment("cache"),
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      await enterWorkspace(g);
      await press(g, await find(g, "gui-scan"), 2 * (await spacing(g)));
      await waitApp(g, (v) => !v.state.autoscan);
      await openSettings(g);
      await press(g, await find(g, "gui-vector-only"), 5 * (await spacing(g)));
      await waitApp(g, (v) => v.state.vectorOnly);
      await selectPresentationPage(g, "STYLE");
      await press(
        g,
        await find(g, "gui-reduced-motion"),
        5 * (await spacing(g)),
      );
      await waitApp(g, (v) => v.state.reducedMotion);
      await selectPresentationPage(g, "SURFACE");
      await dropdown(g, "gui-surface-shape", "CYLINDER");
      const setMode = async (mode: "cached" | "direct") => {
        if (!(await guiApplication(g)).state.app.settings)
          await openSettings(g);
        await selectPresentationPage(g, "SURFACE");
        if ((await guiApplication(g)).state.surfaceCache !== mode)
          await dropdown(g, "gui-surface-cache", mode.toUpperCase());
        await press(
          g,
          await find(g, "gui-settings-close"),
          4 * (await spacing(g)),
        );
        await waitApp(g, (v) => !v.state.app.settings);
        await g.page.mouse.click(1, 1);
        await g.page.mouse.move(1, 1);
        await new Promise((resolve) => setTimeout(resolve, 550));
        await g.capture(`scanner-${mode}-settled`);
      };
      const panel = await g.call<bigint>("galleryEntityId", "gui-demo");
      for (const quality of ["minified", "workspace"] as const) {
        // Keep the ordinary distant band and its measured ink drift separate
        // from full-size app text quality, without changing production density.
        if (quality === "minified")
          await g.call("faceGalleryGuiToCamera", 0.55, 24);
        else await g.call("releaseGalleryGuiTransform");
        await setMode("cached");
        let cached = await g.capture(`scanner-cached-${quality}`);
        for (let attempt = 0; attempt < 100; attempt++) {
          const entries =
            cached.frame.statistics?.surfaces?.surfaceCaches.filter(
              (r) => r.entity === panel,
            ) ?? [];
          if (
            entries.length > 0 &&
            entries.every((r) => r.mode === "reused" && r.residentBytes > 0)
          )
            break;
          assert.ok(
            attempt < 99,
            `curved canvas did not use resident texture images: ${JSON.stringify(entries, (_, v) => (typeof v === "bigint" ? String(v) : v))}`,
          );
          cached = await g.capture(`scanner-cached-${quality}`);
        }
        const cachedGui = await waitForGuiState(g);
        await setMode("direct");
        const direct = await g.capture(`scanner-direct-${quality}`);
        assert.ok(
          sceneEntity(cached.inspection, "gui-demo").components.some(
            (c) => "max_refresh_hz" in c.fields,
          ),
          "cached mode omitted optional SurfaceCache policy",
        );
        assert.ok(
          !sceneEntity(direct.inspection, "gui-demo").components.some(
            (c) => "max_refresh_hz" in c.fields,
          ),
          "direct mode retained optional SurfaceCache policy",
        );
        const directImages =
          direct.frame.statistics!.surfaces!.surfaceCaches.filter(
            (r) => r.entity === panel,
          );
        assert.ok(
          directImages.length > 0 &&
            directImages.every((r) => r.residentBytes > 0),
          "curved direct presentation lost its required raster images",
        );
        const directGui = await waitForGuiState(g);
        assert.deepEqual(
          directGui.controls
            .filter((c) => c.visible)
            .map((c) => [c.symbol, c.bounds, c.focused, c.interaction.hovered]),
          cachedGui.controls
            .filter((c) => c.visible)
            .map((c) => [c.symbol, c.bounds, c.focused, c.interaction.hovered]),
          "comparison changed visible layout/focus/hover",
        );
        const corners = await projectContent(
          g,
          [
            [0, 0],
            [1036, 0],
            [0, 672],
            [1036, 672],
            [518, 0],
            [518, 672],
          ],
          3 * (await spacing(g)),
        );
        const rect = [
          Math.min(...corners.map((p) => p.x)),
          Math.min(...corners.map((p) => p.y)),
          Math.max(...corners.map((p) => p.x)),
          Math.max(...corners.map((p) => p.y)),
        ];
        const [expected, actual] = await Promise.all(
          [direct.label, cached.label].map((label) =>
            g
              .call<{ width: number; height: number; pixels: string }>(
                "viewerCaptureRegionPixels",
                label,
                rect,
              )
              .then(decodeRegion),
          ),
        );
        const difference = compareFrames(expected!, actual!, CHANNEL_THRESHOLD);
        const text = brightAgreement(expected!, actual!);
        const panelInspection = await g.call<Inspection>("inspectGalleryPanel");
        const glyphRegions = [];
        const glyphFrames: RgbaFrame[] = [];
        for (const [symbol, rank] of [
          ["gui-workspace-title", 0],
          ["gui-receiver-panel/header/title", 2],
          ["gui-rate-title", 2],
          ["gui-scanner-status", 1],
        ] as const) {
          const entity = panelInspection.entities.find(
            (e) => e.metadata.symbolicId === symbol,
          );
          assert.ok(entity);
          const bounds = entity.components.find(
            (c) =>
              "width" in c.fields && "x" in c.fields && !("kind" in c.fields),
          )?.fields;
          const leaf = entity.components.find(
            (c) => "font_size" in c.fields && "text" in c.fields,
          )?.fields;
          assert.ok(bounds && leaf);
          // The bundled monospaced font's independent 0.54em advance bounds
          // glyph ink; this excludes radar curves and control frame geometry.
          const width = Math.min(
            Number(bounds.width),
            String(leaf.text).length * Number(leaf.font_size) * 0.54,
          );
          const points = await projectContent(
            g,
            [
              [Number(bounds.x), Number(bounds.y)],
              [
                Number(bounds.x) + width,
                Number(bounds.y) + Number(bounds.height),
              ],
            ],
            0,
          );
          const region = [
            Math.min(...points.map((p) => p.x)) - 1 / direct.frame.width,
            Math.min(...points.map((p) => p.y)) - 1 / direct.frame.height,
            Math.max(...points.map((p) => p.x)) + 1 / direct.frame.width,
            Math.max(...points.map((p) => p.y)) + 1 / direct.frame.height,
          ];
          const frames = await Promise.all(
            [direct.label, cached.label].map((label) =>
              g
                .call<{ width: number; height: number; pixels: string }>(
                  "viewerCaptureRegionPixels",
                  label,
                  region,
                )
                .then(decodeRegion),
            ),
          );
          glyphRegions.push({
            symbol,
            rank,
            bounds: region,
            ...brightAgreement(frames[0]!, frames[1]!),
            ...glyphQuality(frames[0]!, frames[1]!),
          });
          glyphFrames.push(frames[0]!);
          await Promise.all(
            frames.map((frame, index) =>
              writeFile(
                resolve(
                  scenario.evidence.directory,
                  `${quality}-${symbol.replaceAll("/", "-")}-${index === 0 ? "direct" : "cached"}.png`,
                ),
                encodePng(frame!),
              ),
            ),
          );
        }
        await scenario.evidence.record(
          `scanner-cache-${quality}-glyph-regions`,
          glyphRegions,
        );
        await scenario.evidence.record(`scanner-cache-${quality}-comparison`, {
          difference,
          text,
          rect,
          cached: cached.frame.statistics?.surfaces,
          direct: direct.frame.statistics?.surfaces,
        });
        await Promise.all([
          writeFile(
            resolve(
              scenario.evidence.directory,
              `${quality}-scanner-cache-expected.png`,
            ),
            encodePng(expected!),
          ),
          writeFile(
            resolve(
              scenario.evidence.directory,
              `${quality}-scanner-cache-actual.png`,
            ),
            encodePng(actual!),
          ),
          writeFile(
            resolve(
              scenario.evidence.directory,
              `${quality}-scanner-cache-difference.png`,
            ),
            encodePng(differenceImage(expected!, actual!, CHANNEL_THRESHOLD)),
          ),
        ]);
        assert.ok(text.bright > 500, "scanner text/sweep is missing");
        assert.ok(
          difference.changedFraction <= MAX_CHANGED_FRACTION &&
            acceptableGlyphs(glyphRegions, quality === "minified"),
          `cached/direct ${quality} image mismatch: ${JSON.stringify({ difference, text, glyphRegions })}`,
        );
        if (quality === "workspace") {
          const reference = glyphFrames.map((frame) =>
            glyphQuality(frame, frame),
          );
          assert.ok(acceptableGlyphs(reference));
          for (let index = 0; index < glyphFrames.length; index++)
            for (const kind of [
              "half",
              "strokes",
              "blur",
              "dim",
              "fill",
            ] as const) {
              const metrics = glyphQuality(
                glyphFrames[index]!,
                corruptGlyphs(glyphFrames[index]!, kind),
              );
              const regions = reference.map((r, i) =>
                i === index ? metrics : r,
              );
              await scenario.evidence.record("glyph-oracle-negative", {
                symbol: glyphRegions[index]!.symbol,
                source: "direct reference",
                kind,
                metrics,
              });
              assert.ok(
                !acceptableGlyphs(regions),
                `${glyphRegions[index]!.symbol} ${kind} corruption passed`,
              );
            }
        }
        assert.equal(cached.frame.failedDrawCalls, 0);
        assert.equal(direct.frame.failedDrawCalls, 0);
      }
      await setMode("cached");
      await g.capture("scanner-final-cached");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-final-cached-page.png",
        ),
        fullPage: true,
      });
      assert.deepEqual(g.errors, []);
    },
  );
});
