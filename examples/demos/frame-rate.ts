/** A low-rate observer of actual presentation completions feeding ordinary plot data. */
import {
  type DatasetProducer,
  type PresentedFrame,
  type PresentationViewport,
} from "@ipp/client";
import {
  renderDiagnostics,
  type RenderStatisticsSnapshot,
} from "@ipp/client/diagnostics";
import type { IppCanvasHandle } from "@ipp/react/canvas";

export const FRAME_RATE_SAMPLES = 600;
export const FRAME_RATE_INTERVAL_MS = 100;
export const FRAME_RATE_SCHEMA = [
  { name: "seconds", kind: "f32" },
  { name: "fps", kind: "f32" },
  { name: "frame_ms", kind: "f32" },
] as const;
let nextHistory = 0;

export interface GalleryPlatformDiagnostics {
  readonly runtime: string;
  readonly platform: string | null;
  readonly userAgent: string | null;
  readonly logicalCores: number | null;
  readonly deviceMemoryGiB: number | null;
  readonly jsHeapUsedBytes: number | null;
  readonly jsHeapLimitBytes: number | null;
}

export interface GallerySystemSnapshot {
  readonly sampledAt: number | null;
  readonly fps: number | null;
  readonly frameMs: number | null;
  readonly presentedFrames: bigint | null;
  /** Actual draw metadata when provided by the presentation adapter. */
  readonly drawCalls?: number | null;
  readonly triangles?: number | null;
  readonly failedDrawCalls?: number | null;
  readonly viewport: PresentationViewport | null;
  /** Host-wide observations; not a selected-World measurement or frame fence. */
  readonly renderer: RenderStatisticsSnapshot | null;
  readonly rendererStatus: "pending" | "available" | "unavailable";
  readonly rendererError: string | null;
  readonly platform: GalleryPlatformDiagnostics;
}

export interface GalleryDiagnosticsMonitor {
  readonly source: string;
  readonly snapshot: GallerySystemSnapshot;
  getSnapshot(): GallerySystemSnapshot;
  subscribe(listener: () => void): () => void;
  close(): Promise<void>;
}

export interface GallerySystemDiagnostics {
  acquire(): Promise<GalleryDiagnosticsMonitor>;
}

/** Optional JS facts describe this client process, never the remote Host OS. */
function platformDiagnostics(): GalleryPlatformDiagnostics {
  const environment = globalThis as typeof globalThis & {
    navigator?: {
      userAgent?: string;
      platform?: string;
      hardwareConcurrency?: number;
      deviceMemory?: number;
    };
    process?: {
      versions?: { node?: string };
      platform?: string;
      memoryUsage?: () => { heapUsed: number };
    };
    performance: Performance & {
      memory?: { usedJSHeapSize: number; jsHeapSizeLimit: number };
    };
  };
  const browser = environment.navigator;
  const node = environment.process;
  const memory = environment.performance.memory;
  return {
    runtime: node?.versions?.node ? `Node ${node.versions.node}` : "Browser JS",
    platform: node?.platform ?? browser?.platform ?? null,
    userAgent: browser?.userAgent ?? null,
    logicalCores: browser?.hardwareConcurrency ?? null,
    deviceMemoryGiB: browser?.deviceMemory ?? null,
    jsHeapUsedBytes:
      memory?.usedJSHeapSize ?? node?.memoryUsage?.().heapUsed ?? null,
    jsHeapLimitBytes: memory?.jsHeapSizeLimit ?? null,
  };
}

export class DemoFrameRate {
  readonly source: string;
  private producer: DatasetProducer | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private stopped = false;
  private writing: Promise<unknown> | undefined;
  private previous: { frame: PresentedFrame; time: number } | undefined;
  private readonly epoch = performance.now();
  private readonly listeners = new Set<() => void>();
  private state: GallerySystemSnapshot;
  private observing: Promise<void> | undefined;
  private nextDiagnosticsAt = 0;

  constructor(
    private readonly canvas: IppCanvasHandle,
    private readonly report: (error: unknown) => void,
  ) {
    this.source = `datasets://demo-frame-rate/${canvas.client.session}/${++nextHistory}`;
    this.state = {
      sampledAt: null,
      fps: null,
      frameMs: null,
      presentedFrames: null,
      drawCalls: null,
      triangles: null,
      failedDrawCalls: null,
      viewport: canvas.viewport,
      renderer: null,
      rendererStatus: "pending",
      rendererError: null,
      platform: platformDiagnostics(),
    };
  }

  get snapshot() {
    return this.state;
  }

  readonly getSnapshot = () => this.state;

  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private publish(patch: Partial<GallerySystemSnapshot>) {
    if (this.stopped) return;
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }

  async open() {
    this.producer = await this.canvas.host.datasets.create(
      this.source,
      "streaming",
      FRAME_RATE_SCHEMA,
    );
  }

  start() {
    this.observeDiagnostics();
    void this.sample();
  }

  pause() {
    this.stopped = true;
    clearTimeout(this.timer);
  }

  async close() {
    this.pause();
    this.listeners.clear();
    // A pending frame observation owns no producer work. Its continuation is
    // fenced by stopped, so teardown need not await an unselected surface.
    await this.writing?.catch(() => {});
    if (this.producer) {
      await this.canvas.host.datasets.destroy(this.producer);
      this.producer = undefined;
    }
  }

  private observeDiagnostics() {
    const now = performance.now();
    if (this.stopped || this.observing || now < this.nextDiagnosticsAt) return;
    this.nextDiagnosticsAt = now + 2000;
    const diagnostics = renderDiagnostics(this.canvas.host);
    this.publish({
      platform: platformDiagnostics(),
      viewport: this.canvas.viewport,
    });
    if (!diagnostics) {
      this.publish({
        renderer: null,
        rendererStatus: "unavailable",
        rendererError: "This connection exposes no renderer diagnostics",
      });
      return;
    }
    this.observing = diagnostics
      .statistics()
      .then(
        (renderer) =>
          this.publish({
            renderer,
            rendererStatus: "available",
            rendererError: null,
          }),
        (error: unknown) =>
          this.publish({
            renderer: null,
            rendererStatus: "unavailable",
            rendererError:
              error instanceof Error ? error.message : String(error),
          }),
      )
      .finally(() => {
        this.observing = undefined;
      });
  }

  private async sample() {
    try {
      const view = this.canvas.view;
      if (this.stopped) return;
      if (!view) {
        this.previous = undefined;
        this.publish({ fps: null, frameMs: null });
        return;
      }
      // Metadata only: no pixel readback and no simulated/React frame counter.
      const frame = await this.canvas.host.presentation.frame(view);
      if (this.stopped) return;
      const time = performance.now();
      this.observeDiagnostics();
      const previous = this.previous;
      this.previous = { frame, time };
      if (
        previous &&
        previous.frame.view.selection === frame.view.selection &&
        previous.frame.view.binding.generation.host ===
          frame.view.binding.generation.host &&
        previous.frame.view.binding.generation.serial ===
          frame.view.binding.generation.serial &&
        frame.sequence > previous.frame.sequence &&
        time > previous.time
      ) {
        const elapsed = time - previous.time;
        const frames = Number(frame.sequence - previous.frame.sequence);
        this.writing = this.canvas.host.datasets
          .update(this.producer!, [
            {
              operation: "append",
              rows: [
                [
                  { kind: "f32", value: (time - this.epoch) / 1000 },
                  { kind: "f32", value: (frames * 1000) / elapsed },
                  { kind: "f32", value: elapsed / frames },
                ],
              ],
            },
          ])
          .then((outcome) => {
            if (outcome.failure)
              throw new Error(`FPS stream: ${outcome.failure.reason}`);
          });
        await this.writing;
        this.publish({
          sampledAt: time,
          fps: (frames * 1000) / elapsed,
          frameMs: elapsed / frames,
          presentedFrames: frame.sequence,
          drawCalls: frame.drawCalls,
          triangles: frame.triangles,
          failedDrawCalls: frame.failedDrawCalls,
        });
      } else {
        this.publish({
          sampledAt: time,
          fps: null,
          frameMs: null,
          presentedFrames: frame.sequence,
          drawCalls: frame.drawCalls,
          triangles: frame.triangles,
          failedDrawCalls: frame.failedDrawCalls,
        });
      }
    } catch (error) {
      // A changed view or a suspended presentation invalidates the interval.
      // Resume with a fresh baseline, without inventing a spike from that gap.
      this.previous = undefined;
      this.publish({ fps: null, frameMs: null });
      if (
        !this.stopped &&
        !(
          typeof error === "object" &&
          error !== null &&
          "reason" in error &&
          ["staleView", "unavailable", "timeout"].includes(String(error.reason))
        )
      ) {
        this.pause();
        this.report(error);
      }
    } finally {
      if (!this.stopped)
        this.timer = setTimeout(
          () => void this.sample(),
          FRAME_RATE_INTERVAL_MS,
        );
    }
  }
}

/** One producer is shared by the overlay and scene monitors, with explicit leases. */
export class DemoDiagnostics implements GallerySystemDiagnostics {
  private history: DemoFrameRate | undefined;
  private references = 0;
  private closed = false;
  private tail: Promise<unknown> = Promise.resolve();

  constructor(
    private readonly canvas: IppCanvasHandle,
    private readonly report: (error: unknown) => void,
  ) {}

  private ordered<T>(operation: () => Promise<T>): Promise<T> {
    const pending = this.tail.then(operation);
    this.tail = pending.catch(() => {});
    return pending;
  }

  acquire(): Promise<GalleryDiagnosticsMonitor> {
    return this.ordered(async () => {
      if (this.closed) throw new Error("Demo diagnostics are closed");
      if (!this.history) {
        const history = new DemoFrameRate(this.canvas, this.report);
        await history.open();
        this.history = history;
        history.start();
      }
      const history = this.history;
      this.references++;
      let closing: Promise<void> | undefined;
      const subscriptions = new Set<() => void>();
      return {
        source: history.source,
        get snapshot() {
          return history.snapshot;
        },
        getSnapshot: history.getSnapshot,
        subscribe: (listener) => {
          if (closing) return () => {};
          const unsubscribe = history.subscribe(listener);
          subscriptions.add(unsubscribe);
          return () => {
            unsubscribe();
            subscriptions.delete(unsubscribe);
          };
        },
        close: () => {
          for (const unsubscribe of subscriptions) unsubscribe();
          subscriptions.clear();
          return (closing ??= this.ordered(async () => {
            if (this.closed || --this.references > 0) return;
            await history.close();
            this.history = undefined;
          }));
        },
      };
    });
  }

  close(): Promise<void> {
    this.closed = true;
    this.history?.pause();
    return this.ordered(async () => {
      await this.history?.close();
      this.history = undefined;
      this.references = 0;
    });
  }

  pause() {
    this.history?.pause();
  }
}
