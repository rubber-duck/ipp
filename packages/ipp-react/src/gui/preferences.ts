import {
  useCallback,
  useLayoutEffect,
  useRef,
  useSyncExternalStore,
} from "react";
import type { ReactWorldClient } from "../reconciler/world-client.js";
import type { ReactWorldCommits } from "../reconciler/commits.js";

/** One bounded inspection at a time for all providers sharing an attached client.
 * Hosts own motion timing. This low-frequency read observes a World setting
 * for which the generated inspection API currently has no push notification. */
class PreferenceReader {
  value: boolean | undefined;
  private readonly listeners = new Set<() => void>();
  private lease = 0;
  private authored = 0;
  intent(value: boolean) {
    ++this.authored;
    this.publish(value);
  }
  private publish(value: boolean) {
    if (this.value === value) return;
    this.value = value;
    for (const notify of this.listeners) notify();
  }
  private timer: ReturnType<typeof setTimeout> | undefined;
  constructor(private readonly client: ReactWorldClient) {}
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    if (this.listeners.size === 1) void this.read(++this.lease);
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) {
        ++this.lease;
        clearTimeout(this.timer);
        this.value = undefined;
      }
    };
  };
  private async read(lease: number) {
    const version = this.authored;
    try {
      const page = await this.client.inspectPage!({
        collection: "guiPreferences",
      });
      if (this.lease !== lease) return;
      const next = page.guiPreferences?.reducedMotion;
      if (next !== undefined && version === this.authored) this.publish(next);
    } catch {
      // Closed clients have no more updates. A transient read retries below.
    }
    if (lease === this.lease && !this.client.closure) {
      this.timer = setTimeout(() => void this.read(lease), 250);
      // Poll while the application is running. A Node client can exit with
      // subscribers still mounted; this background read owns no Host lifetime.
      if (typeof this.timer === "object") this.timer.unref?.();
    }
  }
}

const readers = new WeakMap<ReactWorldClient, PreferenceReader>();
const noSubscribe = () => () => {};

export function useWorldReducedMotion(client: ReactWorldClient | undefined) {
  let reader = client && readers.get(client);
  if (!reader && client?.inspectPage) {
    reader = new PreferenceReader(client);
    readers.set(client, reader);
  }
  const snapshot = useCallback(() => reader?.value, [reader]);
  return useSyncExternalStore(
    reader?.subscribe ?? noSubscribe,
    snapshot,
    snapshot,
  );
}

/** Only a new authored intent writes the World. Unmounting relinquishes the writer. */
export function useReducedMotionPreference(
  commits: ReactWorldCommits | undefined,
  desired: boolean | undefined,
  client: ReactWorldClient | undefined,
) {
  const sent = useRef<{ commits: ReactWorldCommits; value: boolean } | null>(
    null,
  );
  useLayoutEffect(() => {
    if (!commits || desired === undefined) {
      sent.current = null;
      return;
    }
    if (sent.current?.commits === commits && sent.current.value === desired)
      return;
    if (
      commits.sendSystemCommand({
        type: "GuiPreferencesUpdateCommand",
        reducedMotion: desired,
      })
    ) {
      sent.current = { commits, value: desired };
      if (client) readers.get(client)?.intent(desired);
    }
  }, [commits, desired, client]);
}
