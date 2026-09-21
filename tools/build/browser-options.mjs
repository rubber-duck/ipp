/** Shared Chromium selection for environment probes and browser scenarios. */
export function browserLaunchOptions(rendering = true) {
  const angle = process.env.IPP_BROWSER_ANGLE;
  if (!angle)
    return {
      headless: true,
      args: rendering ? ["--enable-unsafe-swiftshader"] : [],
    };
  if (angle !== "vulkan" && angle !== "gl-egl")
    throw new Error("IPP_BROWSER_ANGLE must be vulkan or gl-egl");
  return {
    headless: true,
    channel: "chromium",
    args: [
      "--enable-gpu",
      `--use-angle=${angle}`,
      "--ignore-gpu-blocklist",
      ...(angle === "vulkan"
        ? ["--enable-features=Vulkan", "--disable-vulkan-surface"]
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
