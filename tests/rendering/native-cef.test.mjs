/** Real CEF helper processes, generated WebSocket client, CanvasHtml and GLES. */
import test from "node:test";
import { mkdir, mkdtemp } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

test("macOS CEF frames and input reach immutable CanvasHtml publications", {
  skip: process.platform !== "darwin",
  timeout: 120_000,
}, async () => {
  const parent = resolve("target/integration-artifacts/native-cef");
  await mkdir(parent, { recursive: true });
  const evidence = await mkdtemp(join(parent, "run-"));
  await build({
    entryPoints: ["tests/rendering/drivers/native-cef.ts"],
    outfile: join(evidence, "driver.mjs"),
    bundle: true,
    platform: "node",
    format: "esm",
    packages: "external",
    sourcemap: true,
  });
  const { withNativeCef } = await import(
    pathToFileURL(join(evidence, "driver.mjs")).href
  );
  const { nativeCefScenario } = await import("./scenarios/native-cef.mjs");
  await withNativeCef(evidence, nativeCefScenario);
});
