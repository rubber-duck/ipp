import type { ReactNode } from "react";
import type { AssetWorldClient, OutputReference } from "@ipp/client";
import {
  GallerySceneProvider,
  type GalleryOptions,
  type GallerySceneContext,
  type GallerySceneMount,
} from "./scene.js";

/** Wait for demanded resources; animation is allowed to keep producing frames. */
export async function waitForGalleryResources(
  context: GallerySceneContext,
): Promise<void> {
  const client = context.canvas.client as AssetWorldClient;
  const deadline = performance.now() + 30_000;
  for (;;) {
    context.signal.throwIfAborted();
    const inspection = await client.inspect();
    const failed = inspection.resources.find(
      (resource) => resource.status === "failed",
    );
    if (failed)
      throw new Error(
        `Gallery resource failed: ${failed.source}: ${failed.error ?? "unknown resource error"}`,
      );
    if (performance.now() >= deadline)
      throw new Error(
        "Gallery resources did not become ready within 30 seconds",
      );
    if (inspection.resources.every((resource) => resource.status === "loaded"))
      return;
    await new Promise<void>((resolve, reject) => {
      const finish = () => {
        clearTimeout(timer);
        unsubscribe();
        context.signal.removeEventListener("abort", abort);
        resolve();
      };
      const abort = () => {
        clearTimeout(timer);
        unsubscribe();
        reject(context.signal.reason);
      };
      const unsubscribe = client.onResourceChange(finish);
      const timer = setTimeout(finish, 25);
      context.signal.addEventListener("abort", abort, { once: true });
    });
    await context.canvas.flush();
  }
}

/** One shared fixed-World root for simple scenes; complex scenes may own more. */
export async function mountGalleryScene(
  context: GallerySceneContext,
  config: {
    readonly options: GalleryOptions;
    readonly output: OutputReference;
    readonly render: (options: GalleryOptions) => ReactNode;
    readonly actions?: Readonly<
      Record<string, (args?: unknown) => Promise<unknown>>
    >;
  },
): Promise<GallerySceneMount> {
  const root = context.canvas.createRoot();
  let options = config.options;
  let disposed = false;
  let closing: Promise<void> | undefined;
  let tail = Promise.resolve();
  const render = () =>
    root.render(
      <GallerySceneProvider context={context}>
        {config.render(options)}
      </GallerySceneProvider>,
    );
  const enqueue = <T,>(operation: () => Promise<T>): Promise<T> => {
    const pending = tail.then(operation);
    tail = pending.then(
      () => {},
      () => {},
    );
    return pending;
  };
  try {
    await render();
  } catch (error) {
    await root.render(null).catch(() => {});
    await root.unmount();
    throw error;
  }
  const ready = waitForGalleryResources(context);
  void ready.catch(() => {});
  return {
    output: config.output,
    ready,
    get options() {
      return options;
    },
    update(patch) {
      if (disposed)
        return Promise.reject(new Error("The gallery scene is disposed"));
      return enqueue(async () => {
        if (disposed) throw new Error("The gallery scene is disposed");
        options = { ...options, ...patch };
        await render();
        await context.canvas.flush();
        await waitForGalleryResources(context);
      });
    },
    async action(name, args) {
      if (disposed) throw new Error("The gallery scene is disposed");
      // Actions may call update, whose writes are serialized above. Runners
      // serialize complete imperative actions; do not enqueue a callback that
      // awaits a later position of this same update queue.
      const action = config.actions?.[name];
      if (!action) throw new Error(`Unknown gallery action: ${name}`);
      return action(args);
    },
    inspect: () => context.canvas.client.inspect(),
    dispose() {
      if (closing) return closing;
      disposed = true;
      closing = enqueue(async () => {
        const failures: unknown[] = [];
        try {
          await root.render(null);
          await root.flush();
        } catch (error) {
          failures.push(error);
        }
        try {
          await root.unmount();
        } catch (error) {
          failures.push(error);
        }
        if (failures.length)
          throw new AggregateError(
            failures,
            "Gallery scene cleanup is incomplete",
          );
      });
      return closing;
    },
  };
}
