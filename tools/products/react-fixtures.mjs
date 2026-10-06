/** Exercise public React exports in development and production consumers. */
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  artifact,
  bundleBrowser,
  packageVersion,
  workspace,
} from "../build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/react-build");
const fixture = resolve(workspace, "tests/react/pages/react.ts");
const guiBrowserFixture = resolve(workspace, "tests/gui/pages/mounted-gui.tsx");
const guiScrollFixture = resolve(
  workspace,
  "tests/gui/pages/mounted-scroll.tsx",
);
// Browser canvas composition that headless and kit consumers must not bundle.
const canvasSources = [
  "packages/ipp-react/src/web.tsx",
  "packages/ipp-react/src/canvas/ipp-canvas.tsx",
  "packages/ipp-react/src/canvas/world-session.ts",
];
for (const source of canvasSources)
  if (!existsSync(resolve(workspace, source)))
    throw new Error(`Canvas composition guard names missing source ${source}`);
const includesCanvas = (inputs) =>
  inputs.some((path) => canvasSources.some((source) => path.endsWith(source)));
await mkdir(output, { recursive: true });
const builds = await Promise.all(
  ["development", "production"].map(async (environment) => {
    const path = resolve(
      output,
      environment === "development" ? "fixture.js" : "fixture-production.js",
    );
    const result = await bundleBrowser(fixture, path, environment, {
      metafile: true,
    });
    const inputs = Object.keys(result.metafile.inputs);
    if (
      environment === "production" &&
      inputs.some((path) => path.includes("packages/ipp-react/src/"))
    )
      throw new Error("Production fixture bypassed public built React exports");
    if (includesCanvas(inputs))
      throw new Error("Headless scene consumer included canvas lifecycle code");
    return { environment, ...(await artifact(path)) };
  }),
);
const kitConsumer = resolve(
  workspace,
  "tests/react/pages/gui-kit-consumer.tsx",
);
for (const environment of ["development", "production"]) {
  const path = resolve(output, `gui-kit-${environment}.js`);
  const result = await bundleBrowser(kitConsumer, path, environment, {
    metafile: true,
  });
  const inputs = Object.keys(result.metafile.inputs);
  if (
    environment === "production" &&
    inputs.some((path) => path.includes("packages/ipp-react/src/"))
  )
    throw new Error("Production kit consumer bypassed public built exports");
  if (
    environment === "development" &&
    !inputs.some((path) => path.endsWith("packages/ipp-react/src/gui-kit.ts"))
  )
    throw new Error("Development kit consumer missed the kit sources");
  if (includesCanvas(inputs))
    throw new Error("Kit consumer included browser composition code");
  builds.push({
    environment: `gui-kit-${environment}`,
    ...(await artifact(path)),
  });
}
const guiBrowserPath = resolve(output, "gui-fixture.js");
const guiBrowserBuild = await bundleBrowser(
  guiBrowserFixture,
  guiBrowserPath,
  "development",
  {
    metafile: true,
    alias: {
      "@ipp/client": resolve(workspace, "packages/ipp-client/src/index.ts"),
    },
  },
);
builds.push({
  environment: "gui-browser",
  ...(await artifact(guiBrowserPath)),
});
const guiScrollPath = resolve(output, "gui-scroll-fixture.js");
await bundleBrowser(guiScrollFixture, guiScrollPath, "development", {
  alias: {
    "@ipp/client": resolve(workspace, "packages/ipp-client/src/index.ts"),
  },
});
builds.push({
  environment: "gui-scroll-browser",
  ...(await artifact(guiScrollPath)),
});
const packageReport = JSON.parse(
  await readFile(
    resolve(workspace, "packages/ipp-react/dist/build-report.json"),
    "utf8",
  ),
);
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Public React package consumption plus the mounted IppCanvas GUI browser fixture", artifacts: builds, guiBrowserInputs: Object.keys(guiBrowserBuild.metafile.inputs), package: packageReport, dependencies: { react: await packageVersion("react"), reactReconciler: await packageVersion("react-reconciler") } }, null, 2)}\n`,
);
console.log(
  "Built real React scenarios through development and production package exports.",
);
