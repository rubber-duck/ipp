import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import {
  artifact,
  bundleBrowser,
  workspace,
} from "../../tools/build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/react-attached");
await mkdir(output, { recursive: true });
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/react/tsconfig.attached-world.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
const artifacts = [];
for (const variant of ["development", "production"]) {
  const path = resolve(output, `fixture-${variant}.js`);
  const result = await bundleBrowser(
    "tests/react/attached-world-fixture.ts",
    path,
    variant,
    { metafile: true },
  );
  const inputs = Object.keys(result.metafile.inputs);
  if (
    inputs.some((path) => /\/(web\.tsx|canvas-world-session\.ts)$/.test(path))
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
for (const name of [
  "attached-world-case",
  "attached-world-closures",
  "attached-world-journals",
  "attached-world-recovery",
  "canvas-world-case",
  "attached-world.test",
]) {
  const path = resolve(output, `${name}.js`);
  await bundleBrowser(`tests/react/${name}.ts`, path, "production", {
    platform: "node",
    packages: "external",
    define: {},
    external: [
      "./attached-world-case.js",
      "./attached-world-closures.js",
      "./attached-world-journals.js",
      "./attached-world-recovery.js",
      "./canvas-world-case.js",
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
