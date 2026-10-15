import { useLayoutEffect, useRef } from "react";
import type { ReactWorldCommits } from "../reconciler/commits.js";

/**
 * Send the World's reduced-motion preference through `commits` when the
 * setting changes, and turn it off again when the setting or the kit goes
 * after turning it on. Undefined `commits`, in a kit that declares nothing,
 * sends nothing.
 */
export function useReducedMotionPreference(
  commits: ReactWorldCommits | undefined,
  reducedMotion: boolean | undefined,
): void {
  /** The value this kit last sent, if any. */
  const sent = useRef<boolean | undefined>(undefined);
  useLayoutEffect(() => {
    if (!commits) return;
    return () => {
      // Unmounting the root fences it first, so this then sends nothing.
      if (sent.current) sendReducedMotion(commits, false);
      sent.current = undefined;
    };
  }, [commits]);
  useLayoutEffect(() => {
    if (!commits) return;
    const next = reducedMotion ?? (sent.current ? false : undefined);
    if (
      next !== undefined &&
      next !== sent.current &&
      sendReducedMotion(commits, next)
    )
      sent.current = next;
  }, [commits, reducedMotion]);
}

function sendReducedMotion(
  commits: ReactWorldCommits,
  reducedMotion: boolean,
): boolean {
  return commits.sendSystemCommand({
    type: "GuiPreferencesUpdateCommand",
    reducedMotion,
  });
}
