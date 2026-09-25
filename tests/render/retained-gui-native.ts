/**
 * The retained GUI scenario on native GLES through the `gles_host` testing
 * host: `node dist/tests/render/retained-gui-native.js EGL_DIRECTORY`. The
 * pipeline check `check:gles-retained-gui` supplies the directory.
 */
import test from "node:test";
import { resolve } from "node:path";
import { runRetainedGui } from "./retained-gui-environment.js";

const eglDirectory = process.argv[2] ?? process.env.IPP_EGL_LIBRARY_DIR;

test("retained text and GUI shapes match analytic frames on native GLES", {
  timeout: 900000,
}, async (context) => {
  if (!eglDirectory)
    throw new Error(
      "Pass the EGL/GLES library directory or set IPP_EGL_LIBRARY_DIR",
    );
  await runRetainedGui(
    context.signal,
    4,
    resolve("target/integration-artifacts/retained-gui/gles"),
    {},
    { kind: "native-gles", eglDirectory },
  );
});
