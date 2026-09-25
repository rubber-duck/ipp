/** Work of one completed frame, observed without reading pixels back. */
export interface FrameSummary {
  session: bigint;
  tick: bigint;
  /** Drawing-buffer size the frame rendered at, bounded by the device limits. */
  width: number;
  height: number;
  drawCalls: number;
  triangles: number;
  /** Instances skipped after a mesh upload failure. */
  failedDrawCalls: number;
  /** Whether the frame cleared because its camera was unusable. */
  invalidCamera: boolean;
  contextGeneration: number;
}

/** Completed GPU output, separate from the runtime command protocol. */
export interface FrameCapture extends FrameSummary {
  /** RGBA8 sRGB pixels of the full drawing buffer, row zero at the top. */
  pixels: ArrayBuffer;
  /**
   * Renderer, device and ingress statistics, read inside this capture. Present
   * only when the runtime is a `diagnostics` build; never a readiness signal.
   */
  statistics?: RenderStatisticsSnapshot;
}

/** Largest drawing buffer the attached graphics device accepts, per axis. */
export interface ViewportLimits {
  maxWidth: number;
  maxHeight: number;
}

/**
 * Optional statistics of a `diagnostics` runtime build, grouped by the
 * capability that produces them. A group is absent when its capability is
 * compiled out: absent counters are unavailable, never zero work. Per-frame
 * counters describe the captured frame; `total*` counters accumulate in the
 * runtime over every rendered frame of this presentation, so compare two
 * captures to measure work between them.
 */
export interface RenderStatisticsSnapshot {
  /** Worker time spent finishing GPU work and reading the pixels back. */
  readbackMs: number;
  frame: FrameRenderStatistics;
  shadows?: ShadowRenderStatistics;
  gui?: GuiRenderStatistics;
  surfaces?: SurfaceRenderStatistics;
  /** Absent when the worker hosts no presentation ingress. */
  ingress?: IngressStatistics;
  device: RenderDeviceInfo;
}

export interface FrameRenderStatistics {
  /** GPU upload bytes attributed to the frame, including uploads between renders. */
  uploadedBytes: number;
  totalUploadedBytes: number;
  unshadowedLights: number;
}

export interface ShadowRenderStatistics {
  shadowDrawCalls: number;
  shadowResidentBytes: number;
}

export interface GuiRenderStatistics {
  guiBatches: number;
  guiRebuilds: number;
  guiAllocations: number;
  guiResidentBytes: number;
  glyphMisses: number;
  glyphPopulates: number;
  glyphPopulationFailures: number;
  glyphPageRetirements: number;
  glyphPages: number;
  glyphResidentBytes: number;
  totalGuiRebuilds: number;
  totalGuiAllocations: number;
  totalGlyphMisses: number;
  totalGlyphPopulates: number;
  totalGlyphPopulationFailures: number;
  totalGlyphPageRetirements: number;
  /** Root reflows of the rendered World's latest GUI layout pass. */
  guiLayoutReflows: number;
  /** Text measurements (retained-cache misses) of that pass. */
  guiTextMeasurements: number;
  /**
   * Root reflows over the rendered World's lifetime. Unlike the other
   * totals this follows the World, not the presentation, so compare two
   * captures of the same World.
   */
  totalGuiLayoutReflows: number;
  /** Text measurements over the rendered World's lifetime. */
  totalGuiTextMeasurements: number;
}

export interface SurfaceRenderStatistics {
  analyticGlyphResidentBytes: number;
  surfaceCacheRepaints: number;
  surfaceCacheReuses: number;
  surfaceCacheDirect: number;
  surfaceCacheFallbacks: number;
  surfaceCacheAnimated: number;
  surfaceCacheAllocations: number;
  surfaceCacheEntries: number;
  surfaceCacheResidentBytes: number;
  totalSurfaceCacheRepaints: number;
  totalSurfaceCacheReuses: number;
  totalSurfaceCacheDirect: number;
  totalSurfaceCacheFallbacks: number;
  totalSurfaceCacheAllocations: number;
  /** Whole-Surface cache state of every opted-in Surface of the captured World. */
  surfaceCaches: SurfaceCacheRecord[];
}

/** Worker ingress counters since the worker started. */
export interface IngressStatistics {
  messages: number;
  wasmCopyBytes: number;
  partsMessages: number;
  transferredAssetBytes: number;
  /** Resource source bytes delivered to the runtime. */
  sourceBytes: number;
  /** Resource source bytes staged in the worker and runtime at the capture. */
  sourceBufferedBytes: number;
  sourcePeakBufferedBytes: number;
}

/** Identity and residency of the graphics device, as the WebGL bridge reports it. */
export interface RenderDeviceInfo {
  readonly api?: string;
  readonly version?: string;
  readonly renderer?: string;
  readonly unmaskedRenderer?: string | null;
  readonly unmaskedVendor?: string | null;
  readonly shaderProgramsCreated?: number;
  readonly shaderProgramAttempts?: number;
  readonly shaderProgramsLive?: number;
  readonly [key: string]: unknown;
}

/** How an opted-in Surface was presented by the last completed frame. */
export type SurfaceCacheMode =
  | "near"
  | "interaction"
  | "fallback"
  | "unavailable"
  | "culled"
  | "reused"
  | "repainted"
  | "animated";

/** Presentation modes in the order of their renderer export codes. */
export const SURFACE_CACHE_MODES: readonly SurfaceCacheMode[] = [
  "near",
  "interaction",
  "fallback",
  "unavailable",
  "culled",
  "reused",
  "repainted",
  "animated",
];

/**
 * Read-only whole-Surface cache state of one opted-in Surface, reported by
 * `FrameCapture.statistics.surfaces` in diagnostics render builds with Surfaces.
 */
export interface SurfaceCacheRecord {
  /** Generational entity identity within the captured World. */
  entity: bigint;
  mode: SurfaceCacheMode;
  /** Selected distance band; 0 is direct. */
  band: number;
  /** Resident image size in texels; zero without an image. */
  width: number;
  height: number;
  /** Repaints and unchanged-image reuses since the entry was created. */
  repaints: number;
  reuses: number;
  /** World time of the last repaint, in milliseconds. */
  paintedAtMs: number;
  /** Resident image bytes. */
  residentBytes: number;
}

/** Optional presentation controls of a host with an attached canvas. */
export interface ClientPresentation {
  /**
   * Resolve with the next frame rendered at or after `afterTick`, reading the
   * full drawing buffer back. Readback finishes pending GPU work and copies
   * width × height × 4 bytes; prefer `frame` when pixels are not needed. While
   * an open command batch withholds presentation, or the Host is paused, no
   * frame renders; request the capture after the batch ends, or observe the
   * presented tick through `waitForFrame` instead.
   */
  capture(afterTick?: bigint): Promise<FrameCapture>;
  /** Resolve with the summary of a completed frame at or after `afterTick`, without readback. */
  frame(afterTick?: bigint): Promise<FrameSummary>;
  /**
   * Request a drawing-buffer size. The worker bounds it to the device's
   * `viewportLimits`, preserving the aspect ratio; frames report the size used.
   */
  resize(width: number, height: number): void;
  /** Limits of the attached device; undefined until the renderer first attaches. */
  readonly viewportLimits: ViewportLimits | undefined;
  /** Observe changed limits, reported at attach and after context restoration. */
  onViewportLimits(listener: (limits: ViewportLimits) => void): () => void;
}

/** Worker control messages of `@ipp/client/testing`, honoured only by diagnostics builds. */
export type PresentationTestingMessage =
  | { type: "context-loss" }
  | { type: "context-restore" }
  | {
      type: "glyph-atlas-limits";
      maxPages: number;
      idlePagePublications: number;
    }
  | { type: "surface-cache-budget"; bytes: number }
  | { type: "exhaustive-draw-checks"; enabled: boolean };

export interface Presentation {
  frame(
    session: bigint,
    afterTick: bigint,
    timeoutMs: number,
    readback: false,
  ): Promise<FrameSummary>;
  frame(
    session: bigint,
    afterTick: bigint,
    timeoutMs: number,
    readback: true,
  ): Promise<FrameCapture>;
  resize(width: number, height: number): void;
  readonly viewportLimits: ViewportLimits | undefined;
  onViewportLimits(listener: (limits: ViewportLimits) => void): () => void;
}

/**
 * Registered symbol linking a presentation object to its worker control
 * channel. The testing entry point is bundled separately from generated
 * clients, so a registered symbol, not module state, carries the link.
 */
const TESTING_CHANNEL = Symbol.for("ipp.presentation.testing");

type TestingChannel = (message: PresentationTestingMessage) => void;

/** Link `target` to the control channel that `@ipp/client/testing` uses. */
export function bindTestingChannel<T extends object>(
  target: T,
  send: TestingChannel,
): T {
  Object.defineProperty(target, TESTING_CHANNEL, { value: send });
  return target;
}

export function testingChannel(target: object): TestingChannel {
  const send = (target as { [TESTING_CHANNEL]?: TestingChannel })[
    TESTING_CHANNEL
  ];
  if (typeof send !== "function")
    throw new TypeError("Expected an IPP worker presentation");
  return send;
}

/**
 * Testing bounds of the renderer's shared glyph atlas on one graphics context,
 * replacing the renderer-owned budget. They survive context loss and apply at
 * the renderer's next glyph demand publication.
 */
export interface GlyphAtlasLimits {
  /** Resident page budget, at least one; allocation beyond it reclaims pages. */
  maxPages: number;
  /** Demand publications a page without demand stays resident before it retires. */
  idlePagePublications: number;
}

export function validateGlyphAtlasLimits(limits: GlyphAtlasLimits): void {
  const { maxPages, idlePagePublications } = limits;
  if (
    !Number.isInteger(maxPages) ||
    maxPages < 1 ||
    maxPages > 0xffff_ffff ||
    !Number.isInteger(idlePagePublications) ||
    idlePagePublications < 0 ||
    idlePagePublications > 0xffff_ffff
  ) {
    throw new RangeError(
      "Glyph atlas limits must be integers: maxPages in 1..=2^32-1 and idlePagePublications in 0..=2^32-1",
    );
  }
}

export function validateSurfaceCacheBudget(bytes: number): void {
  if (!Number.isInteger(bytes) || bytes < 0 || bytes > 0xffff_ffff)
    throw new RangeError(
      "Surface cache budget must be an integer in 0..=2^32-1",
    );
}

/** Drawing-buffer requests are positive integers; the device bounds the effective size. */
export function validateViewport(width: number, height: number): void {
  if (
    ![width, height].every(
      (value) =>
        Number.isSafeInteger(value) && value > 0 && value <= 0xffff_ffff,
    )
  ) {
    throw new RangeError("Viewport dimensions must be positive integers");
  }
}

/**
 * Largest size within `limits` for a requested drawing buffer, scaled
 * uniformly so the aspect ratio is preserved. Unknown limits leave it unchanged.
 */
export function boundViewport(
  width: number,
  height: number,
  limits: ViewportLimits | undefined,
): { width: number; height: number } {
  if (!limits) return { width, height };
  const scale = Math.min(1, limits.maxWidth / width, limits.maxHeight / height);
  if (scale >= 1) return { width, height };
  return {
    width: Math.min(limits.maxWidth, Math.max(1, Math.floor(width * scale))),
    height: Math.min(limits.maxHeight, Math.max(1, Math.floor(height * scale))),
  };
}

export function validateViewportLimits(limits: unknown): ViewportLimits {
  const candidate = limits as Partial<ViewportLimits> | undefined;
  if (
    typeof candidate !== "object" ||
    candidate === null ||
    ![candidate.maxWidth, candidate.maxHeight].every(
      (value) =>
        Number.isSafeInteger(value) && value! > 0 && value! <= 0xffff_ffff,
    )
  )
    throw new Error("Invalid viewport limits");
  return { maxWidth: candidate.maxWidth!, maxHeight: candidate.maxHeight! };
}

interface FrameWaiter {
  session: bigint;
  afterTick: bigint;
  readback: boolean;
  resolve(frame: FrameSummary | FrameCapture): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

/** Bounded request bookkeeping for the dedicated worker's presentation channel. */
export class PortPresentation implements Presentation {
  private readonly pending = new Map<number, FrameWaiter>();
  private readonly limitListeners = new Set<(limits: ViewportLimits) => void>();
  private limits: ViewportLimits | undefined;
  private nextId = 1;

  constructor(private readonly send: (message: unknown) => void) {
    bindTestingChannel(this, (message) => this.send(message));
  }

  get viewportLimits(): ViewportLimits | undefined {
    return this.limits;
  }

  onViewportLimits(listener: (limits: ViewportLimits) => void): () => void {
    this.limitListeners.add(listener);
    return () => {
      this.limitListeners.delete(listener);
    };
  }

  frame(
    session: bigint,
    afterTick: bigint,
    timeoutMs: number,
    readback: false,
  ): Promise<FrameSummary>;
  frame(
    session: bigint,
    afterTick: bigint,
    timeoutMs: number,
    readback: true,
  ): Promise<FrameCapture>;
  frame(
    session: bigint,
    afterTick: bigint,
    timeoutMs: number,
    readback: boolean,
  ): Promise<FrameSummary | FrameCapture> {
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > 60_000) {
      return Promise.reject(
        new RangeError("Frame timeout must be in (0, 60000]"),
      );
    }
    if (session <= 0n || afterTick < 0n || afterTick > 0xffff_ffff_ffff_ffffn) {
      return Promise.reject(new RangeError("Invalid frame session or tick"));
    }
    if (this.pending.size >= 4)
      return Promise.reject(new Error("Frame request queue is full"));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        try {
          this.send({ type: "frame-cancel", id });
        } catch {
          /* A closed host already owns cleanup. */
        }
        reject(
          new Error(
            readback
              ? "Frame capture timed out"
              : "Frame observation timed out",
          ),
        );
      }, timeoutMs);
      this.pending.set(id, {
        session,
        afterTick,
        readback,
        resolve,
        reject,
        timer,
      });
      try {
        this.send({ type: "frame", id, session, afterTick, readback });
      } catch (error) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(error);
      }
    });
  }

  resize(width: number, height: number): void {
    validateViewport(width, height);
    this.send({ type: "resize", width, height });
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "viewport-limits") {
      const limits = validateViewportLimits(data.limits);
      if (
        this.limits?.maxWidth === limits.maxWidth &&
        this.limits.maxHeight === limits.maxHeight
      )
        return true;
      this.limits = limits;
      for (const listener of [...this.limitListeners]) listener(limits);
      return true;
    }
    if (data.type !== "frame-result" && data.type !== "frame-error")
      return false;
    if (
      !Number.isSafeInteger(data.id) ||
      (data.id as number) <= 0 ||
      (data.id as number) >= this.nextId
    ) {
      throw new Error("Invalid frame response identity");
    }
    const id = data.id as number;
    const waiter = this.pending.get(id);
    // A timed-out readback may already be in transit. It cannot resolve another request.
    if (!waiter) return true;
    if (data.type === "frame-error") {
      if (typeof data.message !== "string")
        throw new Error("Invalid frame diagnostic");
      clearTimeout(waiter.timer);
      this.pending.delete(id);
      waiter.reject(new Error(data.message));
      return true;
    }
    const frame = data.frame as Partial<FrameCapture> | undefined;
    if (
      !frame ||
      typeof frame !== "object" ||
      frame.session !== waiter.session ||
      typeof frame.tick !== "bigint" ||
      frame.tick < waiter.afterTick ||
      typeof frame.invalidCamera !== "boolean"
    ) {
      throw new Error("Invalid or stale frame");
    }
    validateViewport(frame.width!, frame.height!);
    if (
      ![
        frame.drawCalls,
        frame.triangles,
        frame.failedDrawCalls,
        frame.contextGeneration,
      ].every((value) => Number.isSafeInteger(value) && value! >= 0) ||
      (waiter.readback
        ? !(frame.pixels instanceof ArrayBuffer) ||
          frame.pixels.byteLength !== frame.width! * frame.height! * 4 ||
          (frame.statistics !== undefined &&
            (typeof frame.statistics !== "object" || frame.statistics === null))
        : "pixels" in frame || "statistics" in frame)
    ) {
      throw new Error("Invalid frame layout");
    }
    clearTimeout(waiter.timer);
    this.pending.delete(id);
    waiter.resolve(frame as FrameSummary | FrameCapture);
    return true;
  }

  close(error: Error): void {
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.pending.clear();
    this.limitListeners.clear();
  }
}
