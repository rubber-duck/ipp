import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { artifact, bundleBrowser, workspace } from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/headless-client");
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "examples/headless-client/tsconfig.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
await mkdir(output, { recursive: true });
const artifacts = [];
for (const [source, name] of [
  ["examples/headless-client/main.ts", "main.js"],
  ["tests/integration/headless-client.test.ts", "headless-client.test.js"],
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
  `${JSON.stringify({ scope: "Native headless-client CLI and real transport test", artifacts }, null, 2)}\n`,
);
