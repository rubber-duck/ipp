import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { artifact, bundleBrowser, workspace } from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/gui-stress");
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--ignoreConfig",
    "--strict",
    "--target",
    "ES2023",
    "--module",
    "NodeNext",
    "--moduleResolution",
    "NodeNext",
    "--lib",
    "ES2023,DOM",
    "--declaration",
    "--emitDeclarationOnly",
    "--outDir",
    "target/gui-stress-contract",
    "target/integration-artifacts/client/generated.ts",
  ],
  { cwd: workspace, stdio: "inherit" },
);
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/performance/tsconfig.gui-stress.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
await mkdir(output, { recursive: true });
const artifacts = [];
for (const name of ["gui-stress-fixture", "gui-stress", "gui-stress.test"]) {
  const path = resolve(output, `${name}.js`);
  await bundleBrowser(
    `tests/performance/${name}.${name.endsWith("fixture") ? "tsx" : "ts"}`,
    path,
    "production",
    name.endsWith("fixture")
      ? {}
      : {
          platform: "node",
          conditions: ["node"],
          packages: "external",
          define: {},
          minify: false,
        },
  );
  artifacts.push(await artifact(path));
}
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify(
    {
      scope: "Frozen GUI stress scene and real transport correctness drivers",
      artifacts,
    },
    null,
    2,
  )}\n`,
);
