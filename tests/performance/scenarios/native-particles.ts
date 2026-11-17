import assert from "node:assert/strict";

/** Compare two completed frames of the same fixed camera. The caller pauses
 * the other animation controllers and enters with the Host clock paused. */
export async function observeNativeParticles<T extends { triangles: number }>(
  call: <R>(name: string, ...args: unknown[]) => Promise<R>,
  capture: (label: string) => Promise<T>,
  hidden: (value: boolean) => Promise<void>,
) {
  // The finite emitter can finish during profiling. Start a fresh emission
  // window in the fixed closeup, with the other animation controllers paused.
  const particleCamera = await call<bigint>("particleView");
  try {
    await call("restartNative");
    const nativeCleared = await capture("native-cleared");
    await hidden(false);
    const particleDeadline = performance.now() + 60_000;
    let nativePresent;
    let nativeDifference;
    for (;;) {
      nativePresent = await capture("native-present");
      nativeDifference = await call<{ changedPixels: number }>(
        "difference",
        "native-present",
        "native-cleared",
      );
      const added = nativePresent.triangles - nativeCleared.triangles;
      if (added > 24 && nativeDifference.changedPixels > 100) break;
      assert.ok(
        performance.now() < particleDeadline,
        `native particles did not add visible instances: ${added} extra triangles, ${nativeDifference.changedPixels} changed pixels`,
      );
    }
    return { nativePresent, nativeCleared, nativeDifference };
  } finally {
    await hidden(true);
    await call("closeDetailView", particleCamera);
  }
}
