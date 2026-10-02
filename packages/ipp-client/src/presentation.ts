/** Largest drawing buffer the attached graphics device accepts, per axis. */
export interface ViewportLimits {
  maxWidth: number;
  maxHeight: number;
}

/**
 * Renderer statistics, grouped by the capability that produces them; every
 * build compiles every group. Per-frame
 * counters describe the latest completed draw at observation, not a capture fence; `total*` counters accumulate in the
 * runtime over every rendered frame of this presentation, so compare two
 * observations to measure work between them.
 */
export interface RenderStatisticsSnapshot {
  /** Platform time spent finishing GPU work and reading the most recent capture. */
  readbackMs: number;
  frame: FrameRenderStatistics;
  shadows: ShadowRenderStatistics;
  gui: GuiRenderStatistics;
  /** Ordinary-layout evaluation across the Host, not a selected-output or draw statistic. */
  guiLayout?: HostGuiLayoutStatistics | null;
  surfaces: SurfaceRenderStatistics;
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
  /** Actual ordinary reflows in the last Host frame; absent before a supported sample. */
  guiLayoutReflows?: number;
  guiTextMeasurements?: number;
  /** Whole-Host lifetime totals, including retired Worlds; membership is in guiLayout. */
  totalGuiLayoutReflows?: number;
  totalGuiTextMeasurements?: number;
}

export interface GuiLayoutWorkStatistics {
  readonly reflows: number;
  readonly visitedEntities: number;
  readonly textMeasurements: number;
  readonly reusedTexts: number;
}

/** Diagnostics use decimal identity strings and saturating u32 work counters on both backends. */
export interface HostGuiLayoutStatistics {
  readonly scope: "host";
  readonly frame: string;
  readonly complete: boolean;
  readonly retiredWorlds: number;
  readonly retired: GuiLayoutWorkStatistics;
  readonly latest: GuiLayoutWorkStatistics | null;
  readonly total: GuiLayoutWorkStatistics | null;
  readonly worlds: readonly {
    readonly world: { readonly id: string; readonly incarnation: string };
    readonly tick: string;
    readonly status: "evaluated" | "unavailable";
    /** Null when ordinary GUI layout is not selected; unevaluated latest work is never repeated. */
    readonly layout: {
      readonly latest: GuiLayoutWorkStatistics | null;
      readonly total: GuiLayoutWorkStatistics;
    } | null;
  }[];
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
  | "animated"
  | "layered";

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
  "layered",
];

/**
 * Read-only whole-Surface cache state of one opted-in Surface, reported by
 * `RenderStatisticsSnapshot.surfaces`.
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

/** Read-only renderer observations; never a frame or capture fence. */
export interface RenderDiagnostics {
  statistics(): Promise<RenderStatisticsSnapshot>;
}

/** Worker control messages of `@ipp/client/testing`, honoured only by instrumentation builds. */
export type PresentationTestingMessage =
  | { type: "context-loss" }
  | { type: "context-restore" }
  | {
      type: "glyph-atlas-limits";
      maxPages: number;
      idlePageFrames: number;
    }
  | { type: "surface-cache-budget"; bytes: number }
  | { type: "exhaustive-draw-checks"; enabled: boolean };

/**
 * The statistics and control channel of one presentation, reached by
 * `@ipp/client/diagnostics` and `@ipp/client/testing` rather than through a
 * Host client member.
 */
export interface PresentationPort extends RenderDiagnostics {
  /**
   * Whether the presenting build is an `instrumentation` build that honours
   * testing controls; undefined until the presentation reports its configuration.
   */
  readonly instrumentation: boolean | undefined;
  /** Send a control message; throws once the presentation has failed or closed. */
  post(message: PresentationTestingMessage): void;
}

/**
 * Registered symbol linking a Host client, transport or statistics object to
 * its presentation port. Those entry points are bundled separately from
 * generated clients, so a registered symbol, not module state, carries the link.
 */
const PRESENTATION = Symbol.for("ipp.presentation");

/** Link `target` to `port`, or leave it unlinked when there is none. */
export function bindPresentation<T extends object>(
  target: T,
  port: PresentationPort | undefined,
): T {
  if (port)
    Object.defineProperty(target, PRESENTATION, {
      value: port,
      configurable: true,
    });
  return target;
}

/** The presentation port linked to `target`, if it presents. */
export function presentationOf(target: object): PresentationPort | undefined {
  return (target as { [PRESENTATION]?: PresentationPort })[PRESENTATION];
}

/**
 * Testing bounds of the renderer's shared glyph atlas on one graphics context,
 * replacing the renderer-owned budget. They survive context loss and apply at
 * the renderer's next frame.
 */
export interface GlyphAtlasLimits {
  /** Resident page budget, at least one; allocation beyond it reclaims pages. */
  maxPages: number;
  /** Host frames a page without demand stays resident before it retires. */
  idlePageFrames: number;
}

export function validateGlyphAtlasLimits(limits: GlyphAtlasLimits): void {
  const { maxPages, idlePageFrames } = limits;
  if (
    !Number.isInteger(maxPages) ||
    maxPages < 1 ||
    maxPages > 0xffff_ffff ||
    !Number.isInteger(idlePageFrames) ||
    idlePageFrames < 0 ||
    idlePageFrames > 0xffff_ffff
  ) {
    throw new RangeError(
      "Glyph atlas limits must be integers: maxPages in 1..=2^32-1 and idlePageFrames in 0..=2^32-1",
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

/** Diagnostic messages are independent of presentation completion and pixel transfers. */
export class PortRenderDiagnostics implements PresentationPort {
  private nextId = 1;
  private stopped: Error | undefined;
  private configured: boolean | undefined;
  private readonly pending = new Map<
    number,
    {
      resolve(value: RenderStatisticsSnapshot): void;
      reject(error: Error): void;
      timer: ReturnType<typeof setTimeout>;
    }
  >();

  constructor(private readonly send: (message: unknown) => void) {
    bindPresentation(this, this);
  }

  get instrumentation(): boolean | undefined {
    return this.configured;
  }

  post(message: PresentationTestingMessage): void {
    if (this.stopped) throw this.stopped;
    this.send(message);
  }

  statistics(): Promise<RenderStatisticsSnapshot> {
    if (this.stopped) return Promise.reject(this.stopped);
    if (this.pending.size >= 4)
      return Promise.reject(new Error("Diagnostics queue full"));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("Diagnostics timed out"));
      }, 5000);
      this.pending.set(id, { resolve, reject, timer });
      try {
        this.send({ type: "render-statistics", id });
      } catch (error) {
        this.close(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "viewport-limits") {
      validateViewportLimits(data.limits);
      return true;
    }
    if (data.type === "presentation-configuration") {
      if (typeof data.instrumentation !== "boolean")
        throw new Error("Invalid presentation configuration");
      this.configured = data.instrumentation;
      return true;
    }
    if (data.type !== "render-statistics") return false;
    if (
      !Number.isSafeInteger(data.id) ||
      (data.id as number) <= 0 ||
      (data.id as number) >= this.nextId ||
      typeof data.statistics !== "object" ||
      data.statistics === null
    )
      throw new Error("Invalid diagnostics response");
    const waiter = this.pending.get(data.id as number);
    if (waiter) {
      clearTimeout(waiter.timer);
      this.pending.delete(data.id as number);
      waiter.resolve(data.statistics as RenderStatisticsSnapshot);
    }
    return true;
  }

  close(error: Error): void {
    this.stopped ??= error;
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.pending.clear();
  }
}
