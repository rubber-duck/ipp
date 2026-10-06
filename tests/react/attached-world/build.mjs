import { existsSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import {
  artifact,
  bundleBrowser,
  workspace,
} from "../../../tools/build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/react-attached");
// Browser canvas composition that the headless attachment must not bundle.
const canvasSources = [
  "packages/ipp-react/src/web.tsx",
  "packages/ipp-react/src/canvas/ipp-canvas.tsx",
  "packages/ipp-react/src/canvas/world-session.ts",
];
for (const source of canvasSources)
  if (!existsSync(resolve(workspace, source)))
    throw new Error(`Canvas composition guard names missing source ${source}`);
await mkdir(output, { recursive: true });
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/react/attached-world/tsconfig.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
const artifacts = [];
for (const variant of ["development", "production"]) {
  const path = resolve(output, `fixture-${variant}.js`);
  const result = await bundleBrowser(
    "tests/react/attached-world/pages/attached-world.ts",
    path,
    variant,
    { metafile: true },
  );
  const inputs = Object.keys(result.metafile.inputs);
  if (
    inputs.some((path) => canvasSources.some((source) => path.endsWith(source)))
  )
    throw new Error("Headless attachment fixture includes Canvas integration");
  if (
    variant === "production" &&
    inputs.some((path) => path.includes("packages/ipp-react/src/"))
  )
    throw new Error(
      "Production attachment fixture bypasses public built exports",
    );
  artifacts.push({ variant, ...(await artifact(path)), inputs });
}
for (const [source, name] of [
  [
    "tests/react/attached-world/scenarios/attached-world.ts",
    "scenarios/attached-world",
  ],
  [
    "tests/react/attached-world/scenarios/attached-world-closures.ts",
    "scenarios/attached-world-closures",
  ],
  [
    "tests/react/attached-world/scenarios/attached-world-journals.ts",
    "scenarios/attached-world-journals",
  ],
  [
    "tests/react/attached-world/scenarios/attached-world-recovery.ts",
    "scenarios/attached-world-recovery",
  ],
  [
    "tests/react/attached-world/scenarios/canvas-world.ts",
    "scenarios/canvas-world",
  ],
  ["tests/react/attached-world/attached-world.test.ts", "attached-world.test"],
]) {
  const path = resolve(output, `${name}.js`);
  await bundleBrowser(source, path, "production", {
    platform: "node",
    packages: "external",
    define: {},
    external: [
      "./scenarios/attached-world.js",
      "./scenarios/attached-world-closures.js",
      "./scenarios/attached-world-journals.js",
      "./scenarios/attached-world-recovery.js",
      "./scenarios/canvas-world.js",
    ],
  });
  artifacts.push(await artifact(path));
}
const unit = resolve(output, "attachment-journal.test.js");
await bundleBrowser(
  "packages/ipp-react/tests/attachment-journal.test.ts",
  unit,
  "production",
  { platform: "node", packages: "external" },
);
artifacts.push(await artifact(unit));
const renderer = resolve(output, "renderer.test.js");
await bundleBrowser(
  "packages/ipp-react/tests/renderer.test.ts",
  renderer,
  "development",
  { platform: "node", packages: "external" },
);
artifacts.push(await artifact(renderer));
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Headless React AttachedWorld, public native and browser consumers, no presentation", artifacts }, null, 2)}\n`,
);
