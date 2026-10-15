import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../../harness/browser.js";
import { presentingBuild } from "../../harness/hosts.js";
import { encodePng } from "../../harness/images.js";
import type {} from "./optical-material/preview.js";

test("glass preview follows browser size and pixel density", {
  timeout: 240_000,
}, async (context) => {
  await runBrowserEnvironment(
    "glass-preview-dpi",
    {
      workspace: resolve(process.cwd()),
      build: presentingBuild(false, {
        directory: "target/browser-build/render",
      }),
      deviceScaleFactor: 2,
      operationTimeoutMs: 180_000,
    },
    context.signal,
    async (environment) => {
      const { page, evidence } = environment;
      await page.setViewportSize({ width: 1280, height: 1100 });
      await page.emulateMedia({ reducedMotion: "reduce" });
      await page.goto(
        `${environment.urls.origin}/target/gui-glass-skin/optical-preview.html?skin=production`,
      );
      await page.waitForFunction(() => window.glassPreviewReady);
      const identity = await page.evaluate(() =>
        String(
          window.glassPreview.session.controls.get("checkbox")!.target.entity,
        ),
      );
      const capture = async (name: string, ratio: number) => {
        await page.waitForFunction((ratio) => {
          const box = document.querySelector("canvas")!.getBoundingClientRect();
          const actual = window.glassPreview.session.binding.viewport;
          return (
            actual.devicePixelRatio === ratio &&
            actual.width === Math.round(box.width * ratio) &&
            actual.height === Math.round(box.height * ratio)
          );
        }, ratio);
        const frame = await page.evaluate(async () => {
          const result = await window.glassPreview.capture();
          const canvas = document.querySelector("canvas")!;
          const box = canvas.getBoundingClientRect();
          const bytes = new Uint8Array(result.pixels);
          let binary = "";
          for (let offset = 0; offset < bytes.length; offset += 0x8000)
            binary += String.fromCharCode(
              ...bytes.subarray(offset, offset + 0x8000),
            );
          return {
            ...result.view.binding.viewport,
            failedDrawCalls: result.failedDrawCalls,
            rgba: btoa(binary),
            css: { width: box.width, height: box.height },
            backing: { width: canvas.width, height: canvas.height },
          };
        });
        assert.equal(frame.failedDrawCalls, 0);
        assert.equal(frame.width, Math.round(frame.css.width * ratio));
        assert.equal(frame.height, Math.round(frame.css.height * ratio));
        assert.deepEqual(frame.backing, {
          width: frame.width,
          height: frame.height,
        });
        const pixels = new Uint8Array(Buffer.from(frame.rgba, "base64"));
        await writeFile(
          resolve(evidence.directory, `${name}.png`),
          encodePng({ ...frame, pixels }),
        );
        // Independently authored title/checkbox regions must contain visible
        // ink and polished glass, not an empty correctly sized framebuffer.
        for (const [label, x, y, width, height] of [
          ["title", 64, 45, 900, 60],
          ["checkbox", 316, 488, 32, 32],
        ] as const) {
          let lit = 0;
          const sx = frame.width / 1536;
          const sy = frame.height / 1024;
          const outside =
            (Math.round((y - 8) * sy) * frame.width +
              Math.round((x - 8) * sx)) *
            4;
          for (let py = Math.ceil(y * sy); py < (y + height) * sy; py++)
            for (let px = Math.ceil(x * sx); px < (x + width) * sx; px++) {
              const offset = (py * frame.width + px) * 4;
              // A quiet unchecked glass edge is visible by contrast, not by
              // glowing above an absolute brightness threshold.
              const visible =
                label === "checkbox"
                  ? [0, 1, 2].some(
                      (channel) =>
                        Math.abs(
                          pixels[offset + channel]! -
                            pixels[outside + channel]!,
                        ) > 8,
                    )
                  : pixels[offset + 1]! > 100 && pixels[offset + 2]! > 100;
              if (visible) lit++;
            }
          assert.ok(
            lit > width * height * sx * sy * 0.02,
            `${name}: visible ${label}`,
          );
        }
        await evidence.record("preview-dpi", {
          name,
          ...frame,
          rgba: undefined,
        });
      };
      const toggle = async (checked: boolean) => {
        const box = await page.locator("canvas").boundingBox();
        assert.ok(box);
        await page.mouse.click(
          box.x + (box.width * 332) / 1536,
          box.y + (box.height * 504) / 1024,
        );
        await page.waitForFunction(
          async (checked) =>
            (await window.glassPreview.session.controls.get("checkbox")!.read())
              .checked === checked,
          checked,
        );
        assert.equal(
          await page.evaluate(() =>
            String(
              window.glassPreview.session.controls.get("checkbox")!.target
                .entity,
            ),
          ),
          identity,
        );
        await page.mouse.move(5, 5);
      };
      const emulation = await page.context().newCDPSession(page);
      try {
        await capture("retina-initial", 2);
        // Density-only changes emulate zoom or moving to another monitor.
        await emulation.send("Emulation.setDeviceMetricsOverride", {
          width: 1280,
          height: 1100,
          deviceScaleFactor: 1,
          mobile: false,
        });
        // CDP changes the real DPR but omits the window notification delivered
        // by browser zoom. Supply that event without changing the CSS box.
        await page.evaluate(() => window.dispatchEvent(new Event("resize")));
        await capture("standard-density", 1);
        await emulation.send("Emulation.setDeviceMetricsOverride", {
          width: 1280,
          height: 1100,
          deviceScaleFactor: 1.25,
          mobile: false,
        });
        await page.evaluate(() => window.dispatchEvent(new Event("resize")));
        await capture("fractional-density", 1.25);
        await emulation.send("Emulation.setDeviceMetricsOverride", {
          width: 960,
          height: 1000,
          deviceScaleFactor: 2,
          mobile: false,
        });
        await capture("retina-resized", 2);
        await toggle(false);
        await capture("retina-checkbox-off", 2);
        // Element-only resize exercises ResizeObserver without a window event.
        await page.evaluate(() => {
          document.body.style.maxWidth = "720px";
        });
        await capture("retina-element-resized", 2);
        await toggle(true);
      } finally {
        await page.evaluate(() => window.glassPreview.close());
        await emulation.detach();
      }
    },
  );
});
