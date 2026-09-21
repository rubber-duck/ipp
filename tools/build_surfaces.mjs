/** Bundle reusable terminal declarations and their real browser exercise. */
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { bundleBrowser, workspace } from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/surface-build");
await mkdir(output, { recursive: true });
await bundleBrowser(
  resolve(workspace, "tests/render/surface-fixture.tsx"),
  resolve(output, "fixture.js"),
);
