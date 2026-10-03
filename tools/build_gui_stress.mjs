import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
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
for (const name of [
  "gui-stress-fixture",
  "gui-stress",
  "gui-stress.test",
  "gui-profile-capture.test",
]) {
  const path = resolve(output, `${name}.js`);
  const built = await bundleBrowser(
    `tests/performance/${name}.${name.endsWith("fixture") ? "tsx" : "ts"}`,
    path,
    "production",
    name.endsWith("fixture")
      ? {
          tsconfig: resolve(
            workspace,
            "tests/performance/tsconfig.gui-stress.json",
          ),
          metafile: true,
        }
      : {
          platform: "node",
          conditions: ["node"],
          packages: "external",
          define: {},
          minify: false,
        },
  );
  artifacts.push(await artifact(path));
  if (name.endsWith("fixture"))
    await writeFile(
      resolve(output, "fixture-inputs.json"),
      `${JSON.stringify({ inputs: Object.keys(built.metafile.inputs).sort() }, null, 2)}\n`,
    );
}
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify(
    {
      scope: "Frozen GUI stress scene and real transport correctness drivers",
      artifacts,
      fixtureSources: await Promise.all(
        [
          "examples/gui-stress/workload.ts",
          "examples/gui-stress/scene.tsx",
          "examples/gui-stress/diagnostic-panel.tsx",
          "tests/performance/gui-stress-fixture.tsx",
          "tests/integration/camera-fixtures.ts",
          "tests/integration/system-selections.ts",
          "tests/integration/gui-actions.ts",
        ].map(async (path) => ({
          path,
          sha256: createHash("sha256")
            .update(await readFile(resolve(workspace, path)))
            .digest("hex"),
        })),
      ),
    },
    null,
    2,
  )}\n`,
);
