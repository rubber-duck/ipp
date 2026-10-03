import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "./build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/integration/tsconfig.plots.json",
  ],
  { stdio: "inherit" },
);
const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/plots");
await bundleBrowser(
  "tests/integration/plots-runtime.test.ts",
  resolve(output, "plots-runtime.test.js"),
  "development",
  {
    platform: "node",
    packages: "external",
    // Node's external package resolution omits the bundler's development
    // condition. Keep this real public source entry paired with development
    // React, as the worker driver and the maintained React data fixtures do.
    alias: {
      "@ipp/react": resolve("packages/ipp-react/src/index.ts"),
      "@ipp/react/gui": resolve("packages/ipp-react/src/gui.ts"),
    },
  },
);
await bundleBrowser(
  "tests/integration/plots-driver.ts",
  resolve(output, "plots-driver.js"),
  "development",
);
await bundleBrowser(
  "packages/ipp-react/tests/plots.test.ts",
  resolve(output, "plot-authoring.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
