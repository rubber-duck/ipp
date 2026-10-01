/** Bundle the ordinary GUI panel fixture and its retained GUI exercises. */
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { artifact, bundleBrowser, workspace } from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ??
  resolve(workspace, "target/surface-gui-build");
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/render/tsconfig.retained-gui.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
await mkdir(output, { recursive: true });
const fixture = resolve(output, "fixture.js");
await bundleBrowser(
  resolve(workspace, "tests/render/retained-gui-fixture.tsx"),
  fixture,
);
const artifacts = [await artifact(fixture)];
for (const [source, name] of [
  ["tests/render/retained-gui.test.ts", "retained-gui.test.js"],
  ["tests/render/retained-gui-native.ts", "retained-gui-native.js"],
]) {
  const path = resolve(output, name);
  await bundleBrowser(source, path, "production", {
    platform: "node",
    conditions: ["node"],
    packages: "external",
    define: {},
    minify: false,
  });
  artifacts.push(await artifact(path));
}
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Strict retained GUI panel fixture and its worker and native GLES exercises", artifacts }, null, 2)}\n`,
);
