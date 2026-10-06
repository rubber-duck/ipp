/**
 * The Node side of the composites harness. `compositeTests` registers one
 * part's cases as a test per host, the worker WASM/WebGL host in Chromium
 * and the native GLES host behind a WebSocket, each in a browser of its own
 * with a fresh Host, so a part never sees another's state and a slow part
 * fails alone. It loads the part's page module into the page, relays
 * Playwright's real pointer and keyboard input to the canvas, reports each
 * case by name with its duration, and keeps the part's outcomes, case
 * timings and captures with the environment's evidence even when a case
 * fails.
 */
import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import type { Page } from "playwright";
import type { GuiObservedEffect } from "@ipp/client";
import { runBrowserEnvironment } from "../../../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../../../harness/hosts.js";
import { encodePng } from "../../../harness/images.js";
import type { CompositePage, CompositeSetup } from "../pages/composites.js";
import { check } from "../../../harness/page/checks.js";

/** The shared GUI font built by the `font-assets` product. */
const FONT = "target/font-assets/shure-tech-mono.ippf";

/** Seconds a Host and a browser may take to start and to clean up. */
const LAUNCH_SECONDS = 60;

/** Seconds a part's Worlds and Host connection may take to close. */
const CLEANUP_SECONDS = 15;

export const json = (value: unknown) =>
  JSON.stringify(value, (_, item: unknown) =>
    typeof item === "bigint" ? item.toString() : item,
  );

export const near = (
  left: readonly number[],
  right: readonly number[],
  tolerance = 0.01,
) =>
  left.length === right.length &&
  left.every((value, index) => Math.abs(value - right[index]!) <= tolerance);

/** The canvas point of a context request, or nothing. */
export const contextPoint = (effect: GuiObservedEffect | undefined) =>
  effect?.effect.kind === "contextRequested" ? [...effect.effect.point] : [];

/** A capture of a part's canvas. */
export interface CompositeImage {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}

/** Pixels of `rect` passing `matches`. */
const pixelsOf =
  (matches: (r: number, g: number, b: number) => boolean) =>
  (image: CompositeImage, [x, y, width, height]: readonly number[]) => {
    let count = 0;
    for (let row = Math.floor(y!); row < y! + height!; row++)
      for (let column = Math.floor(x!); column < x! + width!; column++) {
        const offset = (row * image.width + column) * 4;
        if (
          matches(
            image.pixels[offset]!,
            image.pixels[offset + 1]!,
            image.pixels[offset + 2]!,
          )
        )
          count++;
      }
    return count;
  };

/** Pixels of `rect` in the accent's cyan. */
export const accentPixels = pixelsOf(
  (r, g, b) => r < 100 && g > 150 && b > 150,
);

/** Pixels of `rect` in the amber of the destructive variant. */
export const amberPixels = pixelsOf((r, g, b) => r > 180 && g > 150 && b < 120);

/** An error's message with those of its causes, outermost first. */
function failureChain(error: unknown): string {
  const messages: string[] = [];
  for (
    let current: unknown = error;
    current instanceof Error && messages.length < 6;
    current = current.cause
  ) {
    const line = current.message.split("\n")[0]!;
    if (!messages.some((message) => message.includes(line)))
      messages.push(line);
  }
  return messages.join("\n  caused by: ") || String(error);
}

/** The steps every part's `prepare` returns: the page's own and its part's. */
type Steps = CompositePage["steps"];

type State<S> = typeof globalThis & {
  composite: S;
  closeCompositeHost(): Promise<void>;
};

/** One part's run on one host, as its cases see it. */
export interface CompositeRun<S extends Steps> {
  readonly page: Page;
  readonly native: boolean;
  /** Run one scenario step in the page. */
  step<Name extends keyof S & string>(
    name: Name,
    ...args: S[Name] extends (...values: infer A) => unknown ? A : never
  ): Promise<
    Awaited<S[Name] extends (...values: never[]) => infer R ? R : never>
  >;
  /** The page position of a point of the part's canvas. */
  pagePoint(point: readonly number[]): readonly [number, number];
  /** Page and panel positions of a fraction of a named control's box. */
  at(
    name: string,
    fraction: readonly [number, number],
  ): Promise<{
    page: readonly [number, number];
    panel: readonly [number, number];
  }>;
  /**
   * Page and panel positions of a slider's thumb centre at a fraction of its
   * range, by the runtime's value-to-position mapping: the square thumb,
   * three quarters of the control's smaller side, travels from half a thumb
   * in from the minimum's end, the left or, on a slider taller than it is
   * wide, the bottom.
   */
  thumb(
    name: string,
    fraction: number,
  ): Promise<{
    page: readonly [number, number];
    panel: readonly [number, number];
  }>;
  /**
   * Run one case: a named group of steps and checks whose outcome is kept
   * under its name, timed and reported on failure by name.
   */
  case<T>(name: string, body: () => Promise<T>): Promise<T>;
  /** Capture the presented canvas and keep it with the evidence. */
  capture(name: string): Promise<CompositeImage>;
}

export interface CompositePartOptions {
  /**
   * Seconds this part's setup and cases may take on one host. A part takes
   * at most a sixth of its budget alone under software rendering, so a
   * loaded machine still passes; one that outgrows that is split rather than
   * given a larger budget (see README.md).
   */
  readonly budget: number;
}

/**
 * Register a part: its cases run once through the worker WASM/WebGL host and
 * once through the native WebSocket/GLES host. The part's page module is
 * `target/gui-composites/<part>.js`, exporting `CANVAS` and `prepare`.
 */
export function compositeTests<
  Prepare extends (setup: CompositeSetup) => Promise<Steps>,
>(
  part: string,
  options: CompositePartOptions,
  cases: (run: CompositeRun<Awaited<ReturnType<Prepare>>>) => Promise<void>,
): void {
  type S = Awaited<ReturnType<Prepare>>;
  for (const native of [false, true])
    test(`${part} through ${native ? "native WebSocket/GLES" : "worker WASM/WebGL"}`, {
      timeout: (options.budget + 2 * LAUNCH_SECONDS) * 1000,
    }, async (context) => {
      const host = native ? "gles" : "webgl";
      const workspace = resolve(process.cwd());
      const build = presentingBuild(native, {
        directory: native ? "target/gles-host" : "target/browser-build/render",
      });

      const browser = (endpoint?: { url: string; presentationUrl: string }) =>
        runBrowserEnvironment(
          `composites-${part}-${host}`,
          {
            workspace,
            build,
            operationTimeoutMs: options.budget * 1000,
            rendering: !native,
          },
          context.signal,
          async (environment) => {
            const { page, evidence } = environment;
            const captures = resolve(evidence.directory, "captures");
            await mkdir(captures, { recursive: true });
            const outcomes: Record<string, unknown> = {};
            const timings: {
              name: string;
              seconds: number;
              passed: boolean;
            }[] = [];
            const started = performance.now();
            let ready = started;
            let captured = 0;
            try {
              return await environment.execute(part, { host }, async () => {
                await page.evaluate(
                  async ({ urls, endpoint, part, font }) => {
                    const contract = await import(urls.generated);
                    const scenario = await import(
                      `${urls.origin}/target/gui-composites/${part}.js`
                    );
                    const canvas = document.createElement("canvas");
                    canvas.id = "composite";
                    const { width, height } = scenario.CANVAS;
                    canvas.width = width;
                    canvas.height = height;
                    canvas.style.width = `${width}px`;
                    canvas.style.height = `${height}px`;
                    document.body.append(canvas);
                    const transport = endpoint
                      ? scenario.nativePresentationTransport(
                          endpoint.url,
                          endpoint.presentationUrl,
                        )
                      : scenario.workerTransport(
                          urls.workerScript,
                          urls.wasm,
                          contract.MAX_MESSAGE_BYTES,
                          { canvas: canvas.transferControlToOffscreen() },
                        );
                    const host =
                      await contract.IppHostClient.connectTransport(transport);
                    const state = globalThis as State<unknown>;
                    state.closeCompositeHost = () => host.close();
                    state.composite = await scenario.prepare({
                      host,
                      canvas,
                      fontBytes: await (
                        await fetch(`${urls.origin}/${font}`)
                      ).arrayBuffer(),
                      contract,
                    });
                  },
                  { urls: environment.urls, endpoint, part, font: FONT },
                );
                ready = performance.now();
                const bounds = await page.locator("#composite").boundingBox();
                check(bounds, "The part's canvas is missing");

                const step: CompositeRun<S>["step"] = async (name, ...args) => {
                  try {
                    return (await page.evaluate(
                      ({ name, args }) =>
                        (
                          (globalThis as State<Record<string, unknown>>)
                            .composite[name] as (
                            ...values: unknown[]
                          ) => unknown
                        )(...args),
                      { name, args: args as unknown[] },
                    )) as never;
                  } catch (error) {
                    throw new Error(
                      `Step ${name}(${json(args).slice(1, -1)}): ${
                        error instanceof Error ? error.message : error
                      }`,
                      { cause: error },
                    );
                  }
                };
                const pagePoint = (point: readonly number[]) =>
                  [bounds.x + point[0]!, bounds.y + point[1]!] as const;
                const run: CompositeRun<S> = {
                  page,
                  native,
                  step,
                  pagePoint,
                  async at(name, fraction) {
                    const point = await page.evaluate(
                      ({ name, fraction }) =>
                        (globalThis as State<Steps>).composite.point(
                          name,
                          fraction,
                        ),
                      { name, fraction },
                    );
                    return {
                      page: pagePoint(point.canvas),
                      panel: point.panel,
                    };
                  },
                  async thumb(name, fraction) {
                    const [, , width, height] = await page.evaluate(
                      (name) =>
                        (globalThis as State<Steps>).composite.bounds(name),
                      name,
                    );
                    const edge = 0.75 * Math.min(width, height);
                    return run.at(
                      name,
                      height > width
                        ? [
                            0.5,
                            (height - edge / 2 - fraction * (height - edge)) /
                              height,
                          ]
                        : [(edge / 2 + fraction * (width - edge)) / width, 0.5],
                    );
                  },
                  async case(name, body) {
                    const begun = performance.now();
                    try {
                      const value = await body();
                      outcomes[name] = value;
                      timings.push({
                        name,
                        seconds: (performance.now() - begun) / 1000,
                        passed: true,
                      });
                      return value;
                    } catch (error) {
                      timings.push({
                        name,
                        seconds: (performance.now() - begun) / 1000,
                        passed: false,
                      });
                      throw new Error(
                        `${part} case "${name}" failed: ${
                          error instanceof Error ? error.message : error
                        }`,
                        { cause: error },
                      );
                    }
                  },
                  async capture(name) {
                    const { width, height, rgba } = await page.evaluate(() =>
                      (globalThis as State<Steps>).composite.capture(),
                    );
                    const image = {
                      width,
                      height,
                      pixels: Uint8Array.from(Buffer.from(rgba, "base64")),
                    };
                    await writeFile(
                      resolve(
                        captures,
                        `${String(captured++).padStart(2, "0")}-${name}.png`,
                      ),
                      encodePng(image),
                    );
                    return image;
                  },
                };
                try {
                  await cases(run);
                } finally {
                  outcomes.focusStraddles = await page
                    .evaluate(() =>
                      (globalThis as State<Steps>).composite.focusStraddles(),
                    )
                    .catch(() => undefined);
                }
              });
            } finally {
              const finished = performance.now();
              const timing = {
                part,
                host,
                budget: options.budget,
                setupSeconds: (ready - started) / 1000,
                casesSeconds: (finished - ready) / 1000,
                totalSeconds: (finished - started) / 1000,
                cases: timings,
              };
              await writeFile(
                resolve(evidence.directory, "outcomes.json"),
                json(outcomes),
              );
              await evidence.writeJson("timing.json", timing);
              console.log(
                `gui-composites ${part} ${host}: ${timing.totalSeconds.toFixed(1)} s (setup ${timing.setupSeconds.toFixed(1)} s, budget ${options.budget} s); evidence ${evidence.directory}`,
              );
              // A part that timed out may leave its Host unresponsive; the
              // environment's own cleanup closes the browser regardless.
              let timer: NodeJS.Timeout | undefined;
              await Promise.race([
                page.evaluate(async () => {
                  const state = globalThis as Partial<State<Steps>>;
                  try {
                    await state.composite?.close();
                  } finally {
                    await state.closeCompositeHost?.();
                  }
                }),
                new Promise((_, reject) => {
                  timer = setTimeout(
                    () => reject(new Error("Part cleanup timed out")),
                    CLEANUP_SECONDS * 1000,
                  );
                }),
              ])
                .catch((error: unknown) =>
                  evidence.record("composite_cleanup_error", { error }),
                )
                .finally(() => clearTimeout(timer));
            }
          },
        );

      try {
        await withPresentingHost(
          `composites-${part}-gles-host`,
          build,
          context.signal,
          {
            native,
            readinessTimeoutMs: LAUNCH_SECONDS * 1000,
            operationTimeoutMs: (options.budget + LAUNCH_SECONDS) * 1000,
            missingPresentation: "GLES presentation endpoint absent",
          },
          browser,
        );
      } catch (error) {
        // The environments report where their evidence is; the case that
        // failed and why are causes further down.
        throw new Error(failureChain(error), { cause: error });
      }
    });
}
