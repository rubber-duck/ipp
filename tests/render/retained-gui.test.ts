import test from "node:test";
import { resolve } from "node:path";
import { runRetainedGui } from "./retained-gui-environment.js";

test("retained text remains bounded through streaming, shared panels and recovery", {
  timeout: 180000,
}, async (context) => {
  await runRetainedGui(
    context.signal,
    4,
    resolve("target/integration-artifacts/retained-gui"),
  );
});
