import assert from "node:assert/strict";
import { join } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import { until } from "../harness/page/checks.js";
import {
  choiceHighlight,
  journalOutline,
  rangeLabelsStayVisible,
  stepCornersStayClear,
} from "./scenarios/cartoon-interactions.js";
import { applicationEnvironment } from "./drivers/browser-application.js";

test("Cartoon gallery presents every section and varies a segment on reselection", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Cartoon controls",
    applicationEnvironment,
    context.signal,
    async ({ page, urls, evidence }) => {
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      await page.setViewportSize({ width: 800, height: 650 });
      await page.goto(
        new URL("/examples/demos/cartoon-controls/index.html", urls.origin)
          .href,
      );
      const wait = <T>(label: string, read: () => Promise<T>) =>
        until(read, Boolean, label, { intervalMs: 75, timeoutMs: 25_000 });
      const control = async (id: string, selected?: boolean) => {
        const result = await wait(id, () =>
          page.evaluate(
            async ({ id, selected }) => {
              const canvas = window.ippDemoCanvas;
              if (!canvas) return;
              const client = canvas.client;
              const entity = (await client.inspect()).entities.find(
                (e) => e.metadata.symbolicId === id,
              );
              const fields = (
                name:
                  | "CanvasBounds"
                  | "CanvasStyle"
                  | "GuiSkin"
                  | "GuiButton"
                  | "GuiBehavior"
                  | "GuiScrollView"
                  | "GuiVirtualList",
              ) =>
                entity?.components.find(
                  (c) => c.component === client.components[name]?.id,
                )?.fields;
              if (
                !entity ||
                fields("GuiBehavior")?.available !== true ||
                fields("GuiBehavior")?.effective_visible !== true ||
                (selected !== undefined &&
                  fields("GuiButton")?.selected !== selected)
              )
                return;
              const bounds = fields("CanvasBounds");
              if (
                !bounds ||
                Number(bounds.width) <= 0 ||
                Number(bounds.height) <= 0
              )
                return;
              return {
                x: Number(bounds.x),
                y: Number(bounds.y),
                width: Number(bounds.width),
                height: Number(bounds.height),
                rotation: Number(fields("CanvasStyle")?.rotation ?? 0),
                variant: String(fields("GuiSkin")?.theme),
                bar: Number(fields("GuiScrollView")?.bar_thickness ?? 0),
                offsetY: (fields("GuiScrollView") ?? fields("GuiVirtualList"))
                  ?.offset_y,
              };
            },
            { id, selected },
          ),
        );
        assert.ok(result);
        return result;
      };
      const press = async (id: string) => {
        const rect = await control(id);
        await page.mouse.click(
          rect.x + rect.width / 2,
          rect.y + rect.height / 2,
        );
      };
      const hoverChoice = async (id: string) => {
        const rect = await control(id, false);
        const [rest] = await page.evaluate(choiceHighlight, { rect, count: 1 });
        const [frames] = await Promise.all([
          page.evaluate(choiceHighlight, { rect }),
          page.mouse.move(rect.x + rect.width * 0.8, rect.y + rect.height / 2),
        ]);
        const hover = frames.at(-1)!;
        assert.ok(hover.green > rest!.green + 4, id + " shows a hover tint");
        assert.ok(
          frames.every((f) => f.green <= hover.green + 1),
          id + " goes directly from the surface to the blue hover tint",
        );
        await control(id, false);
        return { rect, hover };
      };
      const selectRow = async (id: string) => {
        const { rect, hover } = await hoverChoice(id);
        await page.mouse.down();
        const [frames] = await Promise.all([
          page.evaluate(choiceHighlight, { rect }),
          page.mouse.up(),
        ]);
        await control(id, true);
        const selected = frames.at(-1)!;
        assert.equal(selected.green, 219);
        assert.ok(
          selected.height >= hover.height + 5 &&
            frames.some(
              (f) => f.height > hover.height + 1 && f.height < selected.height,
            ),
          id + " grows through intermediate sizes into its selected outline",
        );
      };
      await control("difficulty/normal", true);
      // Compare the outside silhouette; hover and open legitimately change the fill.
      const picker = await control("section", false);
      const pickerCorner = () =>
        page.evaluate(async (rect) => {
          const frame = await window.ippDemoCanvas!.capture();
          const pixels = new Uint8Array(frame.pixels);
          const stride = frame.view.binding.viewport.width;
          const left = Math.round(rect.x),
            top = Math.round(rect.y);
          const rgb = (x: number, y: number) =>
            pixels.subarray(4 * (y * stride + x), 4 * (y * stride + x) + 3);
          return Array.from({ length: 10 }, (_, dy) => {
            const bg = rgb(left - 2, top + dy);
            for (let dx = 0; dx < 18; dx++) {
              if (
                rgb(left + dx, top + dy).some(
                  (v, i) => Math.abs(v - bg[i]!) > 10,
                )
              )
                return dx;
            }
            return 18;
          });
        }, picker);
      const closedCorner = await pickerCorner();
      assert.ok(
        closedCorner[0]! > closedCorner[7]! + 2,
        "collapsed picker has rounded corners",
      );
      await press("section");
      await control("section", true);
      await page.mouse.move(1, 1);
      await wait("open picker keeps its outer corner", async () =>
        (await pickerCorner()).every(
          (x, i) => Math.abs(x - closedCorner[i]!) <= 1,
        ),
      );
      await page.keyboard.press("Escape");
      await control("section", false);
      // Inspect the completed frame just outside the native rail, away from text.
      await wait("selected tab overhang", async () =>
        page.evaluate(
          async (rect) => {
            const frame = await window.ippDemoCanvas!.capture();
            const pixels = new Uint8Array(frame.pixels);
            const width = frame.view.binding.viewport.width;
            const green = (x: number, y: number) => {
              const i = 4 * (Math.round(y) * width + Math.round(x));
              return (
                pixels[i + 1]! > pixels[i]! + 38 &&
                pixels[i + 1]! > pixels[i + 2]! + 30
              );
            };
            const x = rect.x + rect.width / 2;
            return (
              green(x, rect.y - 1) &&
              green(x, rect.y + rect.height + 1) &&
              !green(rect.x - 12, rect.y - 2)
            );
          },
          await control("difficulty/normal", true),
        ),
      );
      const normal = await control("difficulty/normal", true);
      await page.mouse.move(
        normal.x + normal.width / 2,
        normal.y + normal.height / 2,
      );
      assert.equal(
        (await control("difficulty/normal", true)).variant,
        normal.variant,
      );
      await hoverChoice("difficulty/easy");
      await press("difficulty/easy");
      await control("difficulty/easy", true);
      await page.keyboard.press("ArrowRight");
      assert.notEqual(
        (await control("difficulty/normal", true)).variant,
        normal.variant,
      );
      await page.screenshot({
        path: join(evidence.directory, "cartoon-settings.png"),
      });
      for (const [section, specimen] of [
        ["actions", "runner-name"],
        ["choices", "radio-mode"],
        ["values", "lap-count"],
        ["colour", "trail-color"],
        ["navigation", "route-tree"],
        ["inventory", "supplies"],
        ["feedback", "progress-ring"],
        ["overlays", "rename-trigger"],
      ] as const) {
        await press("next");
        await page.waitForURL((url) => url.hash === "#" + section);
        await wait(specimen, () =>
          page.evaluate(
            async (id) =>
              (await window.ippDemoCanvas!.client.inspect()).entities.some(
                (e) => e.metadata.symbolicId === id,
              ),
            specimen,
          ),
        );
        if (section === "choices") {
          const radio = await control("radio-mode/easy/mark", false);
          await page.mouse.move(
            radio.x + radio.width / 2,
            radio.y + radio.height / 2,
          );
          await page.mouse.down();
          // Sample completed frames across release, when the old transition briefly
          // filled the square hit target behind the circular mark.
          const corners = page.evaluate(async (rect) => {
            for (let i = 0; i < 16; i++) {
              const frame = await window.ippDemoCanvas!.capture();
              const pixels = new Uint8Array(frame.pixels);
              const width = frame.view.binding.viewport.width;
              const rgb = (x: number, y: number) => {
                const offset = 4 * (Math.round(y) * width + Math.round(x));
                return (
                  (pixels[offset]! << 16) |
                  (pixels[offset + 1]! << 8) |
                  pixels[offset + 2]!
                );
              };
              const surface = rgb(rect.x - 4, rect.y - 4);
              for (const x of [rect.x + 1, rect.x + rect.width - 2])
                for (const y of [rect.y + 1, rect.y + rect.height - 2])
                  if (rgb(x, y) !== surface) return false;
              await new Promise<void>((resolve) =>
                requestAnimationFrame(() => resolve()),
              );
            }
            return true;
          }, radio);
          await page.mouse.up();
          await control("radio-mode/easy/mark", true);
          assert.ok(await corners, "radio corners stay clear through release");
          await selectRow("route-list/Pine forest");
          await press("gear-select");
          await hoverChoice("gear-select/options/Pine forest");
          const id = "gear-select/options/Meadow";
          for (const selected of [false, true]) {
            const row = await control(id, !selected);
            await page.mouse.move(
              row.x + row.width * 0.8,
              row.y + row.height / 2,
            );
            await page.mouse.down();
            const [samples] = await Promise.all([
              page.evaluate(choiceHighlight, { rect: row, checkCorners: true }),
              page.mouse.up(),
            ]);
            await control(id, selected);
            assert.ok(
              samples.some(({ green }) => green > 110 && green < 210),
              "dropdown option paints an intermediate selection colour",
            );
            assert.ok(
              selected
                ? samples.at(-1)!.green === 219
                : samples.at(-1)!.green < 110,
              "option settles into its selected or quiet hovered colour",
            );
          }
          await page.keyboard.press("Escape");
        }
        if (section === "values") {
          const rail = await control("range/slider");
          const edge = rail.height * 0.75;
          const x = (fraction: number) =>
            rail.x + edge / 2 + fraction * (rail.width - edge);
          const y = rail.y + rail.height / 2;
          for (const [from, to, id, expected] of [
            [0.2, 0.4, 0, "40 m"],
            [0.75, 0.6, 1, "60 m"],
          ] as const) {
            await page.mouse.move(x(from), y);
            await page.mouse.down();
            const [visible] = await Promise.all([
              page.evaluate(rangeLabelsStayVisible, rail),
              page.mouse.move(x(to), y, { steps: 24 }),
            ]);
            await page.mouse.up();
            assert.ok(visible, "both readouts remain painted during the drag");
            await wait(expected, () =>
              page.evaluate(
                async ({ id, expected }) => {
                  const { client } = window.ippDemoCanvas!;
                  return (
                    (await client.inspect()).entities
                      .find(
                        (e) =>
                          e.metadata.symbolicId === `range/readout/${id}/text`,
                      )
                      ?.components.find(
                        (c) => c.component === client.components.CanvasText!.id,
                      )?.fields.text === expected
                  );
                },
                { id, expected },
              ),
            );
          }
        }
        if (section === "navigation") {
          const initial = await control("page-navigation");
          assert.equal(initial.bar, 0);
          for (const step of ["walk", "arrive", "pack"]) {
            const badge = await control("camp-steps/step/" + step);
            await page.mouse.move(
              badge.x + badge.width / 2,
              badge.y + badge.height / 2,
            );
            await page.mouse.down();
            const [clear] = await Promise.all([
              page.evaluate(stepCornersStayClear, badge),
              page.mouse.up(),
            ]);
            assert.ok(clear, `${step} keeps its rounded silhouette on release`);
            await control("camp-steps/step/" + step, true);
            if (step === "arrive") await control("check-in");
            const panel = await control("page-navigation");
            assert.equal(panel.bar, 0, step);
            assert.equal(panel.height, initial.height, step);
          }
          const bridge = await control("route-tree/row/bridge", true);
          const treeHeader = await control("world-map/header");
          await wait("grass-green selection and one tree frame", () =>
            page.evaluate(
              async ({ rect, edge }) => {
                const frame = await window.ippDemoCanvas!.capture();
                const pixels = new Uint8Array(frame.pixels);
                const width = frame.view.binding.viewport.width;
                const rgb = (x: number, y: number) => {
                  const i = 4 * (Math.round(y) * width + Math.round(x));
                  return [...pixels.subarray(i, i + 3)].join(",");
                };
                // Check the selected face, then scan just above it where a
                // nested tree frame would draw a second vertical ink line.
                const y = rect.y + rect.height / 2;
                if (rgb(rect.x + rect.width * 0.8, y) !== "128,219,133")
                  return false;
                for (let dx = 4; dx <= 14; dx++)
                  if (rgb(edge + dx, rect.y - 4) !== "36,52,87") return false;
                return true;
              },
              { rect: bridge, edge: treeHeader.x },
            ),
          );
          await page.screenshot({
            path: join(evidence.directory, "cartoon-navigation.png"),
          });
          await selectRow("route-tree/row/camp");
          await control("route-tree/row/bridge", false);
          const map = await control("journal-tabs/tab/map", true);
          for (const tab of ["map", "runner", "gear", "map"]) {
            const id = "journal-tabs/tab/" + tab;
            await press(id);
            await page.mouse.move(1, 1);
            const bounds = await control(id, true);
            await wait(tab + " has a complete ink outline", () =>
              page.evaluate(journalOutline, bounds),
            );
          }
          assert.notEqual(
            (await control("journal-tabs/tab/map", true)).variant,
            map.variant,
          );
        }
        if (section === "inventory") {
          assert.equal(
            await page.evaluate(async () =>
              (await window.ippDemoCanvas!.client.inspect()).entities.some(
                ({ metadata }) =>
                  /^supplies\/(?:bottom-line|header\/line|row\/[^/]+\/line)$/.test(
                    metadata.symbolicId ?? "",
                  ),
              ),
            ),
            false,
            "Cartoon grid header and rows share an unruled surface",
          );
          await press("supplies/row/1");
          await control("supplies/row/1", true);
          const wheel = async (id: string, delta: number, expected: number) => {
            const rect = await control(id);
            await page.mouse.move(
              rect.x + rect.width / 2,
              rect.y + rect.height / 2,
            );
            await page.mouse.wheel(0, delta);
            await wait(
              `${id} scrolls to ${expected}`,
              async () => (await control(id)).offsetY === expected,
            );
          };
          await wheel("supplies/body", 100, 40);
          await wheel("supplies/body", -25, 30);
          await wheel("supplies/body", -75, 0);
          const log = await control("log-scroll");
          const header = await control("trail-log/header");
          assert.ok(
            Math.abs(header.x + header.width - log.x - log.width) < 1,
            "the log viewport extends to the shared frame's right edge",
          );
          await wheel("log-scroll", 100, 40);
          await wait("clear lower log inset", () =>
            page.evaluate(async (rect) => {
              const frame = await window.ippDemoCanvas!.capture();
              const w = frame.view.binding.viewport.width;
              const p = new Uint8Array(frame.pixels);
              for (
                let y = Math.ceil(rect.y + rect.height) + 2;
                y < rect.y + rect.height + 10;
                y++
              )
                for (
                  let x = Math.ceil(rect.x) + 4;
                  x < rect.x + rect.width / 2;
                  x++
                ) {
                  const i = (y * w + x) * 4;
                  if (p[i]! > 150 && p[i + 1]! > 150) return false;
                }
              return true;
            }, log),
          );
          await page.screenshot({
            path: join(evidence.directory, "cartoon-inventory.png"),
          });
        }
        if (section === "feedback") {
          await press("completion/slider");
          await page.keyboard.press("End");
          await wait("completed progress meter", () =>
            page.evaluate(async () =>
              (await window.ippDemoCanvas!.client.inspect()).entities.some(
                (e) => e.metadata.symbolicId === "progress-ring/symbol",
              ),
            ),
          );
          await page.screenshot({
            path: join(evidence.directory, "cartoon-progress-complete.png"),
          });
          await press("show-toast");
          const toast = await control("trail-toasts/saved-1");
          const dismiss = await control("trail-warning/action");
          await wait("drawn information and warning symbols", () =>
            page.evaluate(
              async ({ toast, dismiss }) => {
                const canvas = window.ippDemoCanvas!;
                const { client } = canvas;
                const { entities } = await client.inspect();
                const frame = await canvas.capture();
                const pixels = new Uint8Array(frame.pixels);
                const width = frame.view.binding.viewport.width;
                for (const [id, left, right, midY, rgb] of [
                  [
                    "trail-toasts/saved-1/mark",
                    toast.x,
                    toast.x + 48,
                    toast.y + toast.height / 2,
                    0xb1ebcb,
                  ],
                  [
                    "trail-warning/icon",
                    0,
                    dismiss.x,
                    dismiss.y + dismiss.height / 2,
                    0xffca45,
                  ],
                ] as const) {
                  const entity = entities.find(
                    (e) => e.metadata.symbolicId === id,
                  );
                  if (
                    !entity?.components.some(
                      (c) => c.component === client.components.GuiSkin!.id,
                    ) ||
                    entity.components.some(
                      (c) => c.component === client.components.CanvasText!.id,
                    )
                  )
                    return false;
                  let fill = 0;
                  for (let y = Math.round(midY) - 11; y <= midY + 11; y++)
                    for (let x = Math.ceil(left); x < right; x++) {
                      const i = (y * width + x) * 4;
                      if (
                        ((pixels[i]! << 16) |
                          (pixels[i + 1]! << 8) |
                          pixels[i + 2]!) ===
                        rgb
                      )
                        fill++;
                    }
                  if (fill < 12) return false;
                }
                return true;
              },
              { toast, dismiss },
            ),
          );
          await page.screenshot({
            path: join(evidence.directory, "cartoon-notifications.png"),
          });
          assert.ok(
            Math.abs(toast.rotation) > 0,
            "the toast root owns the tilt",
          );
          const close = await control("trail-toasts/saved-1/close");
          // The extreme corner of a rotated rectangle's AABB is outside its
          // hit region. Pick close enough to the corner to distinguish the two.
          const right = close.x + close.width,
            top = close.y;
          await page.mouse.click(
            toast.rotation > 0 ? right - 0.01 : close.x + 0.01,
            top + 0.01,
          );
          await control("trail-toasts/saved-1/close");
          for (const id of [
            "trail-toasts/saved-1/close",
            "trail-warning/action",
          ]) {
            await press(id);
            await wait(`dismissed ${id}`, () =>
              page.evaluate(async (id) => {
                const { entities } =
                  await window.ippDemoCanvas!.client.inspect();
                return !entities.some(
                  (entity) => entity.metadata.symbolicId === id,
                );
              }, id),
            );
          }
          await press("show-alert");
          await control("trail-warning/action");
          await press("show-toast");
          await control("trail-toasts/saved-2");
        }
      }
      await press("commands");
      await press("command-list/mark");
      await wait("menu command", () =>
        page.evaluate(async () =>
          (await window.ippDemoCanvas!.client.inspect()).entities.some((e) =>
            e.components.some((c) => c.fields.text === "Waypoint marked"),
          ),
        ),
      );
      assert.deepEqual(
        await page.locator('output[role="alert"]').allTextContents(),
        [],
      );
      assert.deepEqual(errors, []);
    },
  );
});
