import { mkdir } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { bundleBrowser, workspace } from "../build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/blender-headless");
const alias = {
  "@ipp/client": resolve(workspace, "packages/ipp-client/src/index.ts"),
};
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/blender/tsconfig.blender-headless.json",
  ],
  { stdio: "inherit", cwd: workspace },
);
await mkdir(output, { recursive: true });
await bundleBrowser(
  resolve(workspace, "tests/blender/drivers/browser-blender-headless.ts"),
  resolve(output, "scenario.js"),
  "development",
  { alias },
);
await bundleBrowser(
  resolve(workspace, "tests/blender/blender-headless.test.ts"),
  resolve(output, "blender-headless.test.js"),
  "development",
  { platform: "node", packages: "external", alias },
);
await bundleBrowser(
  resolve(workspace, "tests/blender/blender-disk-headless.test.ts"),
  resolve(output, "blender-disk-headless.test.js"),
  "development",
  { platform: "node", packages: "external", alias },
);
