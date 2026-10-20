import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { bundleBrowser } from "../build/helpers.mjs";
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/rendering/tsconfig.html-canvas.json",
  ],
  { stdio: "inherit" },
);
const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/html-canvas");
await Promise.all([
  ...[
    "html-canvas",
    "html-canvas-worker",
    "html-canvas-device-worker",
    "static-html",
  ].map((name) =>
    bundleBrowser(
      `tests/rendering/pages/${name}.ts`,
      resolve(output, `${name}.js`),
      "development",
    ),
  ),
  bundleBrowser(
    "tests/rendering/static-html.browser.test.ts",
    resolve(output, "static-html.browser.test.js"),
    "development",
    { platform: "node", packages: "external" },
  ),
  bundleBrowser(
    "tests/rendering/html-canvas.browser.test.ts",
    resolve(output, "html-canvas.browser.test.js"),
    "development",
    { platform: "node", packages: "external" },
  ),
]);
