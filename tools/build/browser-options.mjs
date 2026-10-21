import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import toolchains from "../pipeline/toolchains.json" with { type: "json" };

export const browserPin = toolchains.chromeForTesting;

/** Keep this layout in sync with the shared-cache installer in pipeline/browser.py. */
function browserExecutable() {
  const target = {
    linux: { x64: "linux64", arm64: "linux-arm64" },
    darwin: { x64: "mac-x64", arm64: "mac-arm64" },
    win32: { x64: "win64", arm64: "win64", ia32: "win32" },
  }[process.platform]?.[process.arch];
  if (!target)
    throw new Error(`No pinned Chrome for ${process.platform}/${process.arch}`);
  const cache =
    process.platform === "darwin"
      ? join(homedir(), "Library/Caches")
      : process.platform === "win32"
        ? process.env.LOCALAPPDATA || join(homedir(), "AppData/Local")
        : process.env.XDG_CACHE_HOME || join(homedir(), ".cache");
  const binary =
    process.platform === "darwin"
      ? "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
      : process.platform === "win32"
        ? "chrome.exe"
        : "chrome";
  const path = join(
    cache,
    "ipp/chrome-for-testing",
    `${browserPin.version}-${target}`,
    `chrome-${target}`,
    binary,
  );
  if (!existsSync(path))
    throw new Error(
      `Chrome ${browserPin.version} is missing; run python tools/ipp.py setup browser`,
    );
  return path;
}

/** Latest Stable Chrome, resolved to an exact pin; HTML-in-Canvas is opt-out only for capability tests. */
export function browserLaunchOptions(
  rendering = true,
  { htmlInCanvas = true, experimentalWebPlatformFeatures = true } = {},
) {
  const angle = process.env.IPP_BROWSER_ANGLE;
  const args = htmlInCanvas
    ? [
        "--enable-blink-features=CanvasDrawElement",
        ...(experimentalWebPlatformFeatures
          ? ["--enable-experimental-web-platform-features"]
          : []),
      ]
    : ["--disable-blink-features=CanvasDrawElement"];
  const base = { headless: true, executablePath: browserExecutable() };
  if (!angle)
    return {
      ...base,
      args: [...args, ...(rendering ? ["--enable-unsafe-swiftshader"] : [])],
    };
  if (angle !== "vulkan" && angle !== "gl-egl")
    throw new Error("IPP_BROWSER_ANGLE must be vulkan or gl-egl");
  return {
    ...base,
    args: [
      ...args,
      "--enable-gpu",
      `--use-angle=${angle}`,
      "--ignore-gpu-blocklist",
      ...(angle === "vulkan"
        ? [
            "--enable-features=Vulkan",
            ...(process.env.IPP_BROWSER_GPU_COMPOSITING === "1"
              ? []
              : ["--disable-vulkan-surface"]),
          ]
        : []),
    ],
  };
}

/** Fail closed when a requested hardware run falls back to a software device. */
export function verifyHardwareRenderer(renderer) {
  if (
    typeof renderer !== "string" ||
    !renderer.trim() ||
    /swiftshader|llvmpipe|softpipe|software|lavapipe|microsoft basic|webkit webgl/i.test(
      renderer,
    )
  )
    throw new Error(`Hardware WebGL required; received renderer ${renderer}`);
}
