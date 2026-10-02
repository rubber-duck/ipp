import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../../../tools/build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/integration/gui-composites/tsconfig.json",
  ],
  { stdio: "inherit" },
);

const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/gui-composites");

/** Each part's scenario module, which the page loads, and its cases' test. */
const PARTS = {
  foundations: [
    "tests/integration/gui-composites/foundations.ts",
    "tests/integration/gui-composites/foundations.test.ts",
  ],
  sliders: [
    "tests/integration/gui-composites/sliders.ts",
    "tests/integration/gui-composites/sliders.test.ts",
  ],
  range: [
    "tests/integration/gui-composites/range.ts",
    "tests/integration/gui-composites/range.test.ts",
  ],
  groups: [
    "tests/integration/gui-composites/groups.ts",
    "tests/integration/gui-composites/groups.test.ts",
  ],
  number: [
    "tests/integration/gui-composites/number.ts",
    "tests/integration/gui-composites/number.test.ts",
  ],
  overlays: [
    "tests/integration/gui-composites/overlays.ts",
    "tests/integration/gui-composites/overlays.test.ts",
  ],
  toast: [
    "tests/integration/gui-composites/toast.ts",
    "tests/integration/gui-composites/toast.test.ts",
  ],
  "kit-choice": [
    "tests/integration/gui-composites/kit-choice.ts",
    "tests/integration/gui-composites/kit-choice.test.ts",
  ],
  "kit-overlays": [
    "tests/integration/gui-composites/kit-overlays.ts",
    "tests/integration/gui-composites/kit-overlays.test.ts",
  ],
  "kit-select": [
    "tests/integration/gui-composites/kit-select.ts",
    "tests/integration/gui-composites/kit-select.test.ts",
  ],
  "kit-values": [
    "tests/integration/gui-composites/kit-values.ts",
    "tests/integration/gui-composites/kit-values.test.ts",
  ],
  colour: [
    "tests/integration/gui-composites/colour.ts",
    "tests/integration/gui-composites/colour.test.ts",
  ],
  "kit-colour": [
    "tests/integration/gui-composites/kit-colour.ts",
    "tests/integration/gui-composites/kit-colour.test.ts",
  ],
};

await Promise.all(
  Object.entries(PARTS).flatMap(([part, [scenario, cases]]) => [
    bundleBrowser(scenario, resolve(output, `${part}.js`), "development"),
    bundleBrowser(cases, resolve(output, `${part}.test.js`), "development", {
      platform: "node",
      packages: "external",
    }),
  ]),
);
