/** Build real canvas consumers through the public React package exports. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import {
  artifact,
  bundleBrowser,
  packageVersion,
  workspace,
} from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/canvas-build");
const fixture = resolve(workspace, "tests/render/canvas-fixture.tsx");
await mkdir(output, { recursive: true });
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/render/tsconfig.canvas.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
const artifacts = await Promise.all(
  ["development", "production"].map(async (environment) => {
    const path = resolve(
      output,
      environment === "development" ? "fixture.js" : "fixture-production.js",
    );
    const result = await bundleBrowser(fixture, path, environment, {
      metafile: true,
    });
    if (
      environment === "production" &&
      Object.keys(result.metafile.inputs).some((path) =>
        path.includes("packages/ipp-react/src/"),
      )
    )
      throw new Error("Production canvas bypassed public built React exports");
    return { environment, ...(await artifact(path)) };
  }),
);
for (const name of ["canvas", "dpi", "canvas-controller"]) {
  const path = resolve(output, `${name}.test.js`);
  await bundleBrowser(`tests/render/${name}.test.ts`, path, "production", {
    platform: "node",
    packages: "external",
    define: {},
  });
  artifacts.push({ environment: "node", ...(await artifact(path)) });
}
const controller = resolve(output, "controller.js");
const controllerBuild = await bundleBrowser(
  "tests/render/canvas-controller.tsx",
  controller,
  "production",
  {
    metafile: true,
    alias: {
      "@ipp/react": resolve(workspace, "packages/ipp-react/src/index.ts"),
      "@ipp/react/web": resolve(workspace, "packages/ipp-react/src/web.tsx"),
    },
  },
);
const controllerInputs = Object.keys(controllerBuild.metafile.inputs);
if (
  controllerInputs.filter(
    (path) => path === "packages/ipp-react/src/commits.ts",
  ).length !== 1 ||
  controllerInputs.some((path) => path.includes("ipp-react/dist/"))
)
  throw new Error(
    "Instrumented controller must share one source commit module with its real reconciler",
  );
await writeFile(
  resolve(output, "controller-inputs.json"),
  `${JSON.stringify(controllerInputs, null, 2)}\n`,
);
artifacts.push({ environment: "controller", ...(await artifact(controller)) });
const units = resolve(output, "canvas-presentation.test.js");
await bundleBrowser(
  "packages/ipp-react/tests/canvas-presentation.test.ts",
  units,
  "production",
  { platform: "node", packages: "external" },
);
artifacts.push({ environment: "units", ...(await artifact(units)) });
const worldUnits = resolve(output, "canvas-world.test.js");
await bundleBrowser(
  "packages/ipp-react/tests/canvas-world.test.ts",
  worldUnits,
  "production",
  { platform: "node", packages: "external" },
);
artifacts.push({ environment: "units", ...(await artifact(worldUnits)) });
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Public IppCanvas and World through generated client, worker, WASM, and WebGL", dependencies: { react: await packageVersion("react"), reactDom: await packageVersion("react-dom"), reactReconciler: await packageVersion("react-reconciler") }, artifacts }, null, 2)}\n`,
);
console.log("Built development and production declarative canvas fixtures.");
