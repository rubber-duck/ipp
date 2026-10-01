import { isShader } from "./shaders.js";
import { createContext } from "react";
import ReactReconciler from "react-reconciler";
import {
  DefaultEventPriority,
  NoEventPriority,
} from "react-reconciler/constants.js";
import type { ReactWorldCommits } from "./commits.js";
import type { ReactCompositionHost } from "./attached-world.js";
import { insert, remove } from "./tree.js";
import type {
  ReactWorldDescription,
  ReactWorldElementProps,
  ReactWorldElementType,
  ReactWorldInstance,
  ReactWorldTree,
} from "./tree.js";

export class ReactWorldContainer {
  attachmentHost: ReactCompositionHost | undefined;
  capturePortals: (() => void) | undefined;
  onFailure: ((settled: Promise<void>) => void) | undefined;
  onPublish:
    | ((description: ReactWorldDescription, settled: Promise<void>) => void)
    | undefined;
  private pending: (
    | { description: ReactWorldDescription }
    | { error: unknown }
  )[] = [];
  private scheduled = false;
  private captured = false;

  constructor(
    readonly tree: ReactWorldTree,
    readonly commits: ReactWorldCommits,
  ) {}

  /** Describe this container's tree when a commit changed it. */
  capture(): void {
    if (this.captured && !this.tree.changed) return;
    this.captured = true;
    this.tree.changed = false;
    try {
      const description = this.tree.describe();
      this.pending.push({ description });
    } catch (error) {
      this.pending.push({ error });
    }
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      this.publish();
    });
  }

  publish(): void {
    const pending = this.pending;
    this.pending = [];
    for (const snapshot of pending) {
      if ("description" in snapshot) {
        const settled = this.commits.capture(snapshot.description);
        this.onPublish?.(snapshot.description, settled);
      } else {
        const settled = this.commits.failed(snapshot.error);
        this.onFailure?.(settled);
      }
    }
  }

  uncaught(error: unknown): void {
    // React clears its host tree before reporting an uncaught render error.
    // Discard that recovery snapshot before it can delete acknowledged state.
    this.pending = [];
    const settled = this.commits.failed(error);
    this.onFailure?.(settled);
  }
}

type ReactWorldReconcilerConfig = ReactReconciler.HostConfig<
  ReactWorldElementType,
  ReactWorldElementProps,
  ReactWorldContainer,
  ReactWorldInstance,
  never,
  never,
  never,
  never,
  null,
  object,
  never,
  ReturnType<typeof setTimeout>,
  -1,
  null
>;

let priority: number = NoEventPriority;
const context = {};
const noop = (): void => {};
const no = (): boolean => false;

// The installed 0.33.0 implementation reads additional suspension hooks that
// @types/react-reconciler 0.33.0 does not yet declare. Check both sources when
// updating the pin; no casts hide the remainder of the host contract.
const config: ReactWorldReconcilerConfig & {
  maySuspendCommitOnUpdate: () => boolean;
  maySuspendCommitInSyncRender: () => boolean;
  getSuspendedCommitReason: () => null;
} = {
  supportsMutation: true,
  supportsPersistence: false,
  supportsHydration: false,
  isPrimaryRenderer: false,
  getRootHostContext: () => context,
  getChildHostContext: () => context,
  getPublicInstance: () => null,
  prepareForCommit: () => null,
  resetAfterCommit(container) {
    container.capture();
    container.capturePortals?.();
  },
  createInstance: (type, props, container) =>
    container.tree.instance(type, props),
  createTextInstance() {
    throw new Error("Text nodes are not IPP declarations");
  },
  appendInitialChild: insert,
  finalizeInitialChildren: no,
  shouldSetTextContent: (type) => isShader(type),
  appendChild(parent, child) {
    parent.tree.changed = true;
    insert(parent, child);
  },
  appendChildToContainer(container, child) {
    container.tree.changed = true;
    insert(container.tree, child);
  },
  insertBefore(parent, child, before) {
    parent.tree.changed = true;
    insert(parent, child, before);
  },
  insertInContainerBefore(container, child, before) {
    container.tree.changed = true;
    insert(container.tree, child, before);
  },
  removeChild(parent, child) {
    parent.tree.changed = true;
    remove(parent, child);
  },
  removeChildFromContainer(container, child) {
    container.tree.changed = true;
    remove(container.tree, child);
  },
  clearContainer(container) {
    container.tree.changed = true;
    container.tree.children = [];
  },
  commitUpdate(instance, _type, _previous, props) {
    instance.tree.changed = true;
    instance.props = props;
  },
  hideInstance(instance) {
    instance.tree.changed = true;
    instance.hidden = true;
  },
  unhideInstance(instance) {
    instance.tree.changed = true;
    instance.hidden = false;
  },
  scheduleTimeout: setTimeout,
  cancelTimeout: clearTimeout,
  noTimeout: -1,
  supportsMicrotasks: true,
  scheduleMicrotask: queueMicrotask,
  getInstanceFromNode: () => null,
  beforeActiveInstanceBlur: noop,
  afterActiveInstanceBlur: noop,
  prepareScopeUpdate: noop,
  getInstanceFromScope: () => null,
  detachDeletedInstance: noop,
  preparePortalMount: noop,
  setCurrentUpdatePriority(value) {
    priority = value;
  },
  getCurrentUpdatePriority: () => priority,
  resolveUpdatePriority: () => priority || DefaultEventPriority,
  NotPendingTransition: null,
  HostTransitionContext: createContext(
    null,
  ) as unknown as ReactReconciler.ReactContext<null>,
  resetFormInstance: noop,
  requestPostPaintCallback(callback) {
    setTimeout(() => callback(performance.now()), 0);
  },
  shouldAttemptEagerTransition: no,
  trackSchedulerEvent: noop,
  resolveEventType: () => null,
  resolveEventTimeStamp: () => performance.now(),
  maySuspendCommit: no,
  maySuspendCommitOnUpdate: no,
  maySuspendCommitInSyncRender: no,
  preloadInstance: () => true,
  startSuspendingCommit: noop,
  suspendInstance: noop,
  waitForCommitToBeReady: () => null,
  getSuspendedCommitReason: () => null,
};

export const reconciler = ReactReconciler(config);

export function exchangePriority(value: number): number {
  const previous = priority;
  priority = value;
  return previous;
}
