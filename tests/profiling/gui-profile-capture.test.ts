/** Narrow real rendering and semantic profiling ownership coverage. */
import test from "node:test";
import { resolve } from "node:path";
import { runGuiProfileCapture } from "../performance/gui-stress.js";

for (const arrangement of ["browser", "native-gles"] as const) {
  test(`semantic Host profile ownership through ${arrangement} retains World lifetimes and rendered frames`, {
    timeout: 180_000,
  }, async (context) => {
    await runGuiProfileCapture(
      context.signal,
      arrangement,
      resolve(
        `target/integration-artifacts/gui-profile-capture-${arrangement}`,
      ),
      arrangement === "native-gles"
        ? process.env.IPP_EGL_LIBRARY_DIR
        : undefined,
    );
  });
}
