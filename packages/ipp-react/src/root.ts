import { createElement, type ReactNode } from "react";
import { AttachmentContext } from "./composition/attached-world.js";
import { ReactAttachmentGroup } from "./composition/attachment-state.js";
import {
  ConcurrentRoot,
  DefaultEventPriority,
} from "react-reconciler/constants.js";
import { ReactWorldCommits } from "./reconciler/commits.js";
import type { ReactWorldRootOptions } from "./reconciler/commits.js";
import type { ReactWorldClient } from "./reconciler/world-client.js";
import {
  ReactWorldContainer,
  exchangePriority,
  reconciler,
} from "./reconciler/host-config.js";
import { ReactWorldTree } from "./reconciler/tree.js";
import type { CameraOutputReference } from "@ipp/client";

export interface RootCleanup {
  retry(): Promise<void>;
  abandon(): Promise<void>;
}

export const rootCleanup = new WeakMap<object, RootCleanup>();

export interface ReactWorldRoot {
  /**
   * Resolve an acknowledged Camera declaration in this fixed World, without
   * selecting presentation. A World's canvas needs no binding:
   * `canvasOutput(world)` names it.
   */
  bindOutput(
    entity: string | bigint,
    kind: "camera",
  ): Promise<CameraOutputReference>;
  getDataSource(
    name: string,
  ): import("./data/declarations.js").ReactDataSourceState | undefined;
  onDataSourceChange(
    listener: (
      state: import("./data/declarations.js").ReactDataSourceState,
    ) => void,
  ): () => void;
  getAsset(
    id: string,
  ): import("./assets/registry.js").ReactAssetState | undefined;
  onAssetChange(
    listener: (state: import("./assets/registry.js").ReactAssetState) => void,
  ): () => void;
  render(element: ReactNode): Promise<void>;
  flush(): Promise<void>;
  unmount(): Promise<void>;
}

/** Create declarations from a connected, target-matched generated client. */
export function createRoot(
  client: ReactWorldClient,
  options: ReactWorldRootOptions = {},
): ReactWorldRoot {
  if (client.closure) throw client.closure.reason;
  if (
    options.host &&
    (!client.worldReference ||
      options.host.sessions.get(client.session) !== client)
  )
    throw new Error("React composition requires a live session from this Host");
  const tree = new ReactWorldTree(client);
  const commits = new ReactWorldCommits(client, options);
  const state = new ReactWorldContainer(tree, commits);
  const attachments = options.host
    ? new ReactAttachmentGroup(options.host, state, client, options)
    : undefined;
  let uncaughtError: unknown;
  const onUncaughtError = (error: Error): void => {
    uncaughtError = error;
    state.uncaught(error);
    attachments?.uncaught(error);
  };
  const container = reconciler.createContainer(
    state,
    ConcurrentRoot,
    null,
    false,
    null,
    "ipp-",
    onUncaughtError,
    (error) => {
      commits.report(error);
    },
    (error) => {
      commits.report(error);
    },
    () => {},
  );
  let current: ReactNode = null;
  let closing: Promise<void> | undefined;
  let cleanupFailed = false;
  const wrapped = (element: ReactNode): ReactNode =>
    createElement(AttachmentContext, { value: state }, element);

  const flush = async (): Promise<void> => {
    if (closing) return closing;
    if (uncaughtError) throw uncaughtError;
    reconciler.flushPassiveEffects();
    await new Promise<void>((resolve) => {
      const priority = exchangePriority(DefaultEventPriority);
      try {
        reconciler.updateContainer(wrapped(current), container, null, resolve);
      } finally {
        exchangePriority(priority);
      }
    });
    reconciler.flushPassiveEffects();
    if (uncaughtError) throw uncaughtError;
    state.publish();
    attachments?.publish();
    await Promise.all([commits.checkpoint(), attachments?.settled()]);
  };

  const update = (element: ReactNode): Promise<void> => {
    current = element;
    uncaughtError = undefined;
    try {
      reconciler.flushSyncFromReconciler(() => {
        reconciler.updateContainerSync(wrapped(element), container, null);
      });
    } catch (error) {
      onUncaughtError(
        error instanceof Error ? error : new Error(String(error)),
      );
    }
    state.publish();
    attachments?.publish();
    // React may bail out when given the same element; never retry the rejected
    // description just because a caller asks to render or flush it again.
    return attachments ? flush() : commits.settled();
  };

  const render = (element: ReactNode): Promise<void> =>
    closing
      ? Promise.reject(new Error("The React root is unmounted"))
      : update(element);

  const dispose = async (retry = false): Promise<void> => {
    if (!attachments) return commits.dispose();
    const results = await Promise.allSettled([
      commits.dispose(),
      attachments?.dispose(retry),
    ]);
    const errors = results.flatMap((result) =>
      result.status === "rejected" ? [result.reason] : [],
    );
    if (errors.length)
      throw new AggregateError(errors, "React root cleanup is incomplete");
  };
  const cleanup = (retry: boolean): Promise<void> => {
    cleanupFailed = false;
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    const attempt = new Promise<void>((accept, fail) => {
      resolve = accept;
      reject = fail;
    });
    closing = attempt;
    void attempt.catch(() => {
      if (closing === attempt) cleanupFailed = true;
    });
    if (!retry) {
      // Unmount deletes nothing: fence authoring and attached Worlds before
      // React clears the tree, so the empty tree is never committed.
      commits.fence();
      attachments?.fence();
      try {
        void update(null).catch(() => {});
      } catch (error) {
        commits.report(error);
      }
    }
    void dispose(retry).then(resolve, reject);
    return attempt;
  };
  const root: ReactWorldRoot = {
    async bindOutput(entity, kind) {
      if (!options.host || !client.worldReference)
        throw new Error("Output binding requires createRoot(client, { host })");
      if (closing || client.closure)
        throw new Error("The React root is unavailable");
      if (kind !== "camera")
        throw new Error(
          "Only Camera outputs bind; canvasOutput(world) names a World's canvas",
        );
      const id = commits.resolveEntity(entity);
      const output = await options.host.bindOutput(
        client.worldReference,
        id,
        kind,
      );
      if (closing || client.closure || commits.resolveEntity(entity) !== id)
        throw new Error("Output declaration changed while binding");
      if (output.kind !== "camera")
        throw new Error("The Host bound a non-Camera output");
      return output;
    },
    getDataSource: (name) => commits.dataSources.get(name),
    onDataSourceChange: (listener) =>
      commits.dataSources.subscribe((state) => {
        if (!client.closure && !closing) listener(state);
      }),
    getAsset: (id) => commits.assets.get(id),
    onAssetChange: (listener) =>
      commits.assets.subscribe((state) => {
        if (!client.closure && !closing) listener(state);
      }),
    render,
    flush,
    unmount() {
      if (closing) return cleanupFailed ? cleanup(true) : closing;
      return cleanup(false);
    },
  };
  rootCleanup.set(root, {
    async retry() {
      if (!closing) throw new Error("The React root is not unmounted");
      await root.unmount();
    },
    async abandon() {
      if (!closing) throw new Error("The React root is not unmounted");
      await closing.catch(() => {});
      await attachments?.abandon();
    },
  });
  return root;
}
