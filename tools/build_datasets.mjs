import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "./build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/integration/tsconfig.datasets.json",
  ],
  { stdio: "inherit" },
);
const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/datasets");
await bundleBrowser(
  "tests/integration/datasets.test.ts",
  resolve(output, "datasets.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/data-authoring.test.ts",
  resolve(output, "data-authoring.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/dataset-driver.ts",
  resolve(output, "dataset-driver.js"),
  "development",
);
for (const mode of ["development", "production"])
  await bundleBrowser(
    "packages/ipp-client/src/wasm-worker.ts",
    resolve(output, `dataset-worker-${mode}.js`),
    mode,
  );

// The existing animation scene supplies expression-driver completed-frame evidence.
await bundleBrowser(
  "tests/render/animation.test.ts",
  resolve(output, "animation.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/render/animation-fixture.ts",
  resolve(output, "animation-fixture.js"),
  "development",
);

for (const entry of ["react-data.test", "react-data-driver"])
  await bundleBrowser(
    `tests/integration/${entry}.ts`,
    resolve(output, `${entry}.js`),
    "development",
    entry.endsWith("test") ? { platform: "node", packages: "external" } : {},
  );
