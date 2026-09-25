import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import type { Page } from "playwright";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "./environment.js";

/** Completed-frame RGBA rows of the mounted text input, top row first. */
interface TextInputPaint {
  readonly width: number;
  readonly height: number;
  readonly pixels: readonly number[];
}

/** Rows inside the mounted text input, clear of its focus ring. */
const TEXT_ROWS = { top: 3, bottom: 71 } as const;

function rgb(
  paint: TextInputPaint,
  x: number,
  y: number,
): readonly [number, number, number] {
  const offset = (y * paint.width + x) * 4;
  return [
    paint.pixels[offset]!,
    paint.pixels[offset + 1]!,
    paint.pixels[offset + 2]!,
  ];
}

/** Columns holding matching pixels on at least `rows` text rows. */
function columns(
  paint: TextInputPaint,
  rows: number,
  matches: (x: number, y: number) => boolean,
): number[] {
  const found: number[] = [];
  for (let x = 0; x < paint.width; x += 1) {
    let count = 0;
    for (let y = TEXT_ROWS.top; y <= TEXT_ROWS.bottom; y += 1)
      if (matches(x, y)) count += 1;
    if (count >= rows) found.push(x);
  }
  return found;
}

/** Columns of the near-black caret bar over the blue control background. */
function caretColumns(paint: TextInputPaint): number[] {
  return columns(paint, 4, (x, y) => {
    const [r, g, b] = rgb(paint, x, y);
    return r < 100 && g < 100 && b < 120;
  });
}

/** Pixels `next` tints toward the translucent selection blue over `base`. */
function tintedPixels(base: TextInputPaint, next: TextInputPaint): number {
  let count = 0;
  for (let y = TEXT_ROWS.top; y <= TEXT_ROWS.bottom; y += 1)
    for (let x = 0; x < next.width; x += 1) {
      const [r0, g0, b0] = rgb(base, x, y);
      const [r1, g1, b1] = rgb(next, x, y);
      if (r1 >= r0 + 6 && g1 >= g0 + 6 && b1 >= b0 + 8) count += 1;
    }
  return count;
}

/** Columns where `next` paints bright label glyph coverage absent from `base`. */
function newGlyphColumns(base: TextInputPaint, next: TextInputPaint): number[] {
  return columns(next, 2, (x, y) => {
    const [r] = rgb(next, x, y);
    return r > 150 && r > rgb(base, x, y)[0] + 80;
  });
}

/** Failure context: the paint as a coarse character map, every second
 * column and row: `#` bright glyph coverage, `o` dark caret, `+` lighter
 * than the control background, `-` darker, `.` background. */
function describe(value: unknown): string {
  const paint = value as Partial<TextInputPaint>;
  if (paint.pixels === undefined || paint.width === undefined)
    return JSON.stringify(value);
  const band = paint as TextInputPaint;
  const background = rgb(band, Math.floor(band.width / 2), TEXT_ROWS.top);
  const lines: string[] = [];
  for (let y = 0; y < band.height; y += 2) {
    let line = "";
    for (let x = 0; x < band.width; x += 2) {
      const [r, g, b] = rgb(band, x, y);
      const delta = r + g + b - background[0] - background[1] - background[2];
      line +=
        r > 180 && g > 180 && b > 180
          ? "#"
          : r < 100 && g < 100 && b < 120
            ? "o"
            : delta > 30
              ? "+"
              : delta < -30
                ? "-"
                : ".";
    }
    lines.push(line);
  }
  return `background ${background.join("/")}\n${lines.join("\n")}`;
}

/** Drive selection and IME composition on the mounted text input and assert
 * the completed frames paint the selection highlight, the caret bar and the
 * provisional glyphs as distinct retained work. Leaves the committed text,
 * a collapsed end selection and no composition behind. */
async function exerciseTextOverlayPaint(page: Page, fixture: string) {
  const readPaint = (): Promise<TextInputPaint> =>
    page.evaluate(async (url) => (await import(url)).textInputPaint(), fixture);
  const readObservation = () =>
    page.evaluate(async (url) => (await import(url)).observation(), fixture);
  const poll = async <T>(
    read: () => Promise<T>,
    predicate: (value: T) => boolean,
    message: string,
  ): Promise<T> => {
    const deadline = performance.now() + 5_000;
    let value = await read();
    while (!predicate(value) && performance.now() < deadline) {
      await new Promise<void>((resolve) => setTimeout(resolve, 25));
      value = await read();
    }
    assert.ok(predicate(value), `${message}: ${describe(value)}`);
    return value;
  };
  const select = async (
    start: number,
    end: number,
    core: readonly [number, number],
  ): Promise<void> => {
    await page.locator("textarea").evaluate(
      (editor, [start, end]) => {
        const textarea = editor as HTMLTextAreaElement;
        textarea.setSelectionRange(start!, end!, "forward");
        textarea.dispatchEvent(new Event("select", { bubbles: true }));
      },
      [start, end],
    );
    await poll(
      readObservation,
      (value) =>
        value.focusSelection?.[0] === core[0] &&
        value.focusSelection?.[1] === core[1],
      `selection ${core} did not reach core`,
    );
  };

  // "a😀b" is four UTF-16 units and six UTF-8 bytes.
  await select(4, 4, [6, 6]);
  const collapsed = await poll(
    readPaint,
    (paint) => caretColumns(paint).length > 0,
    "collapsed caret bar did not paint",
  );
  const caret = caretColumns(collapsed);
  const caretEnd = Math.max(...caret);

  await select(0, 4, [0, 6]);
  const selected = await poll(
    readPaint,
    (paint) => tintedPixels(collapsed, paint) >= 100,
    "selection highlight did not paint",
  );
  const selectedCaret = caretColumns(selected);
  assert.ok(
    caret.every((column) => selectedCaret.includes(column)),
    `caret bar did not paint beside the selection: ${JSON.stringify({ caret, selectedCaret })}`,
  );

  await select(4, 4, [6, 6]);
  await poll(
    readPaint,
    (paint) => tintedPixels(collapsed, paint) < 10,
    "selection highlight did not clear",
  );

  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send("Input.imeSetComposition", {
      text: "ZZ",
      selectionStart: 2,
      selectionEnd: 2,
    });
    const composing = await poll(
      readPaint,
      (paint) =>
        newGlyphColumns(collapsed, paint).filter((x) => x > caretEnd).length >=
        4,
      "provisional composition glyphs did not paint",
    );
    const provisional = newGlyphColumns(collapsed, composing);
    const composingCaret = caretColumns(composing);
    assert.ok(
      provisional.every((x) => x >= caret[0]! - 1),
      `provisional glyphs repainted committed text: ${JSON.stringify({ caret, provisional })}`,
    );
    assert.ok(
      composingCaret.length > 0 &&
        composingCaret.every((x) => x > caretEnd + 2),
      `provisional caret did not replace the committed caret: ${JSON.stringify({ caret, composingCaret })}`,
    );

    // An empty composition cancels: the provisional run leaves the frame.
    await cdp.send("Input.imeSetComposition", {
      text: "",
      selectionStart: 0,
      selectionEnd: 0,
    });
    await poll(
      readPaint,
      (paint) =>
        newGlyphColumns(collapsed, paint).length === 0 &&
        caretColumns(paint).join() === caret.join(),
      "cancelled composition kept painting",
    );
    const observation = await readObservation();
    assert.equal(observation.text, "a😀b");
    return {
      caret,
      selectedTint: tintedPixels(collapsed, selected),
      selectedCaret,
      provisional,
      composingCaret,
    };
  } finally {
    await cdp.detach();
  }
}

test("GUI roots, node identity and committed values cross a real worker connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/headless-gui");
  const build: BrowserBuildConfiguration = {
    name: "headless-gui",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    exportWasm: resolve(profile, "export.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      const result = await env.execute("gui lifecycle", {}, () =>
        env.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const { exerciseGuiLifecycle } = await import(
            `${urls.origin}/dist/tests/integration/scenarios/gui-lifecycle.js`
          );
          const canvas = document.createElement("canvas");
          canvas.width = 240;
          canvas.height = 180;
          const host = await contract.IppHostClient.connectWorker(
            urls.workerScript,
            urls.wasm,
            { canvas: canvas.transferControlToOffscreen() },
          );
          try {
            return await exerciseGuiLifecycle(host, contract);
          } finally {
            await host.close();
          }
        }, env.urls),
      );
      env.evidence.record("gui lifecycle", result);
      assert.equal(result.batchApplied, 50);
      assert.equal(result.batchRequests, 1);
      assert.equal(result.failedBatchApplied, 1);
      assert.equal(result.largeBatchApplied, 18);
      assert.equal(result.largeBatchRequests, 4);
      assert.equal(result.failedLargeBatchApplied, 18);
      assert.equal(result.failedLargeBatchRequests, 3);
    },
  );
});

test("GUI pointer, keyboard and text input routes through a real worker connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/headless-gui");
  const build: BrowserBuildConfiguration = {
    name: "headless-gui",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    exportWasm: resolve(profile, "export.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      const result = await env.execute("gui input", {}, () =>
        env.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const { exerciseGuiInput } = await import(
            `${urls.origin}/dist/tests/integration/scenarios/gui-input.js`
          );
          const canvas = document.createElement("canvas");
          canvas.width = 240;
          canvas.height = 180;
          const font = await (
            await fetch(
              `${urls.origin}/target/font-assets/shure-tech-mono.ippf`,
            )
          ).arrayBuffer();
          const host = await contract.IppHostClient.connectWorker(
            urls.workerScript,
            urls.wasm,
            { canvas: canvas.transferControlToOffscreen() },
          );
          try {
            return await exerciseGuiInput(host, font);
          } finally {
            await host.close();
          }
        }, env.urls),
      );
      env.evidence.record("gui input", result);
    },
  );
});

test("mounted IppCanvas owns trusted text, IME, selection and clipboard lifecycle", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/headless-gui");
  const build: BrowserBuildConfiguration = {
    name: "headless-gui",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    exportWasm: resolve(profile, "export.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui-native-text",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      const fixture = `${env.urls.origin}/target/react-build/gui-fixture.js`;
      await env.page
        .context()
        .grantPermissions(["clipboard-read", "clipboard-write"], {
          origin: env.urls.origin,
        });
      await env.page.evaluate(
        async ({ fixture, runtime }) => {
          const mounted = await import(fixture);
          await mounted.mountGuiCanvas(runtime);
        },
        {
          fixture,
          runtime: {
            generatedModuleUrl: env.urls.generated,
            workerScriptUrl: env.urls.workerScript,
            wasmUrl: env.urls.wasm,
            timeoutMs: 20_000,
            logLevel: "off",
          },
        },
      );
      const readObservation = () =>
        env.page.evaluate(
          async (url) => (await import(url)).observation(),
          fixture,
        );
      const readControlPaint = (flush = true) =>
        env.page.evaluate(
          async ({ url, flush }) =>
            (await import(url)).controlPaintObservation(flush),
          { url: fixture, flush },
        );
      type MountedObservation = Awaited<ReturnType<typeof readObservation>>;
      type ControlPaintObservation = Awaited<
        ReturnType<typeof readControlPaint>
      >;
      const waitForObservation = async (
        predicate: (value: MountedObservation) => boolean,
        message: string,
      ): Promise<MountedObservation> => {
        const deadline = performance.now() + 5_000;
        let value = await readObservation();
        while (!predicate(value) && performance.now() < deadline) {
          await new Promise<void>((resolve) => setTimeout(resolve, 25));
          value = await readObservation();
        }
        assert.ok(predicate(value), `${message}: ${JSON.stringify(value)}`);
        return value;
      };
      const waitForControlPaint = async (
        predicate: (value: ControlPaintObservation) => boolean,
        message: string,
        flush = true,
      ): Promise<ControlPaintObservation> => {
        const deadline = performance.now() + 5_000;
        let value = await readControlPaint(flush);
        while (!predicate(value) && performance.now() < deadline) {
          await new Promise<void>((resolve) => setTimeout(resolve, 25));
          value = await readControlPaint(flush);
        }
        assert.ok(predicate(value), `${message}: ${JSON.stringify(value)}`);
        return value;
      };

      // One actual trusted tap is enough: core first confirms text focus,
      // then the still-active gesture opens the sole native editor.
      await env.page
        .locator("#mounted-gui-canvas")
        .click({ position: { x: 120, y: 45 } });
      await waitForObservation(
        (value) => value.activeEditor,
        "trusted text activation failed",
      );
      const themeFrame = await env.page.evaluate(
        async (url) => (await import(url)).captureThemeEvidence(),
        fixture,
      );
      assert.deepEqual(themeFrame.parts, [
        "background",
        "fill",
        "focusRing",
        "icon",
        "label",
      ]);
      assert.equal(themeFrame.width, 240);
      assert.equal(themeFrame.height, 180);
      assert.ok(themeFrame.drawCalls > 0);
      assert.ok(themeFrame.coloredPixels > 0);

      // Text-input overlays paint through retained GUI rendering with their
      // own identities: a selection highlight beside the caret bar, and a
      // provisional composition run with its own glyphs.
      const textPaint = await exerciseTextOverlayPaint(env.page, fixture);
      env.evidence.record("text overlay paint", textPaint);

      // Backward DOM selection around a multibyte code point round-trips as
      // anchor=5, caret=1 in the runtime's UTF-8 coordinate space.
      await env.page.locator("textarea").evaluate((editor) => {
        const textarea = editor as HTMLTextAreaElement;
        textarea.setSelectionRange(1, 3, "backward");
        textarea.dispatchEvent(new Event("select", { bubbles: true }));
      });
      await waitForObservation(
        (value) =>
          value.selectionDirection === "backward" &&
          value.focusSelection?.[0] === 5 &&
          value.focusSelection?.[1] === 1,
        "backward selection did not reach core",
      );

      await env.page.keyboard.insertText("X");
      const cdp = await env.page.context().newCDPSession(env.page);
      await cdp.send("Input.imeSetComposition", {
        text: "世",
        selectionStart: 1,
        selectionEnd: 1,
      });
      // Chromium accepts the live composition through Input.insertText. An
      // empty imeSetComposition is the cancellation gesture and must not be
      // mistaken for a committed final value.
      await cdp.send("Input.insertText", { text: "世" });
      await waitForObservation(
        (value) => value.text === "aX世b",
        "accepted composition did not commit",
      );

      await env.page.evaluate(() => navigator.clipboard.writeText("-clip-"));
      await env.page.keyboard.press("Control+V");
      await waitForObservation(
        (value) => value.text === "aX世-clip-b",
        "clipboard paste did not commit",
      );

      // Copy reads the authoritative committed selection, never a divergent
      // DOM buffer value.
      await env.page.locator("textarea").evaluate((editor) => {
        const textarea = editor as HTMLTextAreaElement;
        textarea.setSelectionRange(0, 1, "forward");
        textarea.dispatchEvent(new Event("select", { bubbles: true }));
      });
      await env.page.keyboard.press("Control+C");
      assert.equal(
        await env.page.evaluate(() => navigator.clipboard.readText()),
        "a",
      );
      await env.page.locator("textarea").evaluate((editor) => {
        const textarea = editor as HTMLTextAreaElement;
        textarea.setSelectionRange(
          textarea.value.length,
          textarea.value.length,
        );
        textarea.dispatchEvent(new Event("select", { bubbles: true }));
      });
      await waitForObservation(
        (value) =>
          value.focusSelection?.[0] === 12 && value.focusSelection?.[1] === 12,
        "end selection did not reach core",
      );

      await env.page.evaluate(
        async (url) => (await import(url)).equivalentRerender(),
        fixture,
      );
      const afterRerender = await waitForObservation(
        (value) => value.renderRevision === 1,
        "equivalent rerender did not reconcile locally",
      );
      assert.deepEqual(
        afterRerender.focusSelection,
        [12, 12],
        `equivalent rerender changed core selection: ${JSON.stringify(afterRerender)}`,
      );
      assert.deepEqual(
        afterRerender.domSelection,
        [10, 10],
        `equivalent rerender changed DOM selection: ${JSON.stringify(afterRerender)}`,
      );
      // Editing the named control theme while the text input is being edited
      // updates only the root theme: the completed frame repaints the button,
      // node rows and theme handles stay as they were, and editing continues.
      const themeEvidence = () =>
        env.page.evaluate(
          async (url) => (await import(url)).themeEditEvidence(),
          fixture,
        );
      const beforeTheme = await themeEvidence();
      assert.ok(
        beforeTheme.button[2]! > beforeTheme.button[0]!,
        `initial tone is not blue: ${beforeTheme.button}`,
      );
      await env.page.evaluate(
        async (url) => (await import(url)).retoneTheme(1),
        fixture,
      );
      let afterTheme = await themeEvidence();
      for (
        const deadline = performance.now() + 5_000;
        afterTheme.button[0]! <= afterTheme.button[2]!;
        afterTheme = await themeEvidence()
      )
        if (performance.now() > deadline)
          throw new Error(`theme edit never repainted: ${afterTheme.button}`);
      env.evidence.record("theme edit during editing", {
        before: beforeTheme.button,
        after: afterTheme.button,
      });
      assert.equal(afterTheme.nodeStyles, beforeTheme.nodeStyles);
      assert.deepEqual(afterTheme.themes, beforeTheme.themes);
      const afterThemeEdit = await readObservation();
      assert.deepEqual(afterThemeEdit.focusSelection, [12, 12]);
      assert.equal(afterThemeEdit.text, "aX世-clip-b");

      await env.page.keyboard.insertText("!");
      await waitForObservation(
        (value) => value.text === "aX世-clip-b!",
        "post-rerender text did not commit",
      );
      const edited = await env.page.evaluate(
        async (url) => (await import(url)).observation(),
        fixture,
      );
      assert.equal(edited.sameEditor, true);
      assert.equal(edited.editorCount, 1);
      assert.equal(edited.activeEditor, true);
      assert.deepEqual(edited.callbackValues, [
        "aXb",
        "aX世b",
        "aX世-clip-b",
        "aX世-clip-b!",
      ]);
      assert.equal(
        edited.callbackValues.filter((value: string) => value === "aX世b")
          .length,
        1,
      );
      assert.equal(edited.callbackRenders.at(-1), 1);
      assert.deepEqual(edited.errors, []);

      // A non-text control click clears authoritative text focus and does
      // not write another text value.
      await env.page
        .locator("#mounted-gui-canvas")
        .click({ position: { x: 120, y: 135 } });
      await env.page.waitForFunction(
        () => document.activeElement !== document.querySelector("textarea"),
      );
      const afterButton = await env.page.evaluate(
        async (url) => (await import(url)).observation(),
        fixture,
      );
      assert.equal(afterButton.text, "aX世-clip-b!");
      assert.equal(afterButton.presses, 1);
      assert.equal(afterButton.callbackValues.length, 4);

      const controlBefore = await readControlPaint();
      assert.equal(controlBefore.checked, false);
      assert.ok(Math.abs(controlBefore.slider - 0.2) < 0.001);
      assert.equal(controlBefore.failedDrawCalls, 0);
      await env.page
        .locator("#mounted-gui-canvas")
        .click({ position: { x: 83, y: 90 } });
      await waitForControlPaint(
        (value) => value.checked,
        "checkbox pointer commit did not reach core",
      );
      await env.page
        .locator("#mounted-gui-canvas")
        .click({ position: { x: 139, y: 90 } });
      const atPaintedThumb = await readControlPaint();
      assert.ok(
        Math.abs(atPaintedThumb.slider - 0.2) < 0.03,
        `pointer-down at the painted slider thumb changed its value: ${JSON.stringify(atPaintedThumb)}`,
      );
      await env.page
        .locator("#mounted-gui-canvas")
        .click({ position: { x: 225, y: 90 } });
      const controlAfter = await waitForControlPaint(
        (value) => value.slider > 0.8,
        "slider pointer commit did not reach core",
      );
      const intensity = (rgb: readonly number[]) => rgb[0]! + rgb[1]! + rgb[2]!;
      assert.ok(
        intensity(controlAfter.checkboxIndicator) >
          intensity(controlBefore.checkboxIndicator) + 100,
        `checked indicator did not brighten: ${JSON.stringify({ controlBefore, controlAfter })}`,
      );
      assert.ok(
        Math.abs(
          intensity(controlAfter.checkboxOutsideIndicator) -
            intensity(controlBefore.checkboxOutsideIndicator),
        ) < 40,
        `fitted drawing-backed indicator painted outside its retained rectangle: ${JSON.stringify({ controlBefore, controlAfter })}`,
      );
      assert.ok(
        intensity(controlBefore.sliderInitialThumb) >
          intensity(controlAfter.sliderInitialThumb) + 100,
        `slider did not leave its initial thumb region: ${JSON.stringify({ controlBefore, controlAfter })}`,
      );
      assert.ok(
        intensity(controlAfter.sliderMovedThumb) >
          intensity(controlBefore.sliderMovedThumb) + 100,
        `slider did not paint its committed thumb region: ${JSON.stringify({ controlBefore, controlAfter })}`,
      );
      assert.ok(
        controlAfter.sliderFill[1]! > controlBefore.sliderFill[1]! + 60,
        `slider fill did not follow its committed value: ${JSON.stringify({ controlBefore, controlAfter })}`,
      );
      // Consecutive GUI work shares draws, so added content need not add draws.
      assert.ok(controlAfter.drawCalls > 0);
      assert.equal(controlAfter.failedDrawCalls, 0);

      await env.page.evaluate(
        async (url) => (await import(url)).holdReactCommitReply(),
        fixture,
      );
      try {
        await env.page.evaluate(
          async (url) => (await import(url)).renderCommitRevision(1),
          fixture,
        );
        await env.page.evaluate(
          async (url) => (await import(url)).waitForHeldReactCommit(),
          fixture,
        );
        await env.page.evaluate(async (url) => {
          const mounted = await import(url);
          for (let revision = 2; revision <= 100; revision += 1)
            mounted.renderCommitRevision(revision);
        }, fixture);
        const bounds = await env.page
          .locator("#mounted-gui-canvas")
          .boundingBox();
        assert.ok(bounds);
        const x = (local: number) => bounds.x + local;
        const y = bounds.y + 90;
        await env.page.mouse.click(x(130), y);
        const low = await waitForControlPaint(
          (value) => value.slider < 0.2,
          "low gain did not commit while React was held",
          false,
        );
        await env.page.mouse.move(x(130), y);
        await env.page.mouse.down();
        await env.page.mouse.move(x(225), y, { steps: 12 });
        await env.page.mouse.up();
        const high = await waitForControlPaint(
          (value) => value.slider > 0.8,
          "rapid drag did not commit while React was held",
          false,
        );
        assert.ok(
          high.sliderFill[1]! > low.sliderFill[1]! + 60,
          `runtime fill did not advance during delayed React acknowledgement: ${JSON.stringify({ low, high })}`,
        );
        assert.ok(
          intensity(high.sliderMovedThumb) >
            intensity(low.sliderMovedThumb) + 100,
          `runtime thumb did not advance during delayed React acknowledgement: ${JSON.stringify({ low, high })}`,
        );
        env.evidence.record("slider during delayed React acknowledgement", {
          low,
          high,
        });
      } finally {
        const submissions = await env.page.evaluate(
          async (url) => (await import(url)).releaseHeldReactCommit(),
          fixture,
        );
        env.evidence.record("React submissions after held drag", {
          submissions,
        });
        assert.ok(
          submissions <= 2,
          `rapid renders submitted ${submissions} batches`,
        );
      }
      assert.equal(
        await env.page.evaluate(
          async (url) => (await import(url)).committedRevision(),
          fixture,
        ),
        100,
      );

      const teardown = await env.page.evaluate(
        async (url) => (await import(url)).closeGuiCanvas(),
        fixture,
      );
      assert.deepEqual(teardown, { canvasCount: 0, editorCount: 0 });
      env.evidence.record("mounted native text lifecycle", {
        themeFrame,
        backwardSelection: edited.focusSelection,
        committedValues: edited.callbackValues,
        copied: "a",
        afterButton,
        controlBefore,
        atPaintedThumb,
        controlAfter,
        teardown,
      });
    },
  );
});
