import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../../harness/images.js";
import { runBrowserEnvironment } from "../../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../../harness/hosts.js";

declare global {
  interface Window {
    saveAgentCapture(
      name: string,
      pixels: number[],
      metadata: object,
    ): Promise<void>;
    recordAgentEvidence(value: object): Promise<void>;
    chooseAgentFile(selector: string): Promise<void>;
  }
}

for (const mode of ["native", "browser"] as const) {
  test(`semantic agent actions ${mode}`, {
    timeout: 180_000,
  }, async (context) => {
    const native = mode === "native";
    const build = presentingBuild(native, {
      directory: native ? "target/gles-host" : "target/browser-build/render",
    });
    await withPresentingHost(
      "gui-agents-gles",
      build,
      context.signal,
      { native, operationTimeoutMs: 120_000 },
      async (endpoint) => {
        await runBrowserEnvironment(
          `gui-agents-${mode}`,
          {
            workspace: resolve(process.cwd()),
            build,
            rendering: !native,
            operationTimeoutMs: 120_000,
          },
          context.signal,
          async (environment) => {
            const captures = resolve(
              environment.evidence.directory,
              "captures",
            );
            await mkdir(captures, { recursive: true });
            await environment.page.exposeFunction(
              "saveAgentCapture",
              async (name: string, pixels: number[], metadata: object) => {
                await writeFile(
                  resolve(captures, `${name}.png`),
                  encodePng({
                    width: 320,
                    height: 400,
                    pixels: Uint8Array.from(pixels),
                  }),
                );
                environment.evidence.record(name, metadata);
              },
            );
            await environment.page.exposeFunction(
              "chooseAgentFile",
              async (selector: string) => {
                const [dialog] = await Promise.all([
                  environment.page.waitForEvent("filechooser"),
                  environment.page.locator(selector).click(),
                ]);
                await dialog.setFiles({
                  name: "notes.txt",
                  mimeType: "text/plain",
                  buffer: Buffer.from("fixture"),
                });
              },
            );
            await environment.page.exposeFunction(
              "recordAgentEvidence",
              (value: object) => environment.evidence.record("agent", value),
            );
            return environment.execute("agent-actions", { mode }, () =>
              environment.page.evaluate(
                async ({ urls, endpoint }) => {
                  const driver = await import(
                    `${urls.origin}/target/gui-agents/driver.js`
                  );
                  return driver.runAgentActions(urls, endpoint, {
                    chooseBrowserFile: (selector: string) =>
                      window.chooseAgentFile(selector),
                    capture: (
                      name: string,
                      frame: { pixels: ArrayBuffer } & object,
                    ) =>
                      window.saveAgentCapture(
                        name,
                        [...new Uint8Array(frame.pixels)],
                        JSON.parse(
                          JSON.stringify(
                            { ...frame, pixels: undefined },
                            (_, value) =>
                              typeof value === "bigint" ? String(value) : value,
                          ),
                        ),
                      ),
                    record: (value: object) =>
                      window.recordAgentEvidence(
                        JSON.parse(JSON.stringify(value)),
                      ),
                  });
                },
                { urls: environment.urls, endpoint },
              ),
            );
          },
        );
      },
    );
  });
}
