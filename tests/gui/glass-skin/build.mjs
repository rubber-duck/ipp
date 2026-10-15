import { copyFile } from "node:fs/promises";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../../../tools/build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/gui/glass-skin/tsconfig.json",
  ],
  { stdio: "inherit" },
);

const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/gui-glass-skin");
await bundleBrowser(
  "tests/gui/glass-skin/scenarios/glass-skin.tsx",
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
  "tests/gui/glass-skin/glass-skin.test.ts",
  resolve(output, "glass-skin.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/gui/glass-skin/preview.browser.test.ts",
  resolve(output, "preview.browser.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "packages/ipp-client/src/wasm-worker.ts",
  resolve(output, "worker.js"),
  "production",
);

await bundleBrowser(
  "tests/gui/glass-skin/optical-material/preview.ts",
  resolve(output, "optical-preview.js"),
  "development",
);
await copyFile(
  "tests/gui/glass-skin/optical-material/preview.html",
  resolve(output, "optical-preview.html"),
);
