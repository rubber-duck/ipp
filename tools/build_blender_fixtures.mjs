import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { artifact, bundleBrowser, workspace } from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/blender-test");
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/render/tsconfig.blender-viewer.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
await mkdir(output, { recursive: true });
const artifacts = [];
for (const [name, source] of [
  ["blender-fixture", "tests/render/blender-fixture.tsx"],
  ["blender-disk-fixture", "tests/render/blender-disk-fixture.ts"],
]) {
  const path = resolve(output, `${name}.js`);
  await bundleBrowser(source, path);
  artifacts.push(await artifact(path));
}
for (const name of ["blender", "blender-stream", "blender-disk"]) {
  const path = resolve(output, `${name}.test.js`);
  await bundleBrowser(`tests/render/${name}.test.ts`, path, "production", {
    platform: "node",
    packages: "external",
    define: {},
  });
  artifacts.push(await artifact(path));
}
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Strict live Blender viewer, streaming and disk presentation callers", artifacts }, null, 2)}\n`,
);
