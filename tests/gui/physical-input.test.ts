import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { runBrowserEnvironment } from "../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../harness/hosts.js";
import { encodePng } from "../harness/images.js";
import type { preparePhysicalInput } from "./scenarios/physical-input.js";

type State = typeof globalThis & {
  physical: Awaited<ReturnType<typeof preparePhysicalInput>>;
  blockers: Awaited<
    ReturnType<Awaited<ReturnType<typeof preparePhysicalInput>>["blockers"]>
  >;
  closePhysicalHost(): Promise<void>;
  /** DOM pointer types that pressed the Canvas, oldest first. */
  pointerTypes: string[];
};

type Point = readonly [number, number];

for (const native of [false, true]) {
  test(`ordinary composed physical input through ${native ? "native WebSocket/GLES" : "worker WASM/WebGL"}`, {
    timeout: 120_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const build = presentingBuild(native, {
      directory: native ? "target/gles-host" : "target/browser-build/render",
    });
    async function browser(native?: { url: string; presentationUrl: string }) {
      const result = await runBrowserEnvironment(
        native ? "physical-input-gles" : "physical-input-webgl",
        {
          workspace,
          build,
          operationTimeoutMs: 90_000,
          rendering: !native,
        },
        context.signal,
        async (environment) =>
          environment.execute(
            "ordinary-composed-physical-input",
            {},
            async () => {
              const page = environment.page;
              await page.evaluate(
                async ({ urls, native }) => {
                  const contract = await import(urls.generated);
                  const scenario = await import(
                    `${urls.origin}/target/multiplex-tests/physical-input.js`
                  );
                  const canvas = document.createElement("canvas");
                  canvas.id = "ordinary-input";
                  canvas.width = 96;
                  canvas.height = 64;
                  canvas.style.width = "96px";
                  canvas.style.height = "64px";
                  document.body.append(canvas);
                  (globalThis as State).pointerTypes = [];
                  canvas.addEventListener("pointerdown", (event) =>
                    (globalThis as State).pointerTypes.push(event.pointerType),
                  );
                  const transport = native
                    ? scenario.nativePresentationTransport(
                        native.url,
                        native.presentationUrl,
                      )
                    : scenario.workerTransport(
                        urls.workerScript,
                        urls.wasm,
                        contract.MAX_MESSAGE_BYTES,
                        { canvas: canvas.transferControlToOffscreen() },
                      );
                  const host =
                    await contract.IppHostClient.connectTransport(transport);
                  (globalThis as State).closePhysicalHost = () => host.close();
                  (globalThis as State).physical =
                    await scenario.preparePhysicalInput(
                      host,
                      contract,
                      canvas,
                      await (
                        await fetch(
                          `${urls.origin}/target/font-assets/shure-tech-mono.ippf`,
                        )
                      ).arrayBuffer(),
                    );
                },
                { urls: environment.urls, native },
              );
              const images = [];
              let failure: unknown;
              // Chromium's own input pipeline, not synthetic DOM events:
              // touch points, IME composition and trusted clipboard keys.
              const cdp = await page.context().newCDPSession(page);
              await page
                .context()
                .grantPermissions(["clipboard-read", "clipboard-write"], {
                  origin: environment.urls.origin,
                });
              const touch = (
                type: "touchStart" | "touchMove" | "touchEnd",
                point?: Point,
              ) =>
                cdp.send("Input.dispatchTouchEvent", {
                  type,
                  touchPoints: point
                    ? [{ x: point[0], y: point[1], id: 1 }]
                    : [],
                });
              const touchDrag = async (from: Point, to: Point, steps = 4) => {
                await touch("touchStart", from);
                for (let step = 1; step <= steps; step++)
                  await touch("touchMove", [
                    from[0] + ((to[0] - from[0]) * step) / steps,
                    from[1] + ((to[1] - from[1]) * step) / steps,
                  ]);
                await touch("touchEnd");
              };
              const pressedBy = async (expected: string) => {
                const types = await page.evaluate(() =>
                  (globalThis as State).pointerTypes.splice(0),
                );
                if (!types.length || types.some((type) => type !== expected))
                  throw new Error(
                    `Expected ${expected} pointer presses, got ${types.join(",")}`,
                  );
                return types.length;
              };
              const clipboard = () =>
                page.evaluate(() => navigator.clipboard.readText());
              try {
                images.push(
                  await page.evaluate(() =>
                    (globalThis as State).physical.image([255, 0, 0, 255]),
                  ),
                );
                const bounds = await page
                  .locator("#ordinary-input")
                  .boundingBox();
                if (!bounds) throw new Error("Physical input Canvas missing");
                await page.mouse.move(bounds.x + 48, bounds.y + 32);
                await page.mouse.down();
                images.push(
                  await page.evaluate(() =>
                    (globalThis as State).physical.image([0, 255, 0, 255]),
                  ),
                );
                await page.mouse.up();
                await page.mouse.move(
                  bounds.x + bounds.width + 10,
                  bounds.y + 32,
                );
                images.push(
                  await page.evaluate(() =>
                    (globalThis as State).physical.image([255, 0, 0, 255]),
                  ),
                );
                const pointer = await page.evaluate(() =>
                  (globalThis as State).physical.observed(1),
                );
                await page.keyboard.press("Enter");
                await page.evaluate(() =>
                  (globalThis as State).physical.image([255, 0, 0, 255]),
                );
                const keyboard = await page.evaluate(() =>
                  (globalThis as State).physical.observed(2),
                );
                // A stationary touch is a tap: it presses while held and
                // leaves no hover behind once the finger lifts.
                await pressedBy("mouse");
                const centre: Point = [bounds.x + 48, bounds.y + 32];
                await touch("touchStart", centre);
                images.push(
                  await page.evaluate(() =>
                    (globalThis as State).physical.image([0, 255, 0, 255]),
                  ),
                );
                await touch("touchEnd");
                images.push(
                  await page.evaluate(() =>
                    (globalThis as State).physical.image([255, 0, 0, 255]),
                  ),
                );
                const touchTap = await page.evaluate(() =>
                  (globalThis as State).physical.observed(3),
                );
                await pressedBy("touch");
                const sliderBefore = await page.evaluate(() =>
                  (globalThis as State).physical.role("GuiSlider"),
                );
                await page.mouse.move(bounds.x + 4, bounds.y + 32);
                await page.mouse.down();
                await page.mouse.move(bounds.x + 95, bounds.y + 32, {
                  steps: 4,
                });
                await page.mouse.up();
                const slider = await page.evaluate(() =>
                  (globalThis as State).physical.value(10),
                );
                if (
                  sliderBefore.pixels.every(
                    (value, index) => value === slider.image.pixels[index],
                  )
                )
                  throw new Error(
                    "Committed slider did not change captured pixels",
                  );
                await page.keyboard.press("Home");
                await page.keyboard.press("ArrowRight");
                const sliderKey = await page.evaluate(() =>
                  (globalThis as State).physical.value(1),
                );
                // A touch drag captures the slider exactly as a mouse drag.
                await pressedBy("mouse");
                await touchDrag(
                  [bounds.x + 12, bounds.y + 32],
                  [bounds.x + 95, bounds.y + 32],
                );
                const sliderTouch = await page.evaluate(() =>
                  (globalThis as State).physical.value(10),
                );
                await pressedBy("touch");
                if (
                  sliderKey.image.pixels.every(
                    (value, index) => value === sliderTouch.image.pixels[index],
                  )
                )
                  throw new Error(
                    "Touch-committed slider did not change captured pixels",
                  );
                const scrollBefore = await page.evaluate(() =>
                  (globalThis as State).physical.role("GuiVirtualList"),
                );
                await page.mouse.move(bounds.x + 48, bounds.y + 32);
                await page.mouse.wheel(0, 40);
                const scroll = await page.evaluate(() =>
                  (globalThis as State).physical.value(40),
                );
                if (
                  scrollBefore.pixels.every(
                    (value, index) => value === scroll.image.pixels[index],
                  )
                )
                  throw new Error(
                    "Committed scroll bar did not change captured pixels",
                  );
                images.push(
                  sliderBefore,
                  slider.image,
                  sliderKey.image,
                  scrollBefore,
                  scroll.image,
                );
                await page.mouse.wheel(0, 200);
                const scrollEnd = await page.evaluate(() =>
                  (globalThis as State).physical.value(136),
                );
                const remainder = await page.evaluate(() =>
                  (globalThis as State).physical.remainder(104),
                );
                images.push(scrollEnd.image);
                await page.evaluate(() =>
                  (globalThis as State).physical.prepareDragTarget(),
                );
                await page.mouse.move(bounds.x + 48, bounds.y + 16);
                await page.mouse.down();
                await page.mouse.move(bounds.x + 48, bounds.y + 46, {
                  steps: 4,
                });
                await page.mouse.up();
                const dragScroll = await page.evaluate(() =>
                  (globalThis as State).physical.value(106),
                );
                images.push(dragScroll.image);
                await page.evaluate(() =>
                  (globalThis as State).physical.finishDragTarget(),
                );
                // Touch arbitration over a child button inside scrolled
                // content: a stationary tap presses it; a drag scrolls the
                // content and cancels the losing press. Offset 106 places
                // item 15 at viewport rows 44..54.
                await pressedBy("mouse");
                await page.evaluate(() =>
                  (globalThis as State).physical.prepareDragTarget(),
                );
                await touch("touchStart", [bounds.x + 48, bounds.y + 49]);
                await touch("touchEnd");
                const touchTapTarget = await page.evaluate(() =>
                  (globalThis as State).physical.tappedDragTarget(106),
                );
                await touchDrag(
                  [bounds.x + 48, bounds.y + 49],
                  [bounds.x + 48, bounds.y + 19],
                );
                const touchScroll = await page.evaluate(() =>
                  (globalThis as State).physical.value(136),
                );
                images.push(touchTapTarget.image, touchScroll.image);
                await page.evaluate(() =>
                  (globalThis as State).physical.finishDragTarget(),
                );
                await pressedBy("touch");
                await page.evaluate(() =>
                  (globalThis as State).physical.role("GuiTextInput"),
                );
                await page.mouse.click(bounds.x + 90, bounds.y + 12);
                const initialText = await page.evaluate(() =>
                  (globalThis as State).physical.text("ab"),
                );
                await page
                  .locator("textarea[data-ipp-native-text]")
                  .waitFor({ state: "attached" });
                await page.keyboard.insertText("c");
                const editedText = await page.evaluate(() =>
                  (globalThis as State).physical.text("abc"),
                );
                await page.evaluate(() => {
                  const area = document.querySelector(
                    "textarea[data-ipp-native-text]",
                  )!;
                  area.dispatchEvent(
                    new CompositionEvent("compositionstart", { data: "" }),
                  );
                  area.dispatchEvent(
                    new CompositionEvent("compositionupdate", { data: "é" }),
                  );
                  area.dispatchEvent(
                    new InputEvent("beforeinput", {
                      inputType: "insertCompositionText",
                      data: "é",
                      cancelable: true,
                    }),
                  );
                });
                const provisional = await page.evaluate(() =>
                  (globalThis as State).physical.text("abc", true),
                );
                await page.evaluate(() =>
                  document
                    .querySelector("textarea[data-ipp-native-text]")!
                    .dispatchEvent(
                      new CompositionEvent("compositionend", { data: "é" }),
                    ),
                );
                const composedText = await page.evaluate(() =>
                  (globalThis as State).physical.text("abcé"),
                );
                // Chromium IME composition: provisional text paints without
                // committing, an empty composition cancels, and accepting the
                // candidate commits it once.
                await cdp.send("Input.imeSetComposition", {
                  text: "ü",
                  selectionStart: 1,
                  selectionEnd: 1,
                });
                const imeProvisional = await page.evaluate(() =>
                  (globalThis as State).physical.text("abcé", true),
                );
                await cdp.send("Input.imeSetComposition", {
                  text: "",
                  selectionStart: 0,
                  selectionEnd: 0,
                });
                const imeCancelled = await page.evaluate(() =>
                  (globalThis as State).physical.text("abcé"),
                );
                await cdp.send("Input.imeSetComposition", {
                  text: "ü",
                  selectionStart: 1,
                  selectionEnd: 1,
                });
                await page.evaluate(() =>
                  (globalThis as State).physical.text("abcé", true),
                );
                await cdp.send("Input.insertText", { text: "ü" });
                const imeCommitted = await page.evaluate(() =>
                  (globalThis as State).physical.text("abcéü"),
                );
                if (
                  imeCancelled.image.pixels.every(
                    (value, index) =>
                      value === imeProvisional.image.pixels[index],
                  )
                )
                  throw new Error("IME composition did not affect paint");
                // Trusted clipboard shortcuts: copy and cut read the committed
                // selection, cut deletes it, and paste inserts the clipboard.
                await page.keyboard.press("ControlOrMeta+A");
                await page.evaluate(() =>
                  (globalThis as State).physical.selection(0, 7),
                );
                await page.keyboard.press("ControlOrMeta+C");
                const copied = await clipboard();
                if (copied !== "abcéü")
                  throw new Error(`Copy wrote ${JSON.stringify(copied)}`);
                await page.evaluate(() =>
                  (globalThis as State).physical.text("abcéü"),
                );
                await page
                  .locator("textarea[data-ipp-native-text]")
                  .evaluate((editor) => {
                    const area = editor as HTMLTextAreaElement;
                    area.setSelectionRange(0, 2, "forward");
                    area.dispatchEvent(new Event("select", { bubbles: true }));
                  });
                await page.evaluate(() =>
                  (globalThis as State).physical.selection(0, 2),
                );
                await page.keyboard.press("ControlOrMeta+X");
                const cutText = await page.evaluate(() =>
                  (globalThis as State).physical.text("céü"),
                );
                const cut = await clipboard();
                if (cut !== "ab") throw new Error(`Cut wrote ${cut}`);
                await page.evaluate(() =>
                  navigator.clipboard.writeText("-clip-"),
                );
                await page.keyboard.press("ControlOrMeta+V");
                const clipboardText = await page.evaluate(() =>
                  (globalThis as State).physical.text("-clip-céü"),
                );
                await page.keyboard.press("ControlOrMeta+A");
                await page.evaluate(() => {
                  const transfer = new DataTransfer();
                  transfer.setData("text/plain", "paste");
                  document
                    .querySelector("textarea[data-ipp-native-text]")!
                    .dispatchEvent(
                      new ClipboardEvent("paste", {
                        clipboardData: transfer,
                        cancelable: true,
                      }),
                    );
                });
                const pastedText = await page.evaluate(() =>
                  (globalThis as State).physical.text("paste"),
                );
                await page.evaluate(() =>
                  (globalThis as State).physical.replaceText("reset"),
                );
                const resetText = await page.evaluate(() =>
                  (globalThis as State).physical.text("reset"),
                );
                await page.keyboard.press("Escape");
                await page.evaluate(() =>
                  (globalThis as State).physical.blurredText(),
                );
                await page.keyboard.press("Tab");
                await page.evaluate(() =>
                  (globalThis as State).physical.text("reset"),
                );
                images.push(
                  initialText.image,
                  editedText.image,
                  provisional.image,
                  composedText.image,
                  imeProvisional.image,
                  imeCommitted.image,
                  cutText.image,
                  clipboardText.image,
                  pastedText.image,
                  resetText.image,
                );
                if (
                  editedText.image.pixels.every(
                    (value, index) => value === provisional.image.pixels[index],
                  )
                )
                  throw new Error(
                    "Provisional composition did not affect paint",
                  );
                if (
                  editedText.image.pixels.every(
                    (value, index) => value === initialText.image.pixels[index],
                  )
                )
                  throw new Error("Committed text did not affect paint");
                await page.evaluate(() =>
                  (globalThis as State).physical.rebind(),
                );
                const nested = [];
                for (const capacity of [100, 5]) {
                  for (const drag of [false, true]) {
                    await page.evaluate(
                      (capacity) =>
                        (globalThis as State).physical.prepareNested(capacity),
                      capacity,
                    );
                    await page.mouse.move(bounds.x + 48, bounds.y + 40);
                    if (drag) {
                      await page.mouse.down();
                      await page.mouse.move(bounds.x + 48, bounds.y + 20);
                      await page.mouse.up();
                    } else await page.mouse.wheel(0, 20);
                    const result = await page.evaluate(
                      ({ inner, outer }) =>
                        (globalThis as State).physical.nestedValue(
                          inner,
                          outer,
                        ),
                      {
                        inner: Math.min(capacity, 20),
                        outer: 20 - Math.min(capacity, 20),
                      },
                    );
                    nested.push({ capacity, drag, result });
                    images.push(result.image);
                  }
                }
                await page.evaluate(() =>
                  (globalThis as State).physical.armSaturatedWheel(),
                );
                await page.mouse.wheel(0, 20);
                const sceneFallback = await page.evaluate(() =>
                  (globalThis as State).physical.sceneGate(true),
                );
                if (sceneFallback.length !== 1 || sceneFallback[0] !== 20)
                  throw new Error(
                    "Saturated wheel fallback was not exactly once",
                  );
                await page.evaluate(async () => {
                  const state = globalThis as State;
                  state.blockers = await state.physical.blockers();
                });
                await page.mouse.click(bounds.x + 48, bounds.y + 32);
                const blocked = await page.evaluate(() =>
                  (globalThis as State).blockers.verify(0, true),
                );
                images.push(blocked.image);
                for (const [button, physicalButton] of [
                  ["right", "secondary"],
                  ["middle", "auxiliary"],
                ] as const) {
                  await page.evaluate(
                    (button) =>
                      (globalThis as State).blockers.armButton(button),
                    physicalButton,
                  );
                  await page.mouse.click(bounds.x + 48, bounds.y + 32, {
                    button,
                  });
                  await page.evaluate(() =>
                    (globalThis as State).blockers.gateResult(false),
                  );
                  await page.evaluate(
                    (button) =>
                      (globalThis as State).blockers.armButton(button),
                    physicalButton,
                  );
                  await page.mouse.click(bounds.x + 4, bounds.y + 4, {
                    button,
                  });
                  await page.evaluate(() =>
                    (globalThis as State).blockers.gateResult(true),
                  );
                }
                await page.evaluate(() =>
                  (globalThis as State).blockers.open(false),
                );
                await page.mouse.click(bounds.x + 48, bounds.y + 32);
                const transparent = await page.evaluate(() =>
                  (globalThis as State).blockers.verify(1, false),
                );
                await page.evaluate(() =>
                  (globalThis as State).blockers.open(true),
                );
                const replacedBlocker = await page.evaluate(() =>
                  (globalThis as State).blockers.replacedBlocker(),
                );
                await page.mouse.click(bounds.x + 48, bounds.y + 32);
                const stale = await page.evaluate(() =>
                  (globalThis as State).blockers.verify(2, false),
                );
                await page.evaluate(() =>
                  (globalThis as State).blockers.scrollRole(),
                );
                await page.mouse.move(bounds.x + 48, bounds.y + 32);
                await page.mouse.wheel(0, 20);
                const blockedScroll = await page.evaluate(() =>
                  (globalThis as State).blockers.verify(2, true, true),
                );
                await page.evaluate(() =>
                  (globalThis as State).blockers.open(false),
                );
                await page.mouse.wheel(0, 20);
                const transparentScroll = await page.evaluate(() =>
                  (globalThis as State).blockers.verify(2, false, true),
                );
                images.push(
                  transparent.image,
                  stale.image,
                  blockedScroll.image,
                  transparentScroll.image,
                );
                return {
                  blockers: {
                    blocked,
                    transparent,
                    replacedBlocker,
                    stale,
                    blockedScroll,
                    transparentScroll,
                  },
                  nested,
                  sceneFallback,
                  pointer,
                  keyboard,
                  touch: {
                    tap: touchTap,
                    slider: sliderTouch.snapshot,
                    tapInScroll: touchTapTarget.pressed,
                    dragScroll: touchScroll.snapshot,
                  },
                  nativeText: {
                    initialText,
                    editedText,
                    provisional,
                    composedText,
                    imeProvisional,
                    imeCancelled,
                    imeCommitted,
                    cutText,
                    clipboardText,
                    pastedText,
                    resetText,
                  },
                  clipboard: { copied, cut },
                  slider: slider.snapshot,
                  sliderKey: sliderKey.snapshot,
                  scroll: scroll.snapshot,
                  scrollEnd: scrollEnd.snapshot,
                  remainder,
                  images,
                };
              } catch (error) {
                failure = error;
                throw error;
              } finally {
                await cdp.detach().catch(() => {});
                await page
                  .evaluate(async () => {
                    try {
                      await (globalThis as State).blockers?.close();
                      await (globalThis as State).physical?.close();
                    } finally {
                      await (globalThis as State).closePhysicalHost?.();
                    }
                  })
                  .catch((error: unknown) => {
                    if (failure === undefined) throw error;
                    console.error(
                      "Physical input cleanup after scenario failure:",
                      error,
                    );
                  });
              }
            },
          ),
      );
      const captures = resolve(result.evidenceDirectory, "captures");
      await writeFile(
        resolve(result.evidenceDirectory, "outcomes.json"),
        JSON.stringify(result.value, (key, value: unknown) =>
          key === "images" || key === "image"
            ? undefined
            : typeof value === "bigint"
              ? value.toString()
              : value,
        ),
      );
      await mkdir(captures, { recursive: true });
      for (const [index, image] of result.value.images.entries())
        await writeFile(
          resolve(captures, `${index}.png`),
          encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
        );
    }
    await withPresentingHost(
      "physical-input-gles-host",
      build,
      context.signal,
      {
        native,
        operationTimeoutMs: 110_000,
        missingPresentation: "GLES diagnostics unavailable",
      },
      browser,
    );
  });
}
