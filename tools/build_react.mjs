/** Exercise public React exports in development and production consumers. */
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  artifact,
  bundleBrowser,
  packageVersion,
  workspace,
} from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/react-build");
const fixture = resolve(workspace, "tests/react/fixture.ts");
const guiBrowserFixture = resolve(workspace, "tests/browser/gui-fixture.tsx");
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
    if (
      inputs.some((path) => /\/(web\.tsx|canvas-world-session\.ts)$/.test(path))
    )
      throw new Error("Headless scene consumer included canvas lifecycle code");
    return { environment, ...(await artifact(path)) };
  }),
);
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
