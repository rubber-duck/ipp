import test from "node:test";
import { resolve } from "node:path";
import { runRetainedGui } from "./retained-gui-environment.js";

test("retained text and GUI shapes stay bounded, evict under atlas pressure and match analytic frames", {
  timeout: 600000,
}, async (context) => {
  await runRetainedGui(
    context.signal,
    4,
    resolve("target/integration-artifacts/retained-gui"),
  );
});
