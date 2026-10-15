import type { OutputReference, PresentationViewport } from "@ipp/client";
import type {
  GalleryOptions,
  GallerySceneMount,
} from "../../../shared/scene.js";

/** Chart actions and options share one queue; World clocks stay with the Host. */
export function mountCharts(config: {
  readonly output: OutputReference;
  readonly options: GalleryOptions;
  readonly ready: Promise<void>;
  readonly resize: (viewport: PresentationViewport) => Promise<void>;
  readonly update: (
    options: GalleryOptions,
    patch: GalleryOptions,
  ) => Promise<GalleryOptions>;
  readonly actions: Readonly<
    Record<
      string,
      (options: GalleryOptions, args?: unknown) => Promise<GalleryOptions>
    >
  >;
  readonly inspect: () => Promise<unknown>;
  readonly dispose: () => Promise<void>;
}): GallerySceneMount {
  let options = config.options;
  let closing: Promise<void> | undefined;
  let tail = config.ready;
  void tail.catch(() => {});
  const listeners = new Set<() => void>();
  const notify = () => {
    for (const listener of listeners) listener();
  };
  const enqueue = <T>(operation: () => Promise<T>): Promise<T> => {
    const result = tail.then(operation);
    tail = result.then(
      () => {},
      () => {},
    );
    return result;
  };
  return {
    output: config.output,
    ready: config.ready,
    get options() {
      return options;
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    update(patch) {
      if (closing)
        return Promise.reject(new Error("The chart scene is disposed"));
      return enqueue(async () => {
        options = await config.update(options, patch);
        notify();
      });
    },
    action(name, args) {
      if (closing)
        return Promise.reject(new Error("The chart scene is disposed"));
      const action = config.actions[name];
      if (!action)
        return Promise.reject(new Error(`Unknown chart action: ${name}`));
      return enqueue(async () => {
        options = await action(options, args);
        notify();
        return options;
      });
    },
    resize(viewport) {
      if (closing)
        return Promise.reject(new Error("The chart scene is disposed"));
      return enqueue(() => config.resize(viewport));
    },
    inspect: config.inspect,
    dispose() {
      if (!closing) {
        closing = tail.catch(() => {}).then(config.dispose);
        listeners.clear();
      }
      return closing;
    },
  };
}
