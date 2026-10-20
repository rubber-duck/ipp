import assert from "node:assert/strict";
import { setImmediate } from "node:timers/promises";
import test from "node:test";
import { withHtmlCanvas } from "./drivers/browser-html-canvas.js";
import {
  exerciseHtmlCanvas,
  exerciseHtmlCanvasDevice,
} from "./scenarios/html-canvas.js";

test("HTML canvas GPU bridge preserves live content and exact retained frames", {
  timeout: 60_000,
}, async (context) => {
  const result = await withHtmlCanvas(true, exerciseHtmlCanvas);
  context.diagnostic(JSON.stringify(result));
});

test("HTML canvas unavailable without experimental capability", {
  timeout: 30_000,
}, async (context) => {
  const result = await withHtmlCanvas(false, async () => {
    throw new Error("Unavailable producer must not run");
  });
  context.diagnostic(JSON.stringify(result));
});

test("HTML canvas runner cleans early launch failures", {
  timeout: 10_000,
}, async () => {
  const before = process
    .getActiveResourcesInfo()
    .filter((value) => value === "TCPServerWrap").length;
  const angle = process.env.IPP_BROWSER_ANGLE;
  process.env.IPP_BROWSER_ANGLE = "invalid-device";
  try {
    await assert.rejects(
      withHtmlCanvas(false, async () => {}),
      /IPP_BROWSER_ANGLE must be/,
    );
  } finally {
    if (angle === undefined) delete process.env.IPP_BROWSER_ANGLE;
    else process.env.IPP_BROWSER_ANGLE = angle;
  }
  for (
    let attempt = 0;
    attempt < 8 &&
    process
      .getActiveResourcesInfo()
      .filter((value) => value === "TCPServerWrap").length !== before;
    attempt++
  )
    await setImmediate();
  assert.equal(
    process
      .getActiveResourcesInfo()
      .filter((value) => value === "TCPServerWrap").length,
    before,
  );
});

test("HTML canvas device imports preserve pixels, state and bounded lifetimes", {
  timeout: 60_000,
}, async (context) => {
  const result = await withHtmlCanvas(true, exerciseHtmlCanvasDevice, true);
  context.diagnostic(JSON.stringify(result));
});
