import { asError, notify } from "./error-reporting.js";
import { createElement, type ReactNode } from "react";
import { CanvasContext } from "./context.js";
import { canvasStateUpdate } from "./world.js";
import {
  canvasOutput,
  type CanvasState,
  type CanvasStateUpdateCommand,
  type Client,
  type OutputReference,
  type PresentationFrameOptions,
  type PresentationView,
  type PresentationViewport,
  type PresentedCapture,
  type PresentedFrame,
  type WorldCreateOptions,
  type WorldReference,
} from "@ipp/client";
import { createRoot, rootCleanup, type ReactWorldRoot } from "../root.js";
import {
  CanvasPresentation,
  type CanvasHost,
  type CanvasSize,
} from "./presentation.js";
import { CanvasLifetime, type CanvasCleanupRecovery } from "./lifetime.js";

export interface IppCanvasHandle {
  readonly host: CanvasHost;
  readonly client: Client;
  readonly closed: Promise<void>;
  readonly cleanup: CanvasCleanupRecovery;
  readonly view: PresentationView | null;
  readonly viewport: PresentationViewport | null;
  createRoot(): ReactWorldRoot;
  flush(): Promise<void>;
  frame(options?: PresentationFrameOptions): Promise<PresentedFrame>;
  capture(options?: PresentationFrameOptions): Promise<PresentedCapture>;
  recoverPresentation(): Promise<void>;
}

export interface CanvasSessionOptions {
  readonly host: CanvasHost;
  readonly client: Client;
  readonly ownsClient?: boolean;
  readonly onError?: (error: Error) => void;
  /** Observe rejected declarations separately from presentation/lifecycle errors. */
  readonly onDeclarationError?: (error: Error) => void;
  readonly onViewChange?: (view: PresentationView | null) => void;
}

/** A fixed authoring session and independent explicit surface selection. External Hosts are borrowed. */
export class CanvasWorldSession implements IppCanvasHandle {
  readonly host: CanvasHost;
  readonly client: Client;
  readonly closed: Promise<void>;
  readonly cleanup: CanvasLifetime;
  readonly report: (error: Error) => void;
  readonly reportDeclaration: (error: Error) => void;
  private readonly presentation: CanvasPresentation;
  private readonly worlds = new Set<CanvasWorldBinding>();
  private tail: Promise<void> = Promise.resolve();
  private pendingRender:
    | {
        binding: CanvasWorldBinding;
        children: ReactNode;
        onCommit: ((scope: ReactWorldRoot) => void) | undefined;
        result: Promise<void>;
      }
    | undefined;
  private closing = false;
  /** The IppCanvas's explicit root output; undefined defers to a root CanvasWorld. */
  private explicitOutput: OutputReference | null | undefined = null;
  /** The canvas output of the root-presented CanvasWorld. */
  private rootClaim: OutputReference | undefined;
  private size: CanvasSize | undefined;
  private readonly closingListeners = new Set<() => void>();
  private resolveClosed!: () => void;
  private rejectClosed!: (error: unknown) => void;

  constructor(options: CanvasSessionOptions, lifetime?: CanvasLifetime) {
    this.host = options.host;
    this.client = options.client;
    this.report = options.onError ?? ((error) => console.error(error));
    this.reportDeclaration = options.onDeclarationError ?? this.report;
    this.cleanup = lifetime ?? new CanvasLifetime(this.host, false);
    if (!lifetime) this.cleanup.adopt(this.client, options.ownsClient ?? false);
    this.presentation = new CanvasPresentation(this.host, (view) => {
      if (!this.isClosing)
        notify(() => options.onViewChange?.(view), this.report);
    });
    this.cleanup.manage({
      close: async (retry) => {
        this.fence();
        const results = await Promise.allSettled([
          this.presentation.close(),
          ...[...this.worlds].map((world) =>
            retry ? world.retry() : world.close(),
          ),
        ]);
        const errors = results.flatMap((result) =>
          result.status === "rejected" ? [result.reason] : [],
        );
        if (errors.length)
          throw new AggregateError(
            errors,
            "Canvas declarations or presentation remain owned",
          );
      },
      abandon: async () => {
        this.fence();
        await Promise.all([...this.worlds].map((world) => world.abandon()));
      },
      journal: () => this.presentation.journal,
    });
    this.closed = new Promise<void>((resolve, reject) => {
      this.resolveClosed = resolve;
      this.rejectClosed = reject;
    });
    void this.closed.catch(() => {});
    void this.client.closed.then(() => {
      if (!this.closing) {
        this.fence();
        void this.finish();
      }
    });
  }

  get isClosing(): boolean {
    return this.closing || !!this.client.closure;
  }
  get view(): PresentationView | null {
    return this.presentation.view;
  }
  get viewport(): PresentationViewport | null {
    return this.presentation.viewport;
  }

  onClosing(listener: () => void): () => void {
    if (this.closing) listener();
    else this.closingListeners.add(listener);
    return () => {
      this.closingListeners.delete(listener);
    };
  }

  /**
   * Select the root output at `size`. Undefined presents the root-presented
   * CanvasWorld's canvas, if one is mounted; an explicit output or null
   * conflicts with one.
   */
  selectOutput(
    output: OutputReference | null | undefined,
    size: CanvasSize,
  ): Promise<void> {
    if (this.isClosing)
      return Promise.reject(new Error("The Canvas is closing"));
    this.explicitOutput = output;
    this.size = size;
    return this.present();
  }

  private present(): Promise<void> {
    if (this.rootClaim && this.explicitOutput !== undefined)
      return Promise.reject(
        new Error(
          "IppCanvas selects an explicit output while a CanvasWorld presents the root",
        ),
      );
    return this.presentation.select(
      this.rootClaim ?? this.explicitOutput ?? null,
      this.size!,
    );
  }

  private reselect(): void {
    if (!this.size || this.isClosing) return;
    void this.present().catch((error) => {
      if (!this.isClosing) this.report(asError(error));
    });
  }

  /** Present `output` as the root until the returned release. */
  claimRoot(output: OutputReference): () => void {
    if (this.isClosing) throw new Error("The Canvas is closing");
    if (this.rootClaim)
      throw new Error("Another CanvasWorld already presents the root");
    this.rootClaim = output;
    this.reselect();
    return () => {
      if (this.rootClaim !== output) return;
      this.rootClaim = undefined;
      this.reselect();
    };
  }

  /**
   * Create a World selecting the Canvas System and author it through its own
   * session; see `OwnedCanvasWorld`.
   */
  openCanvasWorld(
    create: Omit<WorldCreateOptions, "canvas">,
    request: CanvasStateRequest,
    report: (error: Error) => void,
    ready: (world: WorldReference, closed: Promise<void>) => void,
    reportDeclaration: (error: Error) => void = report,
  ): OwnedCanvasWorld {
    if (this.isClosing) throw new Error("The Canvas is closing");
    return new OwnedCanvasWorld(
      this,
      create,
      request,
      report,
      ready,
      reportDeclaration,
    );
  }

  recoverPresentation(): Promise<void> {
    return this.presentation.recover();
  }

  createRoot(): ReactWorldRoot {
    const binding = this.attach(this.reportDeclaration);
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

  attach(
    report: (error: Error) => void,
    owned?: CanvasWorldOwnership,
  ): CanvasWorldBinding {
    if (this.isClosing) throw new Error("The Canvas is closing");
    const world = new CanvasWorldBinding(this, report, owned);
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
    onCommit: ((scope: ReactWorldRoot) => void) | undefined,
    report: (error: Error) => void,
  ): Promise<void> {
    if (this.pendingRender?.binding === binding) {
      this.pendingRender.children = children;
      this.pendingRender.onCommit = onCommit;
      return this.pendingRender.result;
    }
    const pending = { binding, children, onCommit, result: Promise.resolve() };
    const result = this.enqueueOrdered(async () => {
      if (this.pendingRender === pending) this.pendingRender = undefined;
      if (binding.isClosing) return;
      // Every root of the session sees it, so a root CanvasWorld can present.
      await binding.root.render(
        createElement(CanvasContext, { value: this }, pending.children),
      );
      if (!binding.isClosing && !this.isClosing)
        notify(() => pending.onCommit?.(binding.root), report);
    });
    pending.result = result;
    this.pendingRender = pending;
    return result;
  }

  release(world: CanvasWorldBinding): void {
    this.worlds.delete(world);
  }

  flush(): Promise<void> {
    if (this.isClosing)
      return Promise.reject(new Error("The Canvas is closing"));
    const worlds = [...this.worlds];
    return this.enqueue(async () => {
      if (this.isClosing) throw new Error("The Canvas is closing");
      await Promise.all(
        worlds
          .filter((world) => !world.isClosing)
          .map((world) => world.root.flush()),
      );
    });
  }

  async frame(options?: PresentationFrameOptions): Promise<PresentedFrame> {
    await this.flush();
    return this.presentation.frame(options);
  }

  async capture(options?: PresentationFrameOptions): Promise<PresentedCapture> {
    await this.flush();
    return this.presentation.capture(options);
  }

  private fence(): void {
    if (this.closing) return;
    this.closing = true;
    this.presentation.fence();
    for (const world of this.worlds) void world.close().catch(() => {});
    for (const listener of this.closingListeners) notify(listener, this.report);
    this.closingListeners.clear();
  }

  close(): Promise<void> {
    if (this.closing) return this.closed;
    this.fence();
    void this.finish();
    return this.closed;
  }

  private async finish(): Promise<void> {
    try {
      await this.cleanup.close();
      this.resolveClosed();
    } catch (error) {
      this.rejectClosed(error);
      this.report(asError(error));
    }
  }
}

/** A World and session a binding authors in place of the Canvas's own. */
export interface CanvasWorldOwnership {
  readonly world: WorldReference;
  readonly client: Client;
}

/**
 * One React root in a Canvas session. An owned binding authors its own World
 * through its own session: closing it also closes that session, and removing
 * it while the Canvas stays open destroys that World.
 */
export class CanvasWorldBinding {
  readonly root: ReactWorldRoot;
  private closing: Promise<void> | undefined;
  private destroy = false;

  constructor(
    private readonly session: CanvasWorldSession,
    private readonly report: (error: Error) => void,
    private readonly owned?: CanvasWorldOwnership,
  ) {
    this.root = createRoot(owned?.client ?? session.client, {
      host: session.host,
      onError: report,
    });
  }

  get isClosing(): boolean {
    return this.closing !== undefined;
  }

  render(
    children: ReactNode,
    onCommit?: (scope: ReactWorldRoot) => void,
  ): Promise<void> {
    const result = this.session.enqueueRender(
      this,
      children,
      onCommit,
      this.report,
    );
    void result.catch(() => {});
    return result;
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    this.closing = new Promise<void>((accept, fail) => {
      resolve = accept;
      reject = fail;
    });
    void this.closing.catch(() => {});
    try {
      void this.root
        .unmount()
        .then(() => this.finishOwned())
        .then(() => {
          this.session.release(this);
          resolve();
        }, reject);
    } catch (error) {
      reject(error);
    }
    return this.closing;
  }

  /** Close, and destroy an owned World unless the Canvas itself is closing. */
  remove(): Promise<void> {
    if (!this.closing && this.owned && !this.session.isClosing)
      this.destroy = true;
    return this.close();
  }

  private async finishOwned(): Promise<void> {
    if (!this.owned) return;
    if (!this.owned.client.closure) await this.owned.client.close();
    if (this.destroy) await this.session.host.destroyWorld(this.owned.world);
    this.destroy = false;
  }

  async retry(): Promise<void> {
    if (!this.closing) return this.close();
    await this.closing.catch(() => {});
    await rootCleanup.get(this.root)!.retry();
    await this.finishOwned();
    this.session.release(this);
  }

  async abandon(): Promise<void> {
    await this.close().catch(() => {});
    await rootCleanup.get(this.root)!.abandon();
    if (this.owned && !this.owned.client.closure)
      await this.owned.client.close().catch(() => {});
    this.session.release(this);
  }
}

/** Canvas state a CanvasWorld asks for; an omitted value keeps the current one. */
export interface CanvasStateRequest {
  readonly extent?: readonly [number, number];
  readonly unitsPerMetre?: number;
}

/**
 * The World of a root-presented CanvasWorld. It creates a World selecting the
 * Canvas System with the requested initial canvas state, authors it through
 * its own session, presents its canvas as the Canvas root, and sends a Canvas
 * state update for each later changed value. Removal while the Canvas stays
 * open destroys the World; closing the Canvas leaves it to the Host.
 */
export class OwnedCanvasWorld {
  readonly closed: Promise<void>;
  private readonly opening: Promise<void>;
  private binding: CanvasWorldBinding | undefined;
  private world: WorldReference | undefined;
  private client: Client | undefined;
  private state: CanvasState | undefined;
  private children: ReactNode = null;
  private releaseRoot: (() => void) | undefined;
  private removal: Promise<void> | undefined;
  private resolveClosed!: () => void;
  private rejectClosed!: (error: unknown) => void;

  constructor(
    private readonly session: CanvasWorldSession,
    create: Omit<WorldCreateOptions, "canvas">,
    private request: CanvasStateRequest,
    private readonly report: (error: Error) => void,
    ready: (world: WorldReference, closed: Promise<void>) => void,
    private readonly reportDeclaration: (error: Error) => void,
  ) {
    this.closed = new Promise<void>((resolve, reject) => {
      this.resolveClosed = resolve;
      this.rejectClosed = reject;
    });
    void this.closed.catch(() => {});
    this.opening = this.open(create, ready);
    void this.opening.catch((error: unknown) => {
      if (!this.removal) report(asError(error));
    });
    const unsubscribe = session.onClosing(() => {
      void this.remove().catch(() => {});
    });
    void this.closed.then(unsubscribe, unsubscribe);
  }

  private get stopped(): boolean {
    return this.removal !== undefined || this.session.isClosing;
  }

  private async open(
    create: Omit<WorldCreateOptions, "canvas">,
    ready: (world: WorldReference, closed: Promise<void>) => void,
  ): Promise<void> {
    const state: CanvasState = {
      extent: this.request.extent ?? [1, 1],
      unitsPerMetre: this.request.unitsPerMetre ?? 1,
    };
    const created = await this.session.host.createWorld({
      ...create,
      canvas: state,
    });
    this.world = created.reference;
    this.state = state;
    if (this.stopped) return;
    this.client = await this.session.host.openWorld(this.world);
    if (this.stopped) return;
    this.binding = this.session.attach(this.reportDeclaration, {
      world: this.world,
      client: this.client,
    });
    this.update(this.request);
    void this.binding.render(this.children);
    this.releaseRoot = this.session.claimRoot(canvasOutput(this.world));
    ready(this.world, this.closed);
  }

  render(children: ReactNode): void {
    this.children = children;
    if (this.binding && !this.binding.isClosing && !this.stopped)
      void this.binding.render(children);
  }

  /** Send the requested values that differ from the World's canvas state. */
  update(request: CanvasStateRequest): void {
    this.request = request;
    const state = this.state;
    const client = this.client as
      | (Client & { sendCommand?(command: CanvasStateUpdateCommand): void })
      | undefined;
    if (!state || !client || !this.binding || this.stopped) return;
    const update = canvasStateUpdate(request, state);
    if (!update) return;
    try {
      if (!client.sendCommand)
        throw new Error("The canvas World client cannot send System commands");
      client.sendCommand(update.command);
      this.state = update.state;
    } catch (error) {
      this.report(asError(error));
    }
  }

  /** Stop presenting, unmount the declarations and destroy the World. */
  remove(): Promise<void> {
    if (this.removal) return this.removal;
    this.releaseRoot?.();
    this.releaseRoot = undefined;
    const removal = (async () => {
      await this.opening.catch(() => {});
      if (this.binding) return this.binding.remove();
      if (this.client && !this.client.closure) await this.client.close();
      if (this.world && !this.session.isClosing)
        await this.session.host.destroyWorld(this.world);
    })();
    this.removal = removal;
    void removal.then(this.resolveClosed, this.rejectClosed);
    return removal;
  }
}
