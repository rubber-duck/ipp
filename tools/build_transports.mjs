import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "./build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/integration/tsconfig.presentation.json",
  ],
  { stdio: "inherit" },
);

const output = resolve(
  process.env.IPP_BUILD_OUTPUT ?? "target/multiplex-tests",
);
for (const mode of ["development", "production"]) {
  await bundleBrowser(
    "packages/ipp-client/src/wasm-worker.ts",
    resolve(output, `presentation-worker-${mode}.js`),
    mode,
  );
}
await bundleBrowser(
  "tests/integration/scenarios/output-inclusion.ts",
  resolve(output, "output-inclusion.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/output-inclusion.test.ts",
  resolve(output, "output-inclusion.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
for (const name of [
  "gui-input",
  "gui-text-bridge",
  "gui-ime",
  "gui-clipboard",
  "gui-soft-keyboard",
]) {
  await bundleBrowser(
    `packages/ipp-react/tests/${name}.test.ts`,
    resolve(output, `${name}.test.js`),
    "development",
    { platform: "node", packages: "external" },
  );
}
await bundleBrowser(
  "tests/integration/scenarios/physical-input.ts",
  resolve(output, "physical-input.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/physical-input.test.ts",
  resolve(output, "physical-input.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
for (const name of [
  "lifecycle-target-transport",
  "scenarios/lifecycle-targets",
  "worker-startup",
  "task-scheduler",
  "bulk-read-worker",
  "asset-export-browser",
]) {
  await bundleBrowser(
    `tests/integration/${name}.ts`,
    resolve(output, `${name.split("/").at(-1)}.js`),
    "development",
  );
}
await bundleBrowser(
  "tests/integration/bulk-read-worker.test.ts",
  resolve(output, "bulk-read-worker.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/task-scheduler.test.ts",
  resolve(output, "task-scheduler.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/worker-startup.test.ts",
  resolve(output, "worker-startup.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/lifecycle-targets.test.ts",
  resolve(output, "lifecycle-targets.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
for (const mode of ["development", "production"]) {
  await bundleBrowser(
    "packages/ipp-client/src/wasm-worker.ts",
    resolve(output, `lifecycle-worker-${mode}.js`),
    mode,
  );
}
await bundleBrowser(
  "tests/integration/scenarios/gui-observations.ts",
  resolve(output, "gui-observations.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/scenarios/multiplex.ts",
  resolve(output, "scenario.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/host-transport-driver.ts",
  resolve(output, "host-lifecycle.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/multiplex.test.ts",
  resolve(output, "multiplex.test.js"),
  "development",
  { platform: "node", packages: "external" },
);

await bundleBrowser(
  "tests/integration/scenarios/gui-local.ts",
  resolve(output, "gui-local.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/gui-local-transport.ts",
  resolve(output, "gui-local-transport.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/gui-local.test.ts",
  resolve(output, "gui-local.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/scenarios/gui-control-persistence.ts",
  resolve(output, "gui-control-persistence.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/gui-control-persistence.test.ts",
  resolve(output, "gui-control-persistence.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/scenarios/presentation.ts",
  resolve(output, "presentation.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/presentation.test.ts",
  resolve(output, "presentation.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "packages/ipp-client/tests/render-diagnostics.test.ts",
  resolve(output, "render-diagnostics.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "packages/ipp-client/tests/gui-semantics.test.ts",
  resolve(output, "gui-semantics.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
for (const name of ["transport", "worker-connections", "lifecycle-watches"]) {
  await bundleBrowser(
    `packages/ipp-client/tests/${name}.test.ts`,
    resolve(output, `${name}.test.js`),
    "development",
    { platform: "node", packages: "external" },
  );
}
await bundleBrowser(
  "packages/ipp-client/tests/physical-input.test.ts",
  resolve(output, "physical-input-sdk.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
for (const name of ["gui-observer-endpoint", "worker-observer-delivery"]) {
  for (const mode of ["development", "production"]) {
    await bundleBrowser(
      `tests/integration/${name}.ts`,
      resolve(
        output,
        `${name}${mode === "production" ? ".production" : ""}.js`,
      ),
      mode,
    );
  }
}

await bundleBrowser(
  "tests/integration/generated-buffers.ts",
  resolve(output, "generated-buffers.js"),
  "development",
);
await bundleBrowser(
  "tests/integration/generated-buffers.test.ts",
  resolve(output, "generated-buffers.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/integration/native-http.test.ts",
  resolve(output, "native-http.test.js"),
  "development",
  { platform: "node", packages: "external" },
);

await bundleBrowser(
  "tests/integration/asset-exports.test.ts",
  resolve(output, "asset-exports.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
