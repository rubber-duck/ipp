import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/data/tsconfig.datasets.json",
  ],
  { stdio: "inherit" },
);
const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/datasets");
await bundleBrowser(
  "tests/data/datasets.test.ts",
  resolve(output, "datasets.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/data/data-authoring.test.ts",
  resolve(output, "data-authoring.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/data/drivers/browser-datasets.ts",
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
  "tests/rendering/animation.test.ts",
  resolve(output, "animation.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/rendering/pages/animation.ts",
  resolve(output, "pages/animation.js"),
  "development",
);

for (const [source, entry] of [
  ["tests/data/react-data.test.ts", "react-data.test"],
  ["tests/data/drivers/browser-react-data.ts", "react-data-driver"],
])
  await bundleBrowser(
    source,
    resolve(output, `${entry}.js`),
    "development",
    entry.endsWith("test") ? { platform: "node", packages: "external" } : {},
  );
