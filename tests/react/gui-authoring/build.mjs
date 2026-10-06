import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import {
  artifact,
  bundleBrowser,
  workspace,
} from "../../../tools/build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ??
  resolve(workspace, "target/react-gui-authoring");
await mkdir(output, { recursive: true });
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
    "target/react-gui-contract",
    "target/integration-artifacts/client/generated.ts",
  ],
  { cwd: workspace, stdio: "inherit" },
);
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/react/gui-authoring/tsconfig.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
const artifacts = [];
for (const variant of ["development", "production"]) {
  const path = resolve(output, `fixture-${variant}.js`);
  const result = await bundleBrowser(
    "tests/react/gui-authoring/pages/gui-authoring.tsx",
    path,
    variant,
    { metafile: true },
  );
  const inputs = Object.keys(result.metafile.inputs);
  if (
    variant === "production" &&
    inputs.some((path) => path.includes("packages/ipp-react/src/"))
  )
    throw new Error("Production GUI fixture bypasses public built exports");
  artifacts.push({ variant, inputs, ...(await artifact(path)) });
  const paint = resolve(output, `paint-${variant}.js`);
  const painted = await bundleBrowser(
    "tests/react/gui-authoring/pages/gui-paint.tsx",
    paint,
    variant,
    { metafile: true },
  );
  const paintInputs = Object.keys(painted.metafile.inputs);
  if (
    variant === "production" &&
    paintInputs.some((path) => path.includes("packages/ipp-react/src/"))
  )
    throw new Error(
      "Production projected fixture bypasses public built exports",
    );
  artifacts.push({ variant, inputs: paintInputs, ...(await artifact(paint)) });
}
for (const [source, name] of [
  ["tests/react/gui-authoring/pages/gui-authoring.tsx", "pages/gui-authoring"],
  ["tests/react/gui-authoring/gui-root.test.ts", "gui-authoring.test"],
]) {
  const path = resolve(output, `${name}.js`);
  await bundleBrowser(source, path, "production", {
    platform: "node",
    packages: "external",
    define: {},
    external: ["./pages/gui-authoring.js"],
  });
  artifacts.push(await artifact(path));
}
const oracleTest = resolve(output, "projected-oracle.test.js");
await bundleBrowser(
  "tests/react/gui-authoring/projected-oracle.test.ts",
  oracleTest,
  "production",
  { platform: "node", packages: "external", define: {} },
);
artifacts.push(await artifact(oracleTest));
const paintTest = resolve(output, "gui-paint.test.js");
await bundleBrowser(
  "tests/react/gui-authoring/gui-paint.test.ts",
  paintTest,
  "production",
  {
    platform: "node",
    packages: "external",
    define: {},
  },
);
artifacts.push(await artifact(paintTest));
for (const name of ["control-refs", "gui-declaration", "gui-callbacks"]) {
  const path = resolve(output, `${name}.test.js`);
  await bundleBrowser(
    `packages/ipp-react/tests/${name}.test.ts`,
    path,
    "development",
    {
      platform: "node",
      packages: "external",
    },
  );
  artifacts.push(await artifact(path));
}
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Ordinary React GUI authoring using real native and WASM contracts", artifacts }, null, 2)}\n`,
);
