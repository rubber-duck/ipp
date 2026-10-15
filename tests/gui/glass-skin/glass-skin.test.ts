import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../../harness/images.js";
import { runBrowserEnvironment } from "../../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../../harness/hosts.js";
import type { GlassSkinImage } from "./scenarios/glass-skin.js";

declare global {
  interface Window {
    saveGlassSkinCapture(image: GlassSkinImage): Promise<void>;
    recordGlassSkinEvidence(value: object): Promise<void>;
  }
}

/** The shared GUI font built by the `font-assets` product. */
const FONT = "target/font-assets/shure-tech-mono.ippf";

for (const mode of ["worker", "native"] as const) {
  for (const region of [
    "liquid-optics",
    "optical-components",
    "production-optics",
    "panel-backgrounds",
    "optical-context",
    "skin-ownership",
    "appearance",
    "editing",
    "values",
    "reference",
    "focus-scale",
    "material-sharing",
    "composite-values",
    "composite-containers",
    "composite-selection",
    "composite-overlays",
  ] as const) {
    // Full production sheets and the multi-skin contour matrix can keep making
    // progress past four minutes in software WebGL, like the modal overlay cases.
    const extendedBudget =
      region === "composite-overlays" ||
      (mode === "worker" &&
        (region === "production-optics" || region === "composite-selection"));
    test(`glass GUI skin ${mode} ${region}`, {
      timeout: extendedBudget ? 420_000 : 300_000,
    }, async (context) => {
      const native = mode === "native";
      const workspace = resolve(process.cwd());
      const build = presentingBuild(native, {
        directory: native ? "target/gles-host" : "target/browser-build/render",
      });

      async function browser(endpoint?: {
        url: string;
        presentationUrl: string;
      }) {
        await runBrowserEnvironment(
          `gui-glass-skin-${mode}-${region}`,
          {
            workspace,
            build,
            operationTimeoutMs: extendedBudget ? 360_000 : 240_000,
            rendering: !native,
          },
          context.signal,
          async (environment) => {
            const captures = resolve(
              environment.evidence.directory,
              "captures",
            );
            await mkdir(captures, { recursive: true });
            await environment.page.exposeFunction(
              "saveGlassSkinCapture",
              async (image: GlassSkinImage) => {
                await writeFile(
                  resolve(captures, `${image.name}.png`),
                  encodePng({
                    width: image.width,
                    height: image.height,
                    pixels: Uint8Array.from(Buffer.from(image.rgba, "base64")),
                  }),
                );
              },
            );
            await environment.page.exposeFunction(
              "recordGlassSkinEvidence",
              (value: object) =>
                environment.evidence.record("glass-skin", value),
            );
            return environment.execute("glass-skin", { mode }, () =>
              environment.page.evaluate(
                async ({ urls, endpoint, font, region }) => {
                  const contract = await import(urls.generated);
                  // Encode client-owned skin recipes against the runtime under test.
                  Object.assign(globalThis, { ippHostContract: contract });
                  const scenario = await import(
                    `${urls.origin}/target/gui-glass-skin/scenario.js`
                  );
                  const fontBytes = new Uint8Array(
                    await (await fetch(`${urls.origin}/${font}`)).arrayBuffer(),
                  );
                  let transport;
                  if (endpoint) {
                    transport = scenario.nativePresentationTransport(
                      endpoint.url,
                      endpoint.presentationUrl,
                    );
                  } else {
                    const canvas = document.createElement("canvas");
                    canvas.width = 2048;
                    canvas.height = 256;
                    document.body.append(canvas);
                    transport = scenario.workerTransport(
                      `${urls.origin}/target/gui-glass-skin/worker.js`,
                      urls.wasm,
                      contract.MAX_MESSAGE_BYTES,
                      { canvas: canvas.transferControlToOffscreen() },
                    );
                  }
                  const host =
                    await contract.IppHostClient.connectTransport(transport);
                  try {
                    return await scenario.glassSkin(
                      host,
                      contract,
                      fontBytes,
                      {
                        capture: (image: GlassSkinImage) =>
                          window.saveGlassSkinCapture(image),
                        record: (value: object) =>
                          window.recordGlassSkinEvidence(value),
                      },
                      region,
                    );
                  } finally {
                    await host.close();
                  }
                },
                { urls: environment.urls, endpoint, font: FONT, region },
              ),
            );
          },
        );
      }

      await withPresentingHost(
        "gui-glass-skin-gles",
        build,
        context.signal,
        {
          native,
          operationTimeoutMs: 270_000,
          missingPresentation: "GLES presentation endpoint absent",
        },
        browser,
      );
    });
  }
}
