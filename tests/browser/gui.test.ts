import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import type { Page } from "playwright";
import type * as GuiInputScenario from "../integration/scenarios/gui-input.js";
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

/** Rows and columns inside the mounted text input, clear of its focus ring. */
const TEXT_ROWS = { top: 3, bottom: 71 } as const;
const TEXT_COLUMNS = { left: 4, right: 235 } as const;

/** Bright rows a column needs to hold the caret bar: the caret spans the
 * text line, taller than any label or provisional glyph stroke. */
const CARET_ROWS = 26;

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
  const right = Math.min(TEXT_COLUMNS.right, paint.width - 1);
  for (let x = TEXT_COLUMNS.left; x <= right; x += 1) {
    let count = 0;
    for (let y = TEXT_ROWS.top; y <= TEXT_ROWS.bottom; y += 1)
      if (matches(x, y)) count += 1;
    if (count >= rows) found.push(x);
  }
  return found;
}

/** Columns of the theme's white caret bar over the blue control background. */
function caretColumns(paint: TextInputPaint): number[] {
  return columns(paint, CARET_ROWS, (x, y) =>
    rgb(paint, x, y).every((channel) => channel > 200),
  );
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
 * column and row: `#` bright glyph or caret coverage, `o` near-black, `+`
 * lighter than the control background, `-` darker, `.` background. */
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
  const profile = resolve(workspace, "target/browser-build/render");
  const build: BrowserBuildConfiguration = {
    name: "render",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui",
    {
      workspace,
      build,
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
          canvas.width = 256;
          canvas.height = 192;
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
      assert.equal(result.failedBatchApplied, 1);
      assert.equal(result.largeBatchApplied, 18);
      assert.equal(result.failedLargeBatchApplied, 18);

      const restored = await env.execute("gui transient restore", {}, () =>
        env.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const { exerciseGuiTransientRestore } = await import(
            `${urls.origin}/dist/tests/integration/scenarios/gui-lifecycle.js`
          );
          const canvas = document.createElement("canvas");
          canvas.width = 256;
          canvas.height = 192;
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
            return await exerciseGuiTransientRestore(host, contract, font);
          } finally {
            await host.close();
          }
        }, env.urls),
      );
      env.evidence.record("gui transient restore", restored);
      assert.equal(restored.frames.restoredPixels, 0);
    },
  );
});

test("a 100000-item VirtualList scrolls, clips and restores through a real worker connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render");
  const build: BrowserBuildConfiguration = {
    name: "render",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui",
    {
      workspace,
      build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      const result = await env.execute("gui virtual list", {}, () =>
        env.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const { exerciseGuiVirtualList } = await import(
            `${urls.origin}/dist/tests/integration/scenarios/gui-virtual-list.js`
          );
          const canvas = document.createElement("canvas");
          canvas.width = 256;
          canvas.height = 192;
          const host = await contract.IppHostClient.connectWorker(
            urls.workerScript,
            urls.wasm,
            { canvas: canvas.transferControlToOffscreen() },
          );
          try {
            return await exerciseGuiVirtualList(host);
          } finally {
            await host.close();
          }
        }, env.urls),
      );
      env.evidence.record("gui virtual list", result);
      assert.deepEqual(result.attached, [0, 10]);
      assert.equal(result.frames.header, 0);
    },
  );
});

/** The GUI input scenario's state on the page between its timed parts. */
type GuiInputPage = typeof globalThis & {
  guiInput?: {
    scenario: typeof GuiInputScenario;
    host: Parameters<typeof GuiInputScenario.exerciseGuiInput>[0] & {
      close(): Promise<void>;
    };
    font: ArrayBuffer;
  };
};

test("GUI pointer, keyboard and text input routes through a real worker connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render");
  const build: BrowserBuildConfiguration = {
    name: "render",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui",
    {
      workspace,
      build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      // One worker Host connection serves every part of the scenario. Every
      // control read is an inspection answered at a Host frame, so on a
      // software-rendered worker the parts together take about 20 s: each
      // part is its own timed operation, so a stall fails at its part.
      const parts = await env.execute("gui input connection", {}, () =>
        env.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const scenario = await import(
            `${urls.origin}/dist/tests/integration/scenarios/gui-input.js`
          );
          const canvas = document.createElement("canvas");
          canvas.width = 256;
          canvas.height = 192;
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
          (globalThis as GuiInputPage).guiInput = { scenario, host, font };
          return Object.keys(
            scenario.GUI_INPUT_PARTS,
          ) as GuiInputScenario.GuiInputPart[];
        }, env.urls),
      );
      try {
        for (const part of parts) {
          const result = await env.execute(`gui input ${part}`, {}, () =>
            env.page.evaluate(
              (part: GuiInputScenario.GuiInputPart): Promise<unknown> => {
                const { scenario, host, font } = (globalThis as GuiInputPage)
                  .guiInput!;
                return scenario.GUI_INPUT_PARTS[part](host, font);
              },
              part,
            ),
          );
          env.evidence.record(`gui input ${part}`, result);
        }
      } finally {
        await env.page.evaluate(async () => {
          await (globalThis as GuiInputPage).guiInput?.host.close();
        });
      }
    },
  );
});

test("mounted IppCanvas owns trusted text, IME, selection and clipboard lifecycle", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render");
  const build: BrowserBuildConfiguration = {
    name: "render",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui-native-text",
    {
      workspace,
      build,
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
        "caret",
        "fill",
        "focusRing",
        "icon",
        "label",
        "selection",
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
      // updates only the theme entity's part rows: the completed frame
      // repaints the button, the controls' skins and the theme's identity and
      // row slots stay as they were, and editing continues.
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
      assert.equal(afterTheme.skins, beforeTheme.skins);
      assert.equal(afterTheme.theme, beforeTheme.theme);
      const toneOne = [0.62, 0.14, 0.08, 1];
      assert.ok(
        afterTheme.background.every(
          (value: number, index: number) =>
            Math.abs(value - toneOne[index]!) < 1e-6,
        ),
        `named theme background row did not take the new tone: ${afterTheme.background}`,
      );
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
      // onTextCommit reports the current value when registered, then each
      // changed value at the end of its frame. The awaited values each
      // arrive once, in order; the unawaited "aXb" may share a frame with
      // the composition that follows it, so it appears at most once and
      // only before "aX世b".
      assert.equal(edited.callbackValues[0], "a😀b");
      assert.deepEqual(
        edited.callbackValues.filter((value: string) => value !== "aXb"),
        ["a😀b", "aX世b", "aX世-clip-b", "aX世-clip-b!"],
      );
      assert.ok(
        edited.callbackValues.filter((value: string) => value === "aXb")
          .length <= 1 &&
          (!edited.callbackValues.includes("aXb") ||
            edited.callbackValues.indexOf("aXb") <
              edited.callbackValues.indexOf("aX世b")),
        `unexpected intermediate text values: ${edited.callbackValues}`,
      );
      assert.equal(edited.callbackRenders.at(-1), 1);
      assert.deepEqual(edited.errors, []);

      // Enter in the native editor submits the committed text once.
      await env.page.keyboard.press("Enter");
      await waitForObservation(
        (value) =>
          value.submissions.length === 1 &&
          value.submissions[0] === "aX世-clip-b!",
        "Enter did not submit the focused text",
      );

      // An external compare-and-set replacement of the focused text reaches
      // React's value callback and refreshes the native editor without
      // further input; typing then continues on the replaced text.
      await env.page.evaluate(
        async (url) => (await import(url)).replaceText("ext"),
        fixture,
      );
      await waitForObservation(
        (value) =>
          value.editorValue === "ext" && value.callbackValues.at(-1) === "ext",
        "external replacement did not refresh the editor",
      );
      await env.page.keyboard.insertText("!");
      const afterReplace = await waitForObservation(
        (value) => value.text === "ext!",
        "typing after the external replacement did not commit",
      );
      assert.equal(afterReplace.submissions.length, 1);
      assert.deepEqual(afterReplace.errors, []);

      // A non-text control click clears authoritative text focus and does
      // not write another text value.
      await env.page
        .locator("#mounted-gui-canvas")
        .click({ position: { x: 120, y: 135 } });
      await env.page.waitForFunction(
        () => document.activeElement !== document.querySelector("textarea"),
      );
      // DOM focus leaves before the runtime reports the press to React.
      const afterButton = await waitForObservation(
        (value) => value.presses > 0,
        "the non-text control click did not press the button",
      );
      assert.equal(afterButton.text, "ext!");
      assert.equal(afterButton.presses, 1);
      assert.deepEqual(
        afterButton.callbackValues.slice(edited.callbackValues.length),
        ["ext", "ext!"],
      );

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

test("mounted IppCanvas operates text, checkbox, slider and button by keyboard only", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render");
  const build: BrowserBuildConfiguration = {
    name: "render",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui-keyboard",
    {
      workspace,
      build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      const fixture = `${env.urls.origin}/target/react-build/gui-fixture.js`;
      await env.page.evaluate(
        async ({ fixture, runtime }) => {
          const mounted = await import(fixture);
          await mounted.mountGuiCanvas(runtime, { keyboardPanels: true });
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
      const readKeyboard = () =>
        env.page.evaluate(
          async (url) => (await import(url)).keyboardObservation(),
          fixture,
        );
      const readControlPaint = () =>
        env.page.evaluate(
          async (url) => (await import(url)).controlPaintObservation(),
          fixture,
        );
      type KeyboardObservation = Awaited<ReturnType<typeof readKeyboard>>;
      const settle = async <T>(
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
        assert.ok(predicate(value), `${message}: ${JSON.stringify(value)}`);
        return value;
      };
      // Each trusted key waits for the runtime focus and the element that
      // owns keys before the next one, as a keyboard user would.
      const press = async (
        key: string,
        focused: string | null,
        keyOwner: KeyboardObservation["keyOwner"],
      ): Promise<KeyboardObservation> => {
        await env.page.keyboard.press(key);
        return settle(
          readKeyboard,
          (value) => value.focused === focused && value.keyOwner === keyOwner,
          `${key} did not reach ${focused} through the ${keyOwner}`,
        );
      };

      // From a fresh mount no pointer ever touches the page. The document's
      // first Tab focuses the canvas; the next enters the nearest
      // front-facing panel at its first control, the text input, whose
      // native editor then owns the keys. The keyboard-order panels were
      // created first, and the back-facing one is nearer the camera.
      const fresh = await readKeyboard();
      assert.equal(fresh.focused, null);
      assert.equal(fresh.keyOwner, "other");
      await press("Tab", null, "canvas");
      await press("Tab", "text", "editor");
      const controlBefore = await readControlPaint();
      assert.equal(controlBefore.checked, false);

      // Leaving the text input returns keys to the canvas relay.
      await press("Tab", "checkbox", "canvas");
      await env.page.keyboard.press("Space");
      const checked = await settle(
        readControlPaint,
        (value) => value.checked,
        "Space did not toggle the focused checkbox",
      );
      assert.equal(checked.failedDrawCalls, 0);
      assert.ok(checked.drawCalls > 0);

      await press("Tab", "slider", "canvas");
      await env.page.keyboard.press("ArrowRight");
      const stepped = await settle(
        readControlPaint,
        (value) => value.slider > controlBefore.slider + 0.001,
        "ArrowRight did not step the focused slider",
      );

      await press("Tab", "button", "canvas");
      await env.page.keyboard.press("Space");
      const pressed = await settle(
        readKeyboard,
        (value) => value.presses > 0,
        "Space did not press the focused button",
      );
      assert.equal(pressed.presses, 1, "one Space pressed the button twice");

      // Shift+Tab walks backward in tree order: the slider steps back down.
      await press("Shift+Tab", "slider", "canvas");
      await env.page.keyboard.press("ArrowLeft");
      const back = await settle(
        readControlPaint,
        (value) => value.slider < stepped.slider - 0.001,
        "ArrowLeft did not step the slider back",
      );
      assert.equal(back.checked, true);

      // Tabbing back into the text input hands keys to the native editor
      // again, and a typed character reaches core exactly once.
      await press("Shift+Tab", "checkbox", "canvas");
      const beforeTyping = await press("Shift+Tab", "text", "editor");
      await env.page.keyboard.type("Z");
      const typed = await settle(
        readKeyboard,
        (value) => value.text !== beforeTyping.text,
        "typed text did not reach the focused text input",
      );
      assert.equal(typed.text.length, beforeTyping.text.length + 1);
      assert.equal([...typed.text].filter((c) => c === "Z").length, 1);
      assert.equal(typed.presses, 1);
      assert.deepEqual(typed.errors, []);

      // Traversal crosses panels in view order and wraps at the ends:
      // Shift+Tab from the first control reaches the back-facing panel,
      // which orders after the deeper front-facing one, and Tab returns.
      await press("Shift+Tab", "backButton", "canvas");
      await press("Shift+Tab", "farButton", "canvas");
      await press("Shift+Tab", "button", "canvas");
      await press("Tab", "farButton", "canvas");
      await press("Tab", "backButton", "canvas");
      await press("Tab", "text", "editor");

      const teardown = await env.page.evaluate(
        async (url) => (await import(url)).closeGuiCanvas(),
        fixture,
      );
      assert.deepEqual(teardown, { canvasCount: 0, editorCount: 0 });
      env.evidence.record("keyboard-only controls", {
        controlBefore,
        checked,
        stepped,
        back,
        typed,
      });
    },
  );
});

test("mounted nested ScrollViews drag, wheel and clip in completed WebGL frames", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render");
  const build: BrowserBuildConfiguration = {
    name: "render",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui-scroll",
    {
      workspace,
      build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      const fixture = `${env.urls.origin}/target/react-build/gui-scroll-fixture.js`;
      await env.page.evaluate(
        async ({ fixture, runtime }) =>
          (await import(fixture)).mountScrollCanvas(runtime),
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
      const bounds = await env.page.locator("#scroll-gui-canvas").boundingBox();
      assert.ok(bounds, "scroll canvas has no layout box");
      const page = (x: number, y: number) =>
        [bounds.x + x, bounds.y + y] as const;

      // Canvas pixels sampled per frame, one per Canvas logical unit and 60
      // per Surface metre ("unit" below): `right` sits at (3, 0.5) units,
      // `middle` at (3, 1.5), `narrow` at (1, 2.5) and `low` at (3, 2.5).
      const points = {
        right: [180, 30],
        middle: [180, 90],
        narrow: [60, 150],
        low: [180, 150],
      } as const;
      type Rgb = readonly [number, number, number];
      type Samples = Record<keyof typeof points, Rgb>;
      const sample = async (label: string): Promise<Samples> => {
        const frame = await env.page.evaluate(
          async ({ url, points }) =>
            (await import(url)).scrollFrame(Object.values(points)),
          { url: fixture, points },
        );
        assert.equal(frame.failedDrawCalls, 0);
        await writeFile(
          resolve(env.evidence.directory, `${label}.png`),
          Buffer.from(
            frame.dataUrl.slice("data:image/png;base64,".length),
            "base64",
          ),
        );
        const samples = Object.fromEntries(
          Object.keys(points).map((key, index) => [key, frame.samples[index]]),
        ) as Samples;
        env.evidence.record(label, samples);
        return samples;
      };
      // Dominant display colour of a sample: fills are 0.8 against 0.1
      // linear lanes, so dominant lanes lead the others by far more than 80.
      const hue = (rgb: Rgb): string => {
        const [r, g, b] = rgb;
        const lead = (a: number, ...rest: number[]) =>
          rest.every((other) => a > other + 80);
        if (lead(r, b) && lead(g, b) && Math.abs(r - g) < 40) return "yellow";
        if (lead(r, g) && lead(b, g) && Math.abs(r - b) < 40) return "magenta";
        if (lead(g, r) && lead(b, r) && Math.abs(g - b) < 40) return "cyan";
        if (Math.min(r, g, b) > 200) return "white";
        if (lead(r, g, b)) return "red";
        if (lead(g, r, b)) return "green";
        if (lead(b, r, g)) return "blue";
        if (Math.max(r, g, b) - Math.min(r, g, b) < 20 && r > 40) return "gray";
        return `other(${rgb.join(",")})`;
      };
      const expectFrame = async (
        label: string,
        expected: Record<keyof typeof points, string>,
      ) => {
        const deadline = performance.now() + 5_000;
        let samples = await sample(label);
        const matches = (value: Samples) =>
          Object.entries(expected).every(
            ([key, colour]) => hue(value[key as keyof Samples]) === colour,
          );
        while (!matches(samples) && performance.now() < deadline) {
          await new Promise<void>((resolve) => setTimeout(resolve, 50));
          samples = await sample(label);
        }
        assert.deepEqual(
          Object.fromEntries(
            Object.entries(samples).map(([key, rgb]) => [key, hue(rgb)]),
          ),
          expected,
          `${label}: ${JSON.stringify(samples)}`,
        );
      };

      await expectFrame("scroll-initial", {
        right: "red",
        middle: "red",
        narrow: "blue",
        low: "gray",
      });

      // Nested bars stay visible where they would share the right edge: the
      // inner bar column at x 228 (logical 3.8) shows its yellow thumb and
      // blue track beside the outer magenta thumb at x 235.
      const column = {
        innerThumb: [228, 30],
        innerTrack: [228, 100],
        outerThumb: [235, 30],
      } as const;
      const columnFrame = await env.page.evaluate(
        async ({ url, points }) =>
          (await import(url)).scrollFrame(Object.values(points)),
        { url: fixture, points: column },
      );
      assert.equal(columnFrame.failedDrawCalls, 0);
      await writeFile(
        resolve(env.evidence.directory, "scroll-nested-bars.png"),
        Buffer.from(
          columnFrame.dataUrl.slice("data:image/png;base64,".length),
          "base64",
        ),
      );
      const columnHues = Object.fromEntries(
        Object.keys(column).map((key, index) => [
          key,
          hue(columnFrame.samples[index]),
        ]),
      );
      env.evidence.record("scroll-nested-bars", columnFrame.samples);
      assert.deepEqual(columnHues, {
        innerThumb: "yellow",
        innerTrack: "blue",
        outerThumb: "magenta",
      });

      // A primary drag over plain outer content scrolls it by the dragged
      // 60 px (1 unit). The inner viewport moves up to y -1..1, so its green
      // block at y 1..2 falls outside it and must not paint over the outer
      // background at `middle`.
      await env.page.mouse.move(...page(180, 150));
      await env.page.mouse.down();
      await env.page.mouse.move(...page(180, 90), { steps: 6 });
      await env.page.mouse.up();
      await expectFrame("scroll-outer-dragged", {
        right: "red",
        middle: "gray",
        narrow: "blue",
        low: "gray",
      });

      // One wheel notch (100 px) over the moved inner viewport scrolls the
      // inner view by the fixture's step of a quarter unit, an eighth of its
      // 2-unit viewport: the green block's top rises to y 0.75 units (45 px).
      // Pixel rows 50..56 turn green only past 0.17 units and rows 34..40
      // stay red below 0.33 units, bounding it.
      await env.page.mouse.move(...page(180, 30));
      await env.page.mouse.wheel(0, 100);
      const notch = { above: [180, 37], below: [180, 53] } as const;
      const notchDeadline = performance.now() + 5_000;
      let notchHues: Record<keyof typeof notch, string>;
      for (;;) {
        const frame = await env.page.evaluate(
          async ({ url, points }) =>
            (await import(url)).scrollFrame(Object.values(points)),
          { url: fixture, points: notch },
        );
        assert.equal(frame.failedDrawCalls, 0);
        notchHues = {
          above: hue(frame.samples[0]),
          below: hue(frame.samples[1]),
        };
        if (notchHues.below === "green" || performance.now() >= notchDeadline) {
          await writeFile(
            resolve(env.evidence.directory, "scroll-inner-notch.png"),
            Buffer.from(
              frame.dataUrl.slice("data:image/png;base64,".length),
              "base64",
            ),
          );
          env.evidence.record("scroll-inner-notch", frame.samples);
          break;
        }
        await new Promise<void>((resolve) => setTimeout(resolve, 50));
      }
      assert.deepEqual(notchHues, { above: "red", below: "green" });

      // Three more notches scroll the inner view by its remaining 0.75
      // units: the green block rides up into view at `right`.
      await env.page.mouse.wheel(0, 300);
      await expectFrame("scroll-inner-wheeled", {
        right: "green",
        middle: "gray",
        narrow: "blue",
        low: "gray",
      });

      // The outer scroll bar column, 6 px rows at x 235 inside its
      // 3.85..4 track: the thumb rows are the magenta (or pressed white)
      // run, whose extent follows the committed offset over the capacity.
      const rows = Array.from({ length: 30 }, (_, row) => 3 + row * 6);
      // Rows averaging across a thumb end blend both colours; every other row
      // is track or thumb. Input reaches a completed frame asynchronously, so
      // the first capture after an event may still show the previous skin.
      const thumbRows = async (label: string, thumb: string) => {
        const frame = await env.page.evaluate(
          async ({ url, points }) => (await import(url)).scrollFrame(points),
          { url: fixture, points: rows.map((y) => [235, y]) },
        );
        assert.equal(frame.failedDrawCalls, 0);
        await writeFile(
          resolve(env.evidence.directory, `${label}.png`),
          Buffer.from(
            frame.dataUrl.slice("data:image/png;base64,".length),
            "base64",
          ),
        );
        const hues: string[] = frame.samples.map((rgb: Rgb) => hue(rgb));
        env.evidence.record(label, hues);
        const blended = hues.filter(
          (colour) => colour !== thumb && colour !== "cyan",
        );
        const covered = rows.filter((_, index) => hues[index] === thumb);
        return {
          hues,
          span: [covered[0], covered.at(-1)] as const,
          clean:
            blended.length <= 2 &&
            blended.every((colour) => colour.startsWith("other")),
        };
      };
      // Expect the thumb run to cover [start, end) in pixels: rows sample
      // every 6 px and average 7 px, so run ends land within 10 px.
      const expectThumb = async (
        label: string,
        start: number,
        end: number,
        thumb = "magenta",
      ) => {
        const deadline = performance.now() + 5_000;
        const near = ([first, last]: readonly [
          number | undefined,
          number | undefined,
        ]) =>
          first !== undefined &&
          last !== undefined &&
          Math.abs(first - start) <= 10 &&
          Math.abs(last - end) <= 10;
        let rowsOf = await thumbRows(label, thumb);
        while (
          !(rowsOf.clean && near(rowsOf.span)) &&
          performance.now() < deadline
        ) {
          await new Promise<void>((resolve) => setTimeout(resolve, 50));
          rowsOf = await thumbRows(label, thumb);
        }
        assert.ok(
          rowsOf.clean,
          `${label}: bar column is not only track and thumb: ${rowsOf.hues.join(",")}; ${await env.page.evaluate(
            async (url) => (await import(url)).scrollDiagnostics(),
            fixture,
          )}`,
        );
        assert.ok(
          near(rowsOf.span),
          `${label}: thumb rows ${rowsOf.span} not ${start}..${end}`,
        );
      };

      // Outer offset 1 of 2: the 102.6 px thumb starts half way along its
      // 68.4 px travel, which begins below the track's 4.5 px pointed end.
      await expectThumb("bar-scrolled", 39, 141);

      // Dragging the thumb 36 px down scrolls the outer view by one more
      // unit with the pressed skin while held; the capture holds when the
      // pointer leaves the bar.
      await env.page.mouse.move(...page(235, 90));
      await env.page.mouse.down();
      await env.page.mouse.move(...page(150, 126), { steps: 6 });
      await expectThumb("bar-thumb-pressed", 73, 176, "white");
      await env.page.mouse.up();
      await expectThumb("bar-thumb-dragged", 73, 176);
      // Outer offset 2 leaves the narrow blue block above `narrow` and the
      // yellow block under the lower samples.
      await expectFrame("bar-content-after-drag", {
        right: "gray",
        middle: "gray",
        narrow: "yellow",
        low: "yellow",
      });

      // A track press above the thumb pages back by one viewport, clamped
      // at the start.
      await env.page.mouse.click(...page(235, 20));
      await expectThumb("bar-track-paged", 5, 107);

      const canvasCount = await env.page.evaluate(
        async (url) => (await import(url)).closeScrollCanvas(),
        fixture,
      );
      assert.equal(canvasCount, 0);
    },
  );
});

test("a mounted React VirtualList declares its wanted range and scrolls in completed WebGL frames", {
  timeout: 60000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render");
  const build: BrowserBuildConfiguration = {
    name: "render",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui-virtual-list",
    {
      workspace,
      build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/browser",
      ),
    },
    context.signal,
    async (env) => {
      const fixture = `${env.urls.origin}/target/react-build/gui-scroll-fixture.js`;
      await env.page.evaluate(
        async ({ fixture, runtime }) =>
          (await import(fixture)).mountVirtualListCanvas(runtime),
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
      const bounds = await env.page.locator("#scroll-gui-canvas").boundingBox();
      assert.ok(bounds, "VirtualList canvas has no layout box");
      const page = (x: number, y: number) =>
        [bounds.x + x, bounds.y + y] as const;
      type Rgb = readonly [number, number, number];
      // Item fills lead their other lanes by far more than 80 in display
      // values; the list background and the bar skin are told apart too.
      const hue = (rgb: Rgb): string => {
        const [r, g, b] = rgb;
        const lead = (a: number, ...rest: number[]) =>
          rest.every((other) => a > other + 80);
        if (lead(r, b) && lead(g, b) && Math.abs(r - g) < 40) return "yellow";
        if (lead(r, g) && lead(b, g) && Math.abs(r - b) < 40) return "magenta";
        if (lead(g, r) && lead(b, r) && Math.abs(g - b) < 40) return "cyan";
        if (Math.min(r, g, b) > 200) return "white";
        if (lead(r, g, b)) return "red";
        if (lead(g, r, b)) return "green";
        if (lead(b, r, g)) return "blue";
        if (Math.max(r, g, b) - Math.min(r, g, b) < 20 && r > 40) return "gray";
        return `other(${rgb.join(",")})`;
      };
      const fills = ["red", "green", "blue", "yellow"];
      const frame = async (label: string, points: [number, number][]) => {
        const captured = await env.page.evaluate(
          async ({ url, points }) => (await import(url)).scrollFrame(points),
          { url: fixture, points },
        );
        assert.equal(captured.failedDrawCalls, 0);
        await writeFile(
          resolve(env.evidence.directory, `${label}.png`),
          Buffer.from(
            captured.dataUrl.slice("data:image/png;base64,".length),
            "base64",
          ),
        );
        const hues = captured.samples.map((rgb: Rgb) => hue(rgb));
        env.evidence.record(label, hues);
        return hues as string[];
      };
      // Retry until a frame shows `expected` at `points`, 60 px per unit.
      const expectItems = async (
        label: string,
        expected: () => Promise<[number, string][]>,
      ) => {
        const deadline = performance.now() + 5_000;
        for (;;) {
          const wanted = await expected();
          const hues = await frame(
            label,
            wanted.map(([y]) => [60, Math.round(y * 60)]),
          );
          const colours = wanted.map(([, colour]) => colour);
          if (JSON.stringify(hues) === JSON.stringify(colours)) return;
          assert.ok(
            performance.now() < deadline,
            `${label}: ${hues.join(",")} not ${colours.join(",")}`,
          );
          await new Promise<void>((resolve) => setTimeout(resolve, 50));
        }
      };

      // The first range declares items 0..4, measured 0.5, 1, 0.5, 1 and
      // 0.5 units tall instead of the 0.75 estimate.
      await expectItems("virtual-initial", async () => [
        [0.25, "red"],
        [1, "green"],
        [1.75, "blue"],
        [2.5, "yellow"],
      ]);

      // Two wheel notches scroll half a unit: item 1 now starts at the top
      // and item 4 fills the bottom.
      await env.page.mouse.move(...page(60, 90));
      await env.page.mouse.wheel(0, 200);
      await expectItems("virtual-wheeled", async () => [
        [0.5, "green"],
        [1.25, "blue"],
        [2, "yellow"],
        [2.75, "red"],
      ]);

      // Dragging the 18 px thumb, which starts below the track's 4.5 px
      // pointed end, 81 px along its 153 px travel scrolls past the middle
      // of 100000 items; the list then declares the items there and paints
      // each by its index, placed from the persisted anchor.
      await env.page.mouse.move(...page(235, 9));
      await env.page.mouse.down();
      await env.page.mouse.move(...page(235, 90), { steps: 6 });
      await env.page.mouse.up();
      const thumb = await frame(
        "virtual-thumb",
        Array.from({ length: 30 }, (_, row) => [235, 3 + row * 6]),
      );
      const covered = thumb.flatMap((colour, row) =>
        colour === "magenta" ? [3 + row * 6] : [],
      );
      assert.ok(
        covered.length > 0 &&
          Math.abs(covered[0]! - 86) <= 10 &&
          Math.abs(covered.at(-1)! - 104) <= 10,
        `virtual-thumb: thumb rows ${covered} not 86..104; ${await env.page.evaluate(
          async (url) => (await import(url)).scrollDiagnostics(),
          fixture,
        )}`,
      );
      const semantics = async () =>
        env.page.evaluate(
          async (url) => (await import(url)).virtualListSemantics(),
          fixture,
        );
      await expectItems("virtual-middle", async () => {
        const node = await semantics();
        const { anchorIndex, anchorOffset } = node;
        const wanted: [number, string][] = [];
        let top = -anchorOffset / 60;
        for (let index = anchorIndex; top < 3; index += 1) {
          const height = index % 2 === 0 ? 0.5 : 1;
          const centre = top + height / 2;
          if (centre > 0.3 && centre < 2.7)
            wanted.push([centre, fills[index % 4]!]);
          top += height;
        }
        return wanted;
      });
      const node = await semantics();
      const ranges = await env.page.evaluate(
        async (url) => (await import(url)).virtualListRanges(),
        fixture,
      );
      env.evidence.record("virtual-ranges", { ranges, node });
      assert.ok(
        node.anchorIndex > 40_000 &&
          node.anchorIndex < 60_000 &&
          node.last - node.first <= 8,
        `virtual-middle: ${JSON.stringify(node)}`,
      );
      assert.ok(
        ranges.at(-1)!.first <= node.anchorIndex &&
          ranges.at(-1)!.last > node.anchorIndex,
        `virtual-middle: last range ${JSON.stringify(
          ranges.at(-1),
          (_, value) => (typeof value === "bigint" ? value.toString() : value),
        )}`,
      );

      const canvasCount = await env.page.evaluate(
        async (url) => (await import(url)).closeScrollCanvas(),
        fixture,
      );
      assert.equal(canvasCount, 0);
    },
  );
});
