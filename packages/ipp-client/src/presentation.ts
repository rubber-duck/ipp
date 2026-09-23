/** Completed GPU output, separate from the runtime command protocol. */
export interface FrameCapture {
  session: bigint;
  tick: bigint;
  width: number;
  height: number;
  /** RGBA8 sRGB pixels, row zero at the top of the image. */
  pixels: ArrayBuffer;
  drawCalls: number;
  triangles: number;
  contextGeneration: number;
  backend: Record<string, unknown>;
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
 * `FrameCapture.backend.surfaceCaches` in render builds with Surfaces.
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

/**
 * Bounds of the renderer's shared glyph atlas on one graphics context. Limits
 * survive context loss and apply at the renderer's next glyph demand publication.
 */
export interface GlyphAtlasLimits {
  /** Resident page budget, at least one; allocation beyond it reclaims pages. */
  maxPages: number;
  /** Demand publications a page without demand stays resident before it retires. */
  idlePagePublications: number;
}

/** Optional presentation controls of a host with an attached canvas. */
export interface ClientPresentation {
  capture(afterTick?: bigint): Promise<FrameCapture>;
  resize(width: number, height: number): void;
  loseContext(): void;
  restoreContext(): void;
  /** Requires a GUI render build; others fail the presentation. */
  setGlyphAtlasLimits(limits: GlyphAtlasLimits): void;
  /**
   * Bound resident whole-Surface cache image bytes on this graphics context;
   * zero disables caching. Requires a render build with Surfaces.
   */
  setSurfaceCacheBudget(bytes: number): void;
}

export interface Presentation {
  capture(
    session: bigint,
    afterTick: bigint,
    timeoutMs: number,
  ): Promise<FrameCapture>;
  resize(width: number, height: number): void;
  loseContext(): void;
  restoreContext(): void;
  setGlyphAtlasLimits(limits: GlyphAtlasLimits): void;
  setSurfaceCacheBudget(bytes: number): void;
}

export const MAX_CAPTURE_DIMENSION = 2_048;

export function validateViewport(width: number, height: number): void {
  if (
    ![width, height].every(
      (value) =>
        Number.isInteger(value) && value > 0 && value <= MAX_CAPTURE_DIMENSION,
    )
  ) {
    throw new RangeError(
      `Viewport dimensions must be integers in 1..=${MAX_CAPTURE_DIMENSION}`,
    );
  }
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

interface CaptureWaiter {
  session: bigint;
  afterTick: bigint;
  resolve(frame: FrameCapture): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

/** Bounded request bookkeeping for the dedicated worker's presentation channel. */
export class PortPresentation implements Presentation {
  private readonly pending = new Map<number, CaptureWaiter>();
  private nextId = 1;

  constructor(private readonly send: (message: unknown) => void) {}

  capture(
    session: bigint,
    afterTick: bigint,
    timeoutMs: number,
  ): Promise<FrameCapture> {
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > 60_000) {
      return Promise.reject(
        new RangeError("Capture timeout must be in (0, 60000]"),
      );
    }
    if (session <= 0n || afterTick < 0n || afterTick > 0xffff_ffff_ffff_ffffn) {
      return Promise.reject(new RangeError("Invalid capture session or frame"));
    }
    if (this.pending.size >= 4)
      return Promise.reject(new Error("Capture queue is full"));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        try {
          this.send({ type: "capture-cancel", id });
        } catch {
          /* A closed host already owns cleanup. */
        }
        reject(new Error("Frame capture timed out"));
      }, timeoutMs);
      this.pending.set(id, { session, afterTick, resolve, reject, timer });
      try {
        this.send({ type: "capture", id, session, afterTick });
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

  loseContext(): void {
    this.send({ type: "context-loss" });
  }

  restoreContext(): void {
    this.send({ type: "context-restore" });
  }

  setGlyphAtlasLimits(limits: GlyphAtlasLimits): void {
    validateGlyphAtlasLimits(limits);
    this.send({
      type: "glyph-atlas-limits",
      maxPages: limits.maxPages,
      idlePagePublications: limits.idlePagePublications,
    });
  }

  setSurfaceCacheBudget(bytes: number): void {
    validateSurfaceCacheBudget(bytes);
    this.send({ type: "surface-cache-budget", bytes });
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type !== "capture-result" && data.type !== "capture-error")
      return false;
    if (
      !Number.isSafeInteger(data.id) ||
      (data.id as number) <= 0 ||
      (data.id as number) >= this.nextId
    ) {
      throw new Error("Invalid capture response identity");
    }
    const id = data.id as number;
    const waiter = this.pending.get(id);
    // A timed-out readback may already be in transit. It cannot resolve another request.
    if (!waiter) return true;
    if (data.type === "capture-error") {
      if (typeof data.message !== "string")
        throw new Error("Invalid capture diagnostic");
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
      !(frame.pixels instanceof ArrayBuffer)
    ) {
      throw new Error("Invalid or stale captured frame");
    }
    validateViewport(frame.width!, frame.height!);
    if (
      frame.pixels.byteLength !== frame.width! * frame.height! * 4 ||
      ![frame.drawCalls, frame.triangles, frame.contextGeneration].every(
        (value) => Number.isSafeInteger(value) && value! >= 0,
      ) ||
      typeof frame.backend !== "object" ||
      frame.backend === null
    ) {
      throw new Error("Invalid captured frame layout");
    }
    clearTimeout(waiter.timer);
    this.pending.delete(id);
    waiter.resolve(frame as FrameCapture);
    return true;
  }

  close(error: Error): void {
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.pending.clear();
  }
}
