import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/runtime/tsconfig.composed-queries.json",
  ],
  { stdio: "inherit" },
);
const output = resolve(
  process.env.IPP_BUILD_OUTPUT ?? "target/composed-queries",
);
await bundleBrowser(
  "target/integration-artifacts/client/generated.ts",
  resolve(output, "generated.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/runtime/composed-queries.test.ts",
  resolve(output, "composed-queries.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
