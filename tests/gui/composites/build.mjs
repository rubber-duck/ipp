import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../../../tools/build/helpers.mjs";

execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "-p",
    "tests/gui/composites/tsconfig.json",
  ],
  { stdio: "inherit" },
);

const output = resolve(process.env.IPP_BUILD_OUTPUT ?? "target/gui-composites");

/** Each part's page module, which the page loads, and its cases' test. */
const PARTS = {
  foundations: [
    "tests/gui/composites/pages/foundations.ts",
    "tests/gui/composites/foundations.test.ts",
  ],
  sliders: [
    "tests/gui/composites/pages/sliders.ts",
    "tests/gui/composites/sliders.test.ts",
  ],
  range: [
    "tests/gui/composites/pages/range.ts",
    "tests/gui/composites/range.test.ts",
  ],
  groups: [
    "tests/gui/composites/pages/groups.ts",
    "tests/gui/composites/groups.test.ts",
  ],
  number: [
    "tests/gui/composites/pages/number.ts",
    "tests/gui/composites/number.test.ts",
  ],
  overlays: [
    "tests/gui/composites/pages/overlays.ts",
    "tests/gui/composites/overlays.test.ts",
  ],
  toast: [
    "tests/gui/composites/pages/toast.ts",
    "tests/gui/composites/toast.test.ts",
  ],
  "kit-choice": [
    "tests/gui/composites/pages/kit-choice.ts",
    "tests/gui/composites/kit-choice.test.ts",
  ],
  "kit-overlays": [
    "tests/gui/composites/pages/kit-overlays.ts",
    "tests/gui/composites/kit-overlays.test.ts",
  ],
  "kit-select": [
    "tests/gui/composites/pages/kit-select.ts",
    "tests/gui/composites/kit-select.test.ts",
  ],
  "kit-values": [
    "tests/gui/composites/pages/kit-values.ts",
    "tests/gui/composites/kit-values.test.ts",
  ],
  colour: [
    "tests/gui/composites/pages/colour.ts",
    "tests/gui/composites/colour.test.ts",
  ],
  "kit-colour": [
    "tests/gui/composites/pages/kit-colour.ts",
    "tests/gui/composites/kit-colour.test.ts",
  ],
};

await Promise.all(
  Object.entries(PARTS).flatMap(([part, [page, cases]]) => [
    bundleBrowser(page, resolve(output, `${part}.js`), "development"),
    bundleBrowser(cases, resolve(output, `${part}.test.js`), "development", {
      platform: "node",
      packages: "external",
    }),
  ]),
);
