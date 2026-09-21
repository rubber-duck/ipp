import assert from "node:assert/strict";
import { join } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  openGallery,
  galleryEnvironment,
  transform,
  selected,
} from "./gallery-driver.js";

interface GalleryLayout {
  readonly viewport: { readonly width: number; readonly height: number };
  readonly pixelRatio: number;
  readonly scrollWidth: number;
  readonly frame: Rect;
  readonly canvas: Rect;
  readonly controlsToggle: Rect;
  readonly controls: Rect;
  readonly controlsOpen: boolean;
  readonly controlsModal: boolean;
  readonly controlsScrollable: boolean;
  readonly controlsBackground: string;
  readonly backdropBackground: string;
}

interface Rect {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
  readonly width: number;
  readonly height: number;
}

function assertCanvasFillsFrame(layout: GalleryLayout) {
  assert.ok(
    layout.scrollWidth <= layout.viewport.width + 1,
    `page overflows horizontally: ${JSON.stringify(layout)}`,
  );
  assert.ok(Math.abs(layout.canvas.left - layout.frame.left) <= 2);
  assert.ok(Math.abs(layout.canvas.top - layout.frame.top) <= 2);
  assert.ok(Math.abs(layout.canvas.width - layout.frame.width) <= 4);
  assert.ok(Math.abs(layout.canvas.height - layout.frame.height) <= 4);
}

function assertRenderedFrame(
  capture: Awaited<
    ReturnType<Awaited<ReturnType<typeof openGallery>>["capture"]>
  >,
  layout: GalleryLayout,
) {
  assert.ok(capture.summary.foregroundPixels > 1_000);
  assert.ok(
    Math.abs(
      capture.frame.width - Math.round(layout.canvas.width * layout.pixelRatio),
    ) <= 1,
  );
  assert.ok(
    Math.abs(
      capture.frame.height -
        Math.round(layout.canvas.height * layout.pixelRatio),
    ) <= 1,
  );
}

test("geometry and combined scenes orbit and zoom without pan and discard late camera gestures", {
  timeout: 90_000,
}, async (context) => {
  await runBrowserEnvironment(
    "two-scene camera gestures",
    { ...galleryEnvironment, deviceScaleFactor: 2 },
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(bounds);
      const point = (x: number, y: number) =>
        [bounds.x + x * bounds.width, bounds.y + y * bounds.height] as const;
      const initial = await g.capture("geometry-initial");
      await g.drag(point(0.45, 0.4), point(0.6, 0.5));
      const orbit = await g.capture("geometry-orbit");
      assert.notDeepEqual(
        transform(orbit.inspection),
        transform(initial.inspection),
      );
      assert.ok(
        (await g.difference("geometry-initial", "geometry-orbit"))
          .changedPixels > 1000,
      );
      await g.page.mouse.move(...point(0.5, 0.5));
      await g.page.mouse.wheel(0, 180);
      const zoom = await g.capture("geometry-zoom");
      assert.notDeepEqual(
        transform(zoom.inspection),
        transform(orbit.inspection),
      );
      await g.drag(point(0.5, 0.5), point(0.65, 0.6), "middle");
      const noPan = await g.capture("geometry-no-pan");
      assert.deepEqual(transform(noPan.inspection), transform(zoom.inspection));
      await g.page.locator("#reset-camera").click();
      assert.deepEqual(
        transform((await g.capture("geometry-reset")).inspection),
        transform(initial.inspection),
      );

      await g.navigate("lighting");
      await g.seek(0);
      const combined = await g.capture("combined-initial");
      await g.click("lighting-cube");
      const hit = await g.capture("hit-is-selection");
      assert.deepEqual(
        transform(hit.inspection),
        transform(combined.inspection),
      );
      assert.deepEqual(selected(hit.inspection), ["lighting-cube"]);
      const currentBounds = await g.page
        .locator("#ipp-world-canvas")
        .boundingBox();
      assert.ok(currentBounds);
      const empty = [
        currentBounds.x + currentBounds.width * 0.04,
        currentBounds.y + currentBounds.height * 0.06,
      ] as const;
      const end = [empty[0] + 70, empty[1] + 35] as const;
      await g.call("delayNextCameraQuery");
      try {
        await g.drag(empty, end);
        await g.held();
        assert.deepEqual(
          transform(await g.inspect()),
          transform(combined.inspection),
        );
        await g.call("releaseQuery");
        const buffered = await g.capture("buffered-miss-orbit");
        assert.notDeepEqual(
          transform(buffered.inspection),
          transform(combined.inspection),
        );
        assert.deepEqual(selected(buffered.inspection), ["lighting-cube"]);
        await g.drag(empty, end, "middle");
        assert.deepEqual(
          transform((await g.capture("combined-no-pan")).inspection),
          transform(buffered.inspection),
        );

        // The real reply arrives only after a blur cancels the captured gesture.
        await g.call("delayNextCameraQuery");
        await g.drag(empty, end);
        await g.held();
        await g.page.evaluate(() => window.dispatchEvent(new Event("blur")));
        await g.call("releaseQuery");
        assert.deepEqual(
          transform((await g.capture("canceled-orbit")).inspection),
          transform(buffered.inspection),
        );

        await g.call("delayNextCameraQuery");
        await g.drag(empty, end);
        await g.held();
        await g.navigate("shapes");
        const returned = await g.capture("geometry-before-late-pick");
        await g.call("releaseQuery");
        const after = await g.capture("geometry-after-late-pick");
        assert.deepEqual(
          transform(after.inspection),
          transform(returned.inspection),
        );
        assert.equal(after.frame.drawCalls, 12);
      } finally {
        await g.call("releaseQuery");
      }
      assert.deepEqual(g.errors, []);
    },
  );
});

test("gallery fills desktop and phone viewports while picker and controls preserve the live session", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "responsive gallery shell",
    { ...galleryEnvironment, hasTouch: true },
    context.signal,
    async (scenario) => {
      const { page } = scenario;
      const screenshot = (label: string) =>
        page.screenshot({
          path: join(scenario.evidence.directory, `${label}-page.png`),
          fullPage: true,
        });
      const readLayout = () =>
        page.evaluate(() => {
          const rect = (selector: string) => {
            const bounds = document
              .querySelector<HTMLElement>(selector)!
              .getBoundingClientRect();
            return {
              left: bounds.left,
              top: bounds.top,
              right: bounds.right,
              bottom: bounds.bottom,
              width: bounds.width,
              height: bounds.height,
            };
          };
          const controls =
            document.querySelector<HTMLDialogElement>("#world-controls")!;
          const controlsBody = controls.querySelector<HTMLElement>(
            ".control-panel-body",
          )!;
          const scrollable = [controls, controlsBody].some((element) => {
            const overflow = getComputedStyle(element).overflowY;
            return (
              (overflow === "auto" || overflow === "scroll") &&
              element.scrollHeight > element.clientHeight + 1
            );
          });
          return {
            viewport: { width: innerWidth, height: innerHeight },
            pixelRatio: devicePixelRatio,
            scrollWidth: document.documentElement.scrollWidth,
            frame: rect(".canvas-frame"),
            canvas: rect("#ipp-world-canvas"),
            controlsToggle: rect("#controls-toggle"),
            controls: rect("#world-controls"),
            controlsOpen: controls.open,
            controlsModal: controls.matches(":modal"),
            controlsScrollable: scrollable,
            controlsBackground: getComputedStyle(controls).backgroundColor,
            backdropBackground: getComputedStyle(controls, "::backdrop")
              .backgroundColor,
          } satisfies GalleryLayout;
        });
      const tap = async (selector: string) => {
        const bounds = await page.locator(selector).boundingBox();
        assert.ok(bounds, `${selector} has no touch target`);
        assert.ok(
          bounds.width >= 44 && bounds.height >= 44,
          `${selector} touch target is ${bounds.width}x${bounds.height}`,
        );
        await page.touchscreen.tap(
          bounds.x + bounds.width / 2,
          bounds.y + bounds.height / 2,
        );
      };
      const waitForControls = (open: boolean, modal?: boolean) =>
        page.waitForFunction(
          ({ expectedOpen, expectedModal }) => {
            const controls =
              document.querySelector<HTMLDialogElement>("#world-controls");
            return (
              controls?.open === expectedOpen &&
              (expectedModal === undefined ||
                controls.matches(":modal") === expectedModal)
            );
          },
          { expectedOpen: open, expectedModal: modal },
        );
      const waitForFocus = (id: string) =>
        page.waitForFunction(
          (expected) => document.activeElement?.id === expected,
          id,
        );
      const readSession = () =>
        page.evaluate(() => window.ippWorldCanvas!.client.session.toString());

      await page.setViewportSize({ width: 1280, height: 800 });
      const g = await openGallery(scenario);
      const desktopLayout = await readLayout();
      assertCanvasFillsFrame(desktopLayout);
      assert.equal(desktopLayout.controlsOpen, true);
      assert.equal(desktopLayout.controlsModal, false);
      assert.ok(desktopLayout.frame.height >= 700);
      assert.ok(desktopLayout.controls.left >= desktopLayout.frame.right - 2);
      const desktop = await g.capture("responsive-desktop");
      assertRenderedFrame(desktop, desktopLayout);
      await screenshot("responsive-desktop");

      await page.setViewportSize({ width: 390, height: 844 });
      await waitForControls(false);
      const portraitLayout = await readLayout();
      assertCanvasFillsFrame(portraitLayout);
      assert.ok(
        portraitLayout.frame.height >= portraitLayout.viewport.height * 0.75,
      );
      assert.ok(portraitLayout.frame.left <= 12);
      assert.ok(390 - portraitLayout.frame.right <= 12);
      assert.ok(
        portraitLayout.controlsToggle.top >= portraitLayout.frame.bottom - 1,
      );
      assert.ok(
        portraitLayout.controlsToggle.top - portraitLayout.frame.bottom <= 16,
      );
      assert.ok(844 - portraitLayout.controlsToggle.bottom <= 12);
      const portrait = await g.capture("responsive-phone-portrait");
      assert.equal(portrait.session, desktop.session);
      assertRenderedFrame(portrait, portraitLayout);
      await screenshot("responsive-phone-closed");

      await tap("#scene-picker-trigger");
      await page.waitForFunction(
        () =>
          document.querySelector<HTMLDialogElement>("#scene-picker")?.open &&
          document.activeElement?.id === "scene-search",
      );
      assert.equal(
        await page
          .locator("#scene-picker-trigger")
          .getAttribute("aria-expanded"),
        "true",
      );
      assert.equal(
        await page.locator("#world-shapes").getAttribute("aria-current"),
        "page",
      );
      await page.locator("#scene-search").fill("no matching world");
      assert.match(
        await page.locator(".empty-search").innerText(),
        /No scenes match/,
      );
      assert.equal(await page.locator(".scene-option").count(), 0);
      await page.locator("#scene-search").fill("animated lights");
      assert.deepEqual(
        await page
          .locator(".scene-option")
          .evaluateAll((options) => options.map((option) => option.id)),
        ["world-lighting"],
      );
      await screenshot("responsive-phone-picker");
      await page.locator("#world-lighting").focus();
      await page.keyboard.press("Enter");
      await page.waitForFunction(
        () =>
          document.querySelector<HTMLElement>(".viewer-shell")?.dataset.page ===
            "lighting" &&
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      await waitForFocus("scene-picker-trigger");
      assert.equal(new URL(page.url()).hash, "#lighting");
      assert.equal(await readSession(), desktop.session.toString());
      await page.keyboard.press("Enter");
      await waitForFocus("scene-search");
      await page.keyboard.press("Escape");
      await page.waitForFunction(
        () =>
          !document.querySelector<HTMLDialogElement>("#scene-picker")?.open &&
          document
            .querySelector<HTMLButtonElement>("#scene-picker-trigger")
            ?.getAttribute("aria-expanded") === "false" &&
          document.activeElement?.id === "scene-picker-trigger",
      );
      await g.navigate("shapes");
      assert.equal(await readSession(), desktop.session.toString());

      const closedFrame = await page.locator(".canvas-frame").boundingBox();
      assert.ok(closedFrame);
      await tap("#controls-toggle");
      await waitForControls(true, true);
      await page.locator("#reset-camera").focus();
      await page.keyboard.press("Enter");
      await g.inspect();
      await page.waitForSelector('#status[data-state="ready"]');
      await waitForControls(true, true);
      const openLayout = await readLayout();
      assertCanvasFillsFrame(openLayout);
      assert.deepEqual(
        await page.locator(".canvas-frame").boundingBox(),
        closedFrame,
      );
      assert.ok(openLayout.controls.top < openLayout.frame.bottom);
      assert.ok(openLayout.controls.bottom >= openLayout.viewport.height - 2);
      assert.equal(openLayout.controlsScrollable, true);
      assert.match(openLayout.controlsBackground, /rgba\(.+, 0?\.[0-9]+\)/);
      assert.match(openLayout.backdropBackground, /rgba\(.+, 0?\.[0-9]+\)/);
      await page.locator("#mesh-select").selectOption("cube");
      await screenshot("responsive-phone-controls-open");
      assert.equal(await readSession(), desktop.session.toString());

      await page.keyboard.press("Escape");
      await waitForControls(false);
      await waitForFocus("controls-toggle");
      await tap("#controls-toggle");
      await waitForControls(true, true);
      await page.locator("#controls-close").click();
      await waitForControls(false);
      await waitForFocus("controls-toggle");
      await tap("#controls-toggle");
      await waitForControls(true, true);
      const sheet = await page.locator("#world-controls").boundingBox();
      assert.ok(sheet);
      await page.touchscreen.tap(4, Math.max(4, sheet.y - 4));
      await waitForControls(false);
      await waitForFocus("controls-toggle");

      await tap("#controls-toggle");
      await waitForControls(true, true);
      await page.setViewportSize({ width: 844, height: 390 });
      await waitForControls(true, true);
      const landscapeLayout = await readLayout();
      assertCanvasFillsFrame(landscapeLayout);
      assert.ok(
        landscapeLayout.frame.height >= landscapeLayout.viewport.height * 0.6,
      );
      assert.ok(
        landscapeLayout.controlsToggle.top >= landscapeLayout.frame.bottom - 1,
      );
      assert.equal(await page.locator("#mesh-select").inputValue(), "cube");
      const landscape = await g.capture("responsive-phone-landscape");
      assert.equal(landscape.session, desktop.session);
      assertRenderedFrame(landscape, landscapeLayout);

      await page.keyboard.press("Escape");
      await waitForControls(false);
      await tap("#scene-picker-trigger");
      await page.waitForFunction(
        () => document.querySelector<HTMLDialogElement>("#scene-picker")?.open,
      );
      const lastScene = page.locator("#world-gui");
      await lastScene.scrollIntoViewIfNeeded();
      const [pickerBounds, lastSceneBounds] = await Promise.all([
        page.locator("#scene-picker").boundingBox(),
        lastScene.boundingBox(),
      ]);
      assert.ok(pickerBounds);
      assert.ok(lastSceneBounds);
      assert.ok(lastSceneBounds.y >= pickerBounds.y);
      assert.ok(
        lastSceneBounds.y + lastSceneBounds.height <=
          pickerBounds.y + pickerBounds.height,
      );
      assert.ok((await readLayout()).scrollWidth <= 845);
      await lastScene.click();
      await page.waitForFunction(
        () =>
          document.querySelector<HTMLElement>(".viewer-shell")?.dataset.page ===
            "gui" &&
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      assert.equal(await readSession(), desktop.session.toString());
      await g.navigate("shapes");
      await tap("#controls-toggle");
      await waitForControls(true, true);

      await page.setViewportSize({ width: 1280, height: 800 });
      await waitForControls(true, false);
      const returnedDesktopLayout = await readLayout();
      assertCanvasFillsFrame(returnedDesktopLayout);
      assert.equal(await page.locator("#mesh-select").inputValue(), "cube");
      await page.locator("#mesh-select").selectOption("sphere");
      assert.equal(await page.locator("#mesh-select").inputValue(), "sphere");
      const returnedDesktop = await g.capture("responsive-desktop-returned");
      assert.equal(returnedDesktop.session, desktop.session);
      assertRenderedFrame(returnedDesktop, returnedDesktopLayout);
      await screenshot("responsive-desktop-returned");
      assert.deepEqual(g.errors, []);
    },
  );
});
