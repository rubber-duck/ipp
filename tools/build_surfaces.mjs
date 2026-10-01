/** Bundle reusable terminal declarations and their real browser exercise. */
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { artifact, bundleBrowser, workspace } from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/surface-build");
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/render/tsconfig.surfaces.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
await mkdir(output, { recursive: true });
const fixture = resolve(output, "fixture.js");
await bundleBrowser(
  resolve(workspace, "tests/render/surface-fixture.tsx"),
  fixture,
);
const artifacts = [await artifact(fixture)];
for (const [source, name] of [
  ["tests/integration/surface.test.ts", "lifecycle.test.js"],
  ["tests/render/surface.test.ts", "surface.test.js"],
  ["tests/render/surface-cache.test.ts", "surface-cache.test.js"],
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
  `${JSON.stringify({ scope: "Strict Surface Canvas lifecycle and rendering fixtures", artifacts }, null, 2)}\n`,
);
