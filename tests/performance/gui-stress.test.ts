import assert from "node:assert/strict";
import test from "node:test";
import { resolve } from "node:path";
import { GUI_STRESS_WORKLOAD } from "../../examples/gui-stress/workload.js";
import { runGuiStress } from "./gui-stress.js";

test("all supported GUI stress cycles target distinct valid virtual items", () => {
  const targets = Array.from(
    { length: 32 },
    (_, cycle) =>
      GUI_STRESS_WORKLOAD.virtualScrollStart +
      cycle * GUI_STRESS_WORKLOAD.virtualScrollStride,
  );
  assert.equal(new Set(targets).size, targets.length);
  assert.ok(
    targets.every(
      (target) =>
        target >= 0 && target < GUI_STRESS_WORKLOAD.virtualItemsPerPanel,
    ),
  );
});

for (const arrangement of ["browser", "native-gles"] as const)
  test(`current React GUI stress workload through ${arrangement} commits every effect and presents meaningful frames`, {
    timeout: 900_000,
  }, async (context) => {
    await runGuiStress(
      context.signal,
      arrangement,
      1,
      resolve(`target/integration-artifacts/gui-stress-${arrangement}`),
      arrangement === "native-gles"
        ? process.env.IPP_EGL_LIBRARY_DIR
        : undefined,
      2,
      false,
      true,
    );
  });
