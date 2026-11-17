import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../../../tools/build/helpers.mjs";

execFileSync(
  process.execPath,
  ["node_modules/typescript/bin/tsc", "-p", "tests/gui/agents/tsconfig.json"],
  { stdio: "inherit" },
);
const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/gui-agents");
await bundleBrowser(
  "tests/gui/agents/drivers/browser-agent-actions.ts",
  resolve(output, "driver.js"),
  "development",
);
await bundleBrowser(
  "tests/gui/agents/agent-actions.test.ts",
  resolve(output, "agent-actions.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "packages/ipp-client/src/wasm-worker.ts",
  resolve(output, "worker.js"),
  "development",
);
