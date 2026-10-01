import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../../../tools/build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/integration/gui-motion/tsconfig.json",
  ],
  { stdio: "inherit" },
);

const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/gui-motion");
await bundleBrowser(
  "tests/integration/gui-motion/scenario.ts",
  resolve(output, "scenario.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/gui-motion/environment.test.ts",
  resolve(output, "environment.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
for (const mode of ["development", "production"]) {
  await bundleBrowser(
    "packages/ipp-client/src/wasm-worker.ts",
    resolve(output, `worker-${mode}.js`),
    mode,
  );
}
