import type { ReactNode } from "react";
import type { Client, FrameCapture } from "@ipp/client";
import { createRoot, type ReactWorldRoot } from "./index.js";

export interface IppCanvasHandle {
  readonly client: Client;
  readonly closed: Promise<void>;
  /** Create an additional scene scope included in canvas flush and teardown. */
  createRoot(): ReactWorldRoot;
  /** Current host-requested drawing-buffer dimensions for geometry queries. */
  readonly viewport: Readonly<{ width: number; height: number }>;
  /** Wait for world submissions and releases already committed by React DOM. */
  flush(): Promise<void>;
  /** Wait for world acknowledgement and a corresponding completed GPU frame. */
  capture(): Promise<FrameCapture>;
}

/** One canvas owns its connection and all declaration roots using it. */
export class CanvasWorldSession implements IppCanvasHandle {
  readonly closed: Promise<void>;
  private readonly worlds = new Set<CanvasWorldBinding>();
  private tail: Promise<void> = Promise.resolve();
  private pendingRender:
    | {
        binding: CanvasWorldBinding;
        children: ReactNode;
        onCommit: (() => void) | undefined;
        result: Promise<void>;
      }
    | undefined;
  private closing = false;
  private readonly closingListeners = new Set<() => void>();

  get isClosing(): boolean {
    return this.closing;
  }

  onClosing(listener: () => void): () => void {
    if (this.closing) listener();
    else this.closingListeners.add(listener);
    return () => {
      this.closingListeners.delete(listener);
    };
  }
  private resolveClosed!: () => void;
  private rejectClosed!: (error: unknown) => void;

  constructor(
    readonly client: Client,
    readonly report: (error: Error) => void,
    private readonly dimensions: () => Readonly<{
      width: number;
      height: number;
    }>,
  ) {
    this.closed = new Promise<void>((resolve, reject) => {
      this.resolveClosed = resolve;
      this.rejectClosed = reject;
    });
    // Cleanup remains observable through closed even when the DOM has departed.
    void this.closed.catch(() => {});
  }

  get viewport(): Readonly<{ width: number; height: number }> {
    return { ...this.dimensions() };
  }

  createRoot(): ReactWorldRoot {
    const binding = this.attach(this.report);
    return {
      ...binding.root,
      render: (element) =>
        binding.isClosing
          ? Promise.reject(new Error("The React root is unmounted"))
          : binding.render(element),
      flush: () => this.enqueue(() => binding.root.flush()),
      unmount: () => binding.close(),
    };
  }

  attach(report: (error: Error) => void): CanvasWorldBinding {
    if (this.closing) throw new Error("The IPP canvas is closing");
    const world = new CanvasWorldBinding(this, report);
    this.worlds.add(world);
    return world;
  }

  enqueue(work: () => Promise<void>): Promise<void> {
    this.pendingRender = undefined;
    return this.enqueueOrdered(work);
  }

  private enqueueOrdered(work: () => Promise<void>): Promise<void> {
    const next = this.tail.catch(() => {}).then(work);
    this.tail = next;
    void next.catch(() => {});
    return next;
  }

  enqueueRender(
    binding: CanvasWorldBinding,
    children: ReactNode,
    onCommit: (() => void) | undefined,
    report: (error: Error) => void,
  ): Promise<void> {
    if (this.pendingRender?.binding === binding) {
      this.pendingRender.children = children;
      this.pendingRender.onCommit = onCommit;
      return this.pendingRender.result;
    }
    const pending = {
      binding,
      children,
      onCommit,
      result: Promise.resolve(),
    };
    const result = this.enqueueOrdered(async () => {
      if (this.pendingRender === pending) this.pendingRender = undefined;
      if (binding.isClosing) return;
      await binding.root.render(pending.children);
      if (!binding.isClosing) notify(() => pending.onCommit?.(), report);
    });
    pending.result = result;
    this.pendingRender = pending;
    return result;
  }

  release(world: CanvasWorldBinding): void {
    this.worlds.delete(world);
  }

  async flush(): Promise<void> {
    if (this.closing) throw new Error("The IPP canvas is closing");
    this.pendingRender = undefined;
    for (;;) {
      const pending = this.tail;
      await pending;
      await Promise.all([...this.worlds].map((world) => world.flush()));
      if (pending === this.tail) return;
    }
  }

  async capture(): Promise<FrameCapture> {
    await this.flush();
    const inspection = await this.client.inspectPage();
    const frame = await this.client.presentation!.capture(inspection.tick);
    if (frame.session !== this.client.session) {
      throw new Error("Captured frame belongs to a different canvas session");
    }
    return frame;
  }

  close(): Promise<void> {
    if (this.closing) return this.closed;
    this.closing = true;
    for (const listener of this.closingListeners) listener();
    this.closingListeners.clear();
    const releases = [...this.worlds].map((world) => world.close());
    void (async () => {
      const results = await Promise.allSettled(releases);
      const errors = results.flatMap((result) =>
        result.status === "rejected" ? [result.reason] : [],
      );
      try {
        await this.client.close();
      } catch (error) {
        errors.push(error);
      }
      if (errors.length) {
        this.rejectClosed(
          new AggregateError(errors, "IPP canvas cleanup failed"),
        );
      } else {
        this.resolveClosed();
      }
    })();
    return this.closed;
  }
}

export class CanvasWorldBinding {
  readonly root: ReactWorldRoot;
  private closing: Promise<void> | undefined;

  constructor(
    private readonly session: CanvasWorldSession,
    private readonly report: (error: Error) => void,
  ) {
    this.root = createRoot(session.client, { onError: report });
  }

  get isClosing(): boolean {
    return this.closing !== undefined;
  }

  render(children: ReactNode, onCommit?: () => void): Promise<void> {
    // Defer custom reconciliation until React DOM has left its commit phase.
    // The shared queue also orders a departing scope before its replacement.
    const result = this.session.enqueueRender(
      this,
      children,
      onCommit,
      this.report,
    );
    void result.catch(() => {
      // The root's onError already reports rejected/invalid declarations.
    });
    return result;
  }

  flush(): Promise<void> {
    return this.closing ?? this.root.flush();
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.closing = this.session.enqueue(async () => {
      try {
        await this.root.unmount();
      } finally {
        this.session.release(this);
      }
    });
    return this.closing;
  }
}

export function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

/** Observe user callback failures without blocking world work on user promises. */
export function notify(
  callback: () => void | Promise<void>,
  report: (error: Error) => void,
): void {
  try {
    void Promise.resolve(callback()).catch((error: unknown) =>
      report(asError(error)),
    );
  } catch (error) {
    report(asError(error));
  }
}
