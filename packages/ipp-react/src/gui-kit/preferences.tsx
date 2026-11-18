import { useCallback, useSyncExternalStore, type ReactNode } from "react";
import { GuiKit, type GuiKitProps } from "./kit.js";
import type { GuiSkinDefinition } from "../gui/skin-types.js";

export interface GuiPreferencesSettings {
  readonly skin: string;
  readonly reducedMotion: "system" | "reduce" | "full";
}
export interface GuiPreferenceStorage {
  read(): unknown;
  write(value: GuiPreferencesSettings): void;
  subscribe?(changed: () => void): () => void;
}
export interface GuiMotionPreference {
  get(): boolean;
  subscribe(changed: () => void): () => void;
}

/** Platform adapters are explicit. Calling this helper is the browser integration point. */
export function browserMotionPreference(window: Window): GuiMotionPreference {
  const query = window.matchMedia("(prefers-reduced-motion: reduce)");
  return {
    get: () => query.matches,
    subscribe: (changed) => {
      query.addEventListener("change", changed);
      return () => query.removeEventListener("change", changed);
    },
  };
}
export function browserPreferenceStorage(
  window: Window,
  key: string,
): GuiPreferenceStorage {
  return {
    read: () => {
      const text = window.localStorage.getItem(key);
      return text === null ? null : JSON.parse(text);
    },
    write: (value) => window.localStorage.setItem(key, JSON.stringify(value)),
    subscribe: (changed) => {
      const onStorage = (e: StorageEvent) => {
        if (
          e.storageArea === window.localStorage &&
          (e.key === null || e.key === key)
        )
          changed();
      };
      window.addEventListener("storage", onStorage);
      return () => window.removeEventListener("storage", onStorage);
    },
  };
}

/** A scoped settings store supplies a synchronous snapshot before visible mount. */
export function createGuiPreferences(
  defaults: GuiPreferencesSettings,
  skins: Readonly<Record<string, GuiSkinDefinition>>,
  storage?: GuiPreferenceStorage,
  onError?: (error: unknown) => void,
) {
  function valid(value: unknown): value is GuiPreferencesSettings {
    if (
      !value ||
      typeof value !== "object" ||
      !("skin" in value) ||
      !("reducedMotion" in value)
    )
      return false;
    return (
      typeof value.skin === "string" &&
      Object.hasOwn(skins, value.skin) &&
      (value.reducedMotion === "system" ||
        value.reducedMotion === "reduce" ||
        value.reducedMotion === "full")
    );
  }
  if (!valid(defaults)) throw new TypeError("Invalid default GUI preferences");
  let current = Object.freeze({ ...defaults }),
    unsubscribe: (() => void) | undefined;
  const listeners = new Set<() => void>();
  function update(next: GuiPreferencesSettings) {
    if (
      next.skin === current.skin &&
      next.reducedMotion === current.reducedMotion
    )
      return;
    current = Object.freeze({
      skin: next.skin,
      reducedMotion: next.reducedMotion,
    });
    for (const listener of listeners) listener();
  }
  function read() {
    if (!storage) return;
    try {
      const saved = storage.read();
      if (valid(saved)) update(saved);
      else update(defaults);
    } catch (error) {
      onError?.(error);
    }
  }
  read();
  return {
    skins,
    get: () => current,
    set(next: GuiPreferencesSettings) {
      if (!valid(next)) throw new TypeError("Invalid GUI preferences");
      update(next);
      try {
        storage?.write(current);
      } catch (error) {
        onError?.(error);
      }
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      if (listeners.size === 1) {
        try {
          unsubscribe = storage?.subscribe?.(read);
          read();
        } catch (error) {
          onError?.(error);
        }
      }
      return () => {
        listeners.delete(listener);
        if (!listeners.size) {
          unsubscribe?.();
          unsubscribe = undefined;
        }
      };
    },
  };
}
export interface PreferencesProps
  extends Omit<GuiKitProps, "skin" | "reducedMotion" | "children"> {
  readonly settings: ReturnType<typeof createGuiPreferences>;
  readonly platformMotion?: GuiMotionPreference;
  readonly children?: ReactNode;
}
const noSubscription = () => () => {};
export function Preferences({
  settings,
  platformMotion,
  children,
  ...kit
}: PreferencesProps) {
  const value = useSyncExternalStore(
    settings.subscribe,
    settings.get,
    settings.get,
  );
  const platformSnapshot = useCallback(
    () => platformMotion?.get(),
    [platformMotion],
  );
  const motion = useSyncExternalStore(
    platformMotion?.subscribe ?? noSubscription,
    platformSnapshot,
    platformSnapshot,
  );
  const reduced =
    value.reducedMotion === "system"
      ? motion
      : value.reducedMotion === "reduce";
  return (
    <GuiKit
      {...kit}
      skin={settings.skins[value.skin]!}
      {...(reduced === undefined ? {} : { reducedMotion: reduced })}
    >
      {children}
    </GuiKit>
  );
}
