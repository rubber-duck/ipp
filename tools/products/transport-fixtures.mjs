import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../build/helpers.mjs";

for (const project of [
  "tests/runtime/tsconfig.transports.json",
  "tests/gui/tsconfig.presentation.json",
])
  execFileSync(
    process.execPath,
    ["node_modules/typescript/bin/tsc", "--project", project],
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
  "tests/gui/scenarios/output-inclusion.ts",
  resolve(output, "output-inclusion.js"),
  "development",
);
await bundleBrowser(
  "tests/gui/output-inclusion.test.ts",
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
  "tests/gui/scenarios/physical-input.ts",
  resolve(output, "physical-input.js"),
  "development",
);
await bundleBrowser(
  "tests/gui/physical-input.test.ts",
  resolve(output, "physical-input.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
for (const [source, name] of [
  [
    "tests/runtime/drivers/browser-lifecycle-targets.ts",
    "lifecycle-target-transport",
  ],
  ["tests/runtime/scenarios/lifecycle-targets.ts", "lifecycle-targets"],
  ["tests/runtime/pages/worker-startup.ts", "worker-startup"],
  ["tests/runtime/pages/task-scheduler.ts", "task-scheduler"],
  ["tests/assets/pages/bulk-read-worker.ts", "bulk-read-worker"],
  ["tests/assets/drivers/browser-asset-exports.ts", "asset-export-browser"],
]) {
  await bundleBrowser(source, resolve(output, `${name}.js`), "development");
}
await bundleBrowser(
  "tests/assets/bulk-read-worker.test.ts",
  resolve(output, "bulk-read-worker.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/runtime/task-scheduler.test.ts",
  resolve(output, "task-scheduler.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/runtime/worker-startup.test.ts",
  resolve(output, "worker-startup.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/runtime/lifecycle-targets.test.ts",
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
  "tests/gui/scenarios/gui-observations.ts",
  resolve(output, "gui-observations.js"),
  "development",
);
await bundleBrowser(
  "tests/runtime/scenarios/multiplex.ts",
  resolve(output, "scenario.js"),
  "development",
);
await bundleBrowser(
  "tests/runtime/drivers/browser-host-transport.ts",
  resolve(output, "host-lifecycle.js"),
  "development",
);
await bundleBrowser(
  "tests/runtime/multiplex.test.ts",
  resolve(output, "multiplex.test.js"),
  "development",
  { platform: "node", packages: "external" },
);

await bundleBrowser(
  "tests/gui/scenarios/gui-local.ts",
  resolve(output, "gui-local.js"),
  "development",
);
await bundleBrowser(
  "tests/gui/drivers/browser-gui-local.ts",
  resolve(output, "gui-local-transport.js"),
  "development",
);
await bundleBrowser(
  "tests/gui/gui-local.test.ts",
  resolve(output, "gui-local.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/gui/scenarios/gui-control-persistence.ts",
  resolve(output, "gui-control-persistence.js"),
  "development",
);
await bundleBrowser(
  "tests/gui/gui-control-persistence.test.ts",
  resolve(output, "gui-control-persistence.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/rendering/scenarios/presentation.ts",
  resolve(output, "presentation.js"),
  "development",
);
await bundleBrowser(
  "tests/rendering/presentation.test.ts",
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
for (const [source, name] of [
  ["tests/gui/drivers/browser-observer-endpoint.ts", "gui-observer-endpoint"],
  [
    "tests/gui/drivers/browser-observer-delivery.ts",
    "worker-observer-delivery",
  ],
]) {
  for (const mode of ["development", "production"]) {
    await bundleBrowser(
      source,
      resolve(
        output,
        `${name}${mode === "production" ? ".production" : ""}.js`,
      ),
      mode,
    );
  }
}

await bundleBrowser(
  "tests/assets/pages/generated-buffers.ts",
  resolve(output, "generated-buffers.js"),
  "development",
);
await bundleBrowser(
  "tests/assets/generated-buffers.test.ts",
  resolve(output, "generated-buffers.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
await bundleBrowser(
  "tests/assets/native-http.test.ts",
  resolve(output, "native-http.test.js"),
  "development",
  { platform: "node", packages: "external" },
);

await bundleBrowser(
  "tests/assets/asset-exports.test.ts",
  resolve(output, "asset-exports.test.js"),
  "development",
  { platform: "node", packages: "external" },
);
