/** Prove the selected Chromium environment can launch and obtain real WebGL 2. */
import { chromium } from "playwright";
import {
  browserLaunchOptions,
  verifyHardwareRenderer,
} from "./browser-options.mjs";

const options = browserLaunchOptions();
const browser = await chromium.launch(options);
try {
  const page = await browser.newPage();
  const graphics = await page.evaluate(() => {
    const gl = document.createElement("canvas").getContext("webgl2");
    if (!gl) throw new Error("Chromium cannot create WebGL 2");
    const debug = gl.getExtension("WEBGL_debug_renderer_info");
    return {
      version: gl.getParameter(gl.VERSION),
      renderer: gl.getParameter(gl.RENDERER),
      unmaskedRenderer: debug && gl.getParameter(debug.UNMASKED_RENDERER_WEBGL),
    };
  });
  if (process.env.IPP_BROWSER_ANGLE)
    verifyHardwareRenderer(graphics.unmaskedRenderer);
  console.log(
    JSON.stringify({ browser: browser.version(), options, ...graphics }),
  );
} finally {
  await browser.close();
}
