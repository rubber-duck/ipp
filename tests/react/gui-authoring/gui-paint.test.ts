import test from "node:test";
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { parseArgs } from "node:util";
import { runBrowserEnvironment } from "../../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../../harness/hosts.js";
import { encodePng } from "../../harness/images.js";

interface TransitionImage {
  label: string;
  width: number;
  height: number;
  pixels: number[];
  sequence: bigint;
}
interface TransitionFrames {
  ippTransitionImages?: (TransitionImage | null)[];
}

const { values } = parseArgs({
  options: {
    backend: { type: "string" },
    "egl-dir": { type: "string" },
    projected: { type: "string" },
    components: { type: "boolean", default: false },
    sliders: { type: "boolean", default: false },
    "slider-wheel": { type: "boolean", default: false },
  },
});
if (values.backend !== "webgl" && values.backend !== "gles")
  throw new Error("Select --backend webgl or gles");
const native = values.backend === "gles";
const projected = values.projected;
const components = values.components;
const sliders = values.sliders;
const sliderWheel = values["slider-wheel"];
if (
  Number(components) +
    Number(sliders) +
    Number(sliderWheel) +
    Number(projected !== undefined) >
  1
)
  throw new Error(
    "Select one of --components, --sliders, --slider-wheel or --projected",
  );
if (
  projected !== undefined &&
  projected !== "advanced" &&
  projected !== "recovery" &&
  projected !== "transitions"
)
  throw new Error("Select --projected advanced, recovery or transitions");
const instrumentation = projected === "recovery";
const eglDirectory = values["egl-dir"];
if (native && !eglDirectory)
  throw new Error("Native GUI authoring requires --egl-dir");
if (!native && eglDirectory)
  throw new Error("Worker GUI authoring does not use --egl-dir");

for (const variant of native || instrumentation
  ? ["production"]
  : ["development", "production"]) {
  test(`React GUI authoring through ${native ? "native WebSocket/GLES" : "worker WASM/WebGL"} ${variant}${projected ? ` projected ${projected}` : components ? " components" : sliders ? " sliders" : sliderWheel ? " slider wheel" : ""}`, {
    timeout: 240_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const build = presentingBuild(native, {
      directory: native
        ? instrumentation
          ? "target/gles-host-instrumentation"
          : "target/gles-host"
        : instrumentation
          ? "target/browser-build/render-instrumentation"
          : "target/browser-build/render",
      instrumentation,
      name: native ? "gles" : "render",
    });
    async function browser(native?: { url: string; presentationUrl: string }) {
      const result = await runBrowserEnvironment(
        `${native ? "react-gui-gles" : "react-gui-webgl"}-${variant}`,
        {
          workspace,
          build,
          rendering: !native,
          operationTimeoutMs: 210_000,
        },
        context.signal,
        async (environment) => {
          const value = await environment.execute(
            "react-gui-authoring-paint",
            {},
            () =>
              environment.page.evaluate(
                async ({
                  urls,
                  native,
                  variant,
                  projected,
                  components,
                  sliders,
                  sliderWheel,
                }) => {
                  const contract = await import(urls.generated);
                  const scenario = await import(
                    `${urls.origin}/target/react-gui-authoring/paint-${variant}.js`
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
                        { canvas: new OffscreenCanvas(96, 64) },
                      );
                  const host =
                    await contract.IppHostClient.connectTransport(transport);
                  try {
                    const load = async (path: string) =>
                      new Uint8Array(
                        await (
                          await fetch(`${urls.origin}/${path}`)
                        ).arrayBuffer(),
                      );
                    const assets = {
                      font: await load(
                        "target/font-assets/shure-tech-mono.ippf",
                      ),
                      drawing: await load("target/surface-assets/icon.ippd"),
                      bitmap: await load("target/surface-assets/badge.ippt"),
                    };
                    if (projected === "transitions") {
                      const transitions = await scenario
                        .guiLayerTransitions(host, contract)
                        .catch((error: unknown) => ({
                          failure: `Scenario setup or cleanup failed: ${error instanceof Error ? error.message : String(error)}`,
                          images: [],
                          observations: [
                            {
                              error:
                                error instanceof Error
                                  ? error.stack
                                  : String(error),
                            },
                          ],
                        }));
                      // Keep large pixel arrays outside CDP's by-value result graph.
                      // The owned browser transfers one frame at a time below.
                      (globalThis as TransitionFrames).ippTransitionImages =
                        transitions.images;
                      return {
                        transitions: {
                          ...transitions,
                          images: transitions.images.map(
                            ({ pixels, ...image }: TransitionImage) => image,
                          ),
                        },
                      };
                    }
                    if (projected)
                      return {
                        advanced: await scenario.guiProjectedAdvanced(
                          host,
                          contract,
                          assets,
                          projected === "recovery",
                        ),
                      };
                    if (sliderWheel)
                      return {
                        sliderWheel: await scenario.guiSliderWheel(
                          host,
                          contract,
                          assets.font,
                        ),
                      };
                    if (sliders)
                      return {
                        sliders: await scenario.guiSliderDrags(
                          host,
                          contract,
                          assets.font,
                        ),
                      };
                    if (components) {
                      const widgets = await scenario.guiWidgets(
                        host,
                        contract,
                        assets.font,
                      );
                      const retention = widgets.failure
                        ? null
                        : await scenario.guiPresentationRetention(
                            host,
                            contract,
                            assets.font,
                          );
                      const fields =
                        !retention || retention.failure
                          ? null
                          : await scenario.guiFieldObservation(
                              host,
                              contract,
                              assets.font,
                            );
                      const projectedResult =
                        !fields || fields.failure
                          ? null
                          : await scenario.guiProjectedSurfaces(
                              host,
                              contract,
                              assets,
                            );
                      return {
                        components: true,
                        widgets,
                        retention,
                        fields,
                        projected: projectedResult,
                      };
                    }
                    const paint = await scenario.guiPaint(
                      host,
                      contract,
                      assets,
                    );
                    const layers = paint.failure
                      ? null
                      : await scenario.guiLayers(host, contract);
                    const overlays =
                      !layers || layers.failure
                        ? null
                        : await scenario.guiOverlays(host, contract);
                    const projectedResult =
                      !overlays || overlays.failure
                        ? null
                        : await scenario.guiProjectedSurfaces(
                            host,
                            contract,
                            assets,
                          );
                    return {
                      ...paint,
                      layers,
                      overlays,
                      projected: projectedResult,
                    };
                  } finally {
                    await host.close();
                  }
                },
                {
                  urls: environment.urls,
                  native,
                  variant,
                  projected,
                  components,
                  sliders,
                  sliderWheel,
                },
              ),
          );
          if (value?.transitions) {
            const captures = resolve(
              environment.evidence.directory,
              "captures",
            );
            await mkdir(captures, { recursive: true });
            for (
              let index = 0;
              index < value.transitions.images.length;
              index++
            ) {
              const frame = await environment.page.evaluate((index) => {
                const images = (globalThis as TransitionFrames)
                  .ippTransitionImages!;
                const image = images[index]!;
                images[index] = null;
                // A byte string avoids serializing half a million JS numbers.
                let encoded = "";
                for (let start = 0; start < image.pixels.length; start += 8192)
                  encoded += String.fromCharCode(
                    ...image.pixels.slice(start, start + 8192),
                  );
                const { pixels, ...metadata } = image;
                return { ...metadata, pixelsBase64: btoa(encoded) };
              }, index);
              await writeFile(
                resolve(captures, `${frame.label}.png`),
                encodePng({
                  ...frame,
                  pixels: Buffer.from(frame.pixelsBase64, "base64"),
                }),
              );
            }
            await environment.page.evaluate(() => {
              delete (globalThis as TransitionFrames).ippTransitionImages;
            });
          }
          return value;
        },
      );
      assert.ok(result.value, "Browser did not serialize the scenario result");
      const captures = resolve(result.evidenceDirectory, "captures");
      await mkdir(captures, { recursive: true });
      for (const image of [
        ...(result.value.images ?? []),
        ...(result.value.advanced?.images ?? []),
        ...(result.value.advanced?.comparisons ?? []),
        ...(result.value.layers?.images ?? []),
        ...(result.value.overlays?.images ?? []),
        ...(result.value.projected?.images ?? []),
        ...(result.value.widgets?.images ?? []),
        ...(result.value.sliders?.images ?? []),
        ...(result.value.sliderWheel?.images ?? []),
        ...(result.value.retention?.images ?? []),
        ...(result.value.fields?.images ?? []),
      ])
        await writeFile(
          resolve(captures, `${image.label}.png`),
          encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
        );
      if (result.value.transitions) {
        await writeFile(
          resolve(captures, "transition-observations.json"),
          `${JSON.stringify(result.value.transitions.observations, (_, value) => (typeof value === "bigint" ? value.toString() : value), 2)}\n`,
        );
        assert.equal(result.value.transitions.failure, null);
        assert.equal(result.value.transitions.images.length, 31);
        return;
      }
      if (result.value.advanced) {
        await writeFile(
          resolve(captures, "advanced-observations.json"),
          `${JSON.stringify(result.value.advanced.observations, (_, value) => (typeof value === "bigint" ? value.toString() : value), 2)}\n`,
        );
        assert.equal(result.value.advanced.failure, null);
        assert.equal(
          result.value.advanced.images.length,
          instrumentation ? 6 : 16,
        );
        return;
      }
      if (result.value.sliderWheel) {
        await writeFile(
          resolve(captures, "slider-wheel-observations.json"),
          JSON.stringify(result.value.sliderWheel.report, null, 2),
        );
        assert.equal(result.value.sliderWheel.failure, null);
        assert.equal(result.value.sliderWheel.report.length, 4);
        assert.equal(result.value.sliderWheel.images.length, 8);
        return;
      }
      if (result.value.sliders) {
        await writeFile(
          resolve(captures, "slider-observations.json"),
          JSON.stringify(result.value.sliders.report, null, 2),
        );
        assert.equal(result.value.sliders.failure, null);
        assert.equal(result.value.sliders.report.length, 8);
        assert.equal(result.value.sliders.images.length, 12);
        return;
      }
      if (result.value.components) {
        assert.equal(result.value.widgets?.failure, null);
        assert.equal(result.value.widgets?.images.length, 12);
        assert.equal(result.value.retention?.failure, null);
        assert.equal(result.value.retention?.images.length, 3);
        assert.equal(result.value.fields?.failure, null);
        assert.equal(result.value.fields?.images.length, 2);
        assert.equal(result.value.projected?.failure, null);
        assert.equal(result.value.projected?.images.length, 9);
        return;
      }
      if (result.value.colourErrors)
        await writeFile(
          resolve(captures, "colour-fill-errors.json"),
          `${JSON.stringify(result.value.colourErrors, null, 2)}\n`,
        );
      assert.equal(result.value.failure, null);
      assert.equal(result.value.images.length, 13);
      assert.equal(result.value.layers?.failure, null);
      assert.equal(result.value.layers?.images.length, 8);
      assert.equal(result.value.overlays?.failure, null);
      assert.equal(result.value.overlays?.images.length, 5);
      assert.equal(result.value.projected?.failure, null);
      assert.equal(result.value.projected?.images.length, 9);
    }
    await withPresentingHost(
      "react-gui-gles-host",
      build,
      context.signal,
      {
        native,
        ...(eglDirectory === undefined ? {} : { eglDirectory }),
        // Sustained flat/curved drags take about80s on the shared software GLES Host.
        operationTimeoutMs: sliders || sliderWheel ? 180_000 : 100_000,
        missingPresentation: "GLES presentation diagnostics missing",
      },
      browser,
    );
  });
}
