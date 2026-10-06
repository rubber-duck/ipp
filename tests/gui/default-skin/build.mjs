import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../../../tools/build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/gui/default-skin/tsconfig.json",
  ],
  { stdio: "inherit" },
);

const output = resolve(
  process.env.IPP_BUILD_OUTPUT ?? "target/gui-default-skin",
);
await bundleBrowser(
  "tests/gui/default-skin/scenarios/default-skin.ts",
  resolve(output, "scenario.js"),
  "development",
  {
    alias: {
      "@ipp/host-contract": resolve(
        "tests/gui/default-skin/support/host-contract.ts",
      ),
    },
  },
);
await bundleBrowser(
  "tests/gui/default-skin/default-skin.test.ts",
  resolve(output, "default-skin.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "packages/ipp-client/src/wasm-worker.ts",
  resolve(output, "worker.js"),
  "production",
);
