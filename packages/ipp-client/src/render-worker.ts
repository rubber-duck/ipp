import {
  boundViewport,
  SURFACE_CACHE_MODES,
  validateGlyphAtlasLimits,
  validateSurfaceCacheBudget,
  validateViewport,
} from "./presentation.js";
import type {
  FrameCapture,
  FrameSummary,
  GlyphAtlasLimits,
  IngressStatistics,
  RenderDeviceInfo,
  RenderStatisticsSnapshot,
  SurfaceCacheRecord,
  ViewportLimits,
} from "./presentation.js";
import { DiagnosticLogger, type LogLevel } from "./logging.js";

/** Imported only by a worker initialized with an OffscreenCanvas. */
interface WebGlHostExports {
  imports: WebAssembly.Imports[string];
  setMemory(memory: WebAssembly.Memory): void;
  resize(width: number, height: number): void;
  capture(): Uint8Array<ArrayBuffer>;
  isContextLost(): boolean;
  dispose(): void;
  loseContext(): void;
  restoreContext(): void;
  info(): Record<string, unknown>;
}

interface RenderHostExports {
  memory: WebAssembly.Memory;
  ipp_render_attach(width: number, height: number): number;
  ipp_render_resize(width: number, height: number): number;
  ipp_render_detach(): void;
  ipp_render_tick(): bigint;
  ipp_render_draw_calls(): number;
  ipp_render_triangles(): number;
  ipp_render_failed_draw_calls(): number;
  ipp_render_invalid_camera(): number;
  ipp_render_max_viewport_width(): number;
  ipp_render_max_viewport_height(): number;
}

/**
 * Exports of a `diagnostics` render build. Each is optional: a missing export
 * disables its statistics or testing control and never fails initialization.
 */
interface RenderDiagnosticsExports {
  ipp_render_statistics_ptr?(): number;
  ipp_render_statistics_len?(): number;
  ipp_render_surface_cache_records_ptr?(): number;
  ipp_render_surface_cache_records_len?(): number;
  ipp_render_set_glyph_atlas_limits?(
    maxPages: number,
    idlePagePublications: number,
  ): number;
  ipp_render_set_surface_cache_budget?(bytes: number): number;
  ipp_render_set_exhaustive_draw_checks?(enabled: number): number;
}

/**
 * Word offsets of the packed statistics record exported by `ipp-wasm`
 * (`RenderStatisticsRecord` in `services/render.rs`, which documents the
 * layout). Every word is a little-endian `u32`; totals saturate.
 */
const RECORD = {
  flags: 0,
  uploadedBytes: 1,
  totalUploadedBytes: 2,
  unshadowedLights: 3,
  shadowDrawCalls: 4,
  shadowResidentBytes: 5,
  guiBatches: 6,
  guiRebuilds: 7,
  guiAllocations: 8,
  guiResidentBytes: 9,
  glyphMisses: 10,
  glyphPopulates: 11,
  glyphPopulationFailures: 12,
  glyphPageRetirements: 13,
  glyphPages: 14,
  glyphResidentBytes: 15,
  totalGuiRebuilds: 16,
  totalGuiAllocations: 17,
  totalGlyphMisses: 18,
  totalGlyphPopulates: 19,
  totalGlyphPopulationFailures: 20,
  totalGlyphPageRetirements: 21,
  analyticGlyphResidentBytes: 22,
  surfaceCacheRepaints: 23,
  surfaceCacheReuses: 24,
  surfaceCacheDirect: 25,
  surfaceCacheFallbacks: 26,
  surfaceCacheAnimated: 27,
  surfaceCacheAllocations: 28,
  surfaceCacheEntries: 29,
  surfaceCacheResidentBytes: 30,
  totalSurfaceCacheRepaints: 31,
  totalSurfaceCacheReuses: 32,
  totalSurfaceCacheDirect: 33,
  totalSurfaceCacheFallbacks: 34,
  totalSurfaceCacheAllocations: 35,
} as const;

const RECORD_WORDS = 36;

/** Capability groups compiled into the runtime, as bits of `RECORD.flags`. */
const RECORD_SHADOWS = 1;
const RECORD_GUI = 2;
const RECORD_SURFACES = 4;

/** Words per exported Surface cache record; see `ipp-wasm` `services/render.rs`. */
const SURFACE_CACHE_RECORD_WORDS = 10;

function pick<K extends keyof typeof RECORD>(
  words: Uint32Array,
  keys: readonly K[],
): Record<K, number> {
  return Object.fromEntries(
    keys.map((key) => [key, words[RECORD[key]]!]),
  ) as Record<K, number>;
}

interface FrameRequest {
  id: number;
  session: bigint;
  afterTick: bigint;
  readback: boolean;
}

export class RenderWorkerService {
  readonly imports: WebAssembly.Imports;
  private runtime: (RenderHostExports & RenderDiagnosticsExports) | undefined;
  /** Present for `diagnostics` builds, which export the packed statistics record. */
  private ingress: IngressStatistics | undefined;
  /** Latest testing atlas bounds, applied again for a later World session. */
  private glyphAtlasLimits: GlyphAtlasLimits | undefined;
  /** Latest testing cache image budget, applied again for a later World session. */
  private surfaceCacheBudget: number | undefined;
  private exhaustiveDrawChecks: boolean | undefined;
  private session = 0n;
  private generation = 0;
  /** Render tick before this frame's evaluation, read only while requests wait. */
  private tickBeforeFrame: bigint | undefined;
  private attached = false;
  private closed = false;
  private lossObserved = false;
  private restoreRequested = false;
  private restoreTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly requests = new Map<number, FrameRequest>();
  /** Latest requested drawing-buffer size; the device limits bound the size used. */
  private requestedWidth: number;
  private requestedHeight: number;
  private limits: ViewportLimits | undefined;

  private constructor(
    private readonly canvas: OffscreenCanvas,
    private readonly device: WebGlHostExports,
    private readonly post: (message: unknown, transfer: Transferable[]) => void,
    private readonly fail: (error: Error) => void,
    private readonly logger: DiagnosticLogger,
  ) {
    this.requestedWidth = canvas.width;
    this.requestedHeight = canvas.height;
    this.imports = { ipp_gl: device.imports };
    canvas.addEventListener("webglcontextlost", this.lost);
    canvas.addEventListener("webglcontextrestored", this.restored);
  }

  static async create(
    canvas: OffscreenCanvas,
    wasmUrl: string,
    post: (message: unknown, transfer: Transferable[]) => void,
    fail: (error: Error) => void,
    logLevel: LogLevel = "info",
  ): Promise<RenderWorkerService> {
    validateViewport(canvas.width, canvas.height);
    // Only the render distribution ships this binding. The lean worker has no GL dependency.
    const module = (await import(new URL("webgl.js", wasmUrl).href)) as {
      createWebGlDevice(canvas: OffscreenCanvas): WebGlHostExports;
    };
    return new RenderWorkerService(
      canvas,
      module.createWebGlDevice(canvas),
      post,
      fail,
      new DiagnosticLogger("renderer", logLevel),
    );
  }

  /** Whether the runtime is a `diagnostics` build that reports statistics. */
  reportsStatistics(exports: object): boolean {
    const candidate = exports as Record<string, unknown>;
    return (
      typeof candidate.ipp_render_statistics_ptr === "function" &&
      typeof candidate.ipp_render_statistics_len === "function"
    );
  }

  initialize(
    exports: object,
    session: bigint,
    ingress: IngressStatistics | undefined,
  ): void {
    const candidate = exports as Record<string, unknown>;
    for (const name of [
      "ipp_render_attach",
      "ipp_render_resize",
      "ipp_render_detach",
      "ipp_render_tick",
      "ipp_render_draw_calls",
      "ipp_render_triangles",
      "ipp_render_failed_draw_calls",
      "ipp_render_invalid_camera",
      "ipp_render_max_viewport_width",
      "ipp_render_max_viewport_height",
    ]) {
      if (typeof candidate[name] !== "function")
        throw new Error(
          `WASM runtime is missing ${name}; select the render build`,
        );
    }
    this.runtime = exports as RenderHostExports & RenderDiagnosticsExports;
    this.ingress = ingress;
    this.session = session;
    this.tickBeforeFrame = undefined;
    this.device.setMemory(this.runtime.memory);
    this.applyTestingOverrides();
    if (!this.device.isContextLost()) this.attach();
  }

  /** The renderer keeps overrides through context loss; a later session receives them again. */
  private applyTestingOverrides(): void {
    const runtime = this.runtime;
    if (!runtime) return;
    const limits = this.glyphAtlasLimits;
    if (limits) {
      const apply = this.testingExport(
        runtime.ipp_render_set_glyph_atlas_limits,
        "glyph atlas limits",
        "a GUI render build",
      );
      if (apply(limits.maxPages, limits.idlePagePublications) !== 1)
        throw new Error("Rust renderer rejected the glyph atlas limits");
      this.logger.log("debug", "renderer.glyph_atlas_limits", () => ({
        session: this.session,
        maxPages: limits.maxPages,
        idlePagePublications: limits.idlePagePublications,
      }));
    }
    const bytes = this.surfaceCacheBudget;
    if (bytes !== undefined) {
      const apply = this.testingExport(
        runtime.ipp_render_set_surface_cache_budget,
        "Surface cache budget",
        "a render build with Surfaces",
      );
      if (apply(bytes >>> 0) !== 1)
        throw new Error("Rust renderer rejected the Surface cache budget");
      this.logger.log("debug", "renderer.surface_cache_budget", () => ({
        session: this.session,
        bytes,
      }));
    }
    const enabled = this.exhaustiveDrawChecks;
    if (enabled !== undefined) {
      const apply = this.testingExport(
        runtime.ipp_render_set_exhaustive_draw_checks,
        "exhaustive draw checks",
        "a render build",
      );
      if (apply(Number(enabled)) !== 1)
        throw new Error("Rust renderer rejected exhaustive draw checks");
    }
  }

  private testingExport<F extends (...args: never[]) => number>(
    candidate: F | undefined,
    control: string,
    build: string,
  ): F {
    if (typeof candidate !== "function")
      throw new Error(
        `The ${control} testing override requires a diagnostics runtime of ${build}`,
      );
    return candidate.bind(this.runtime) as F;
  }

  /** Read the device limits, reporting a change to the presentation channel. */
  private refreshLimits(): void {
    const runtime = this.runtime;
    if (!runtime) return;
    const maxWidth = runtime.ipp_render_max_viewport_width() >>> 0;
    const maxHeight = runtime.ipp_render_max_viewport_height() >>> 0;
    // Zero means the renderer has no device context to ask.
    if (maxWidth === 0 || maxHeight === 0) return;
    if (
      this.limits?.maxWidth === maxWidth &&
      this.limits.maxHeight === maxHeight
    )
      return;
    this.limits = { maxWidth, maxHeight };
    this.post({ type: "viewport-limits", limits: this.limits }, []);
  }

  private get viewport(): { width: number; height: number } {
    return boundViewport(
      this.requestedWidth,
      this.requestedHeight,
      this.limits,
    );
  }

  private attach(): void {
    if (!this.runtime || this.closed) return;
    this.refreshLimits();
    // Layout can change while the context is unavailable. Apply the latest
    // surface intent before rebuilding graphics in the existing world session.
    const { width, height } = this.viewport;
    this.device.resize(width, height);
    if (this.runtime.ipp_render_attach(width, height) !== 1)
      throw new Error("Rust renderer initialization failed");
    this.attached = true;
    this.lossObserved = false;
    this.generation++;
    this.logger.log(
      "info",
      this.generation === 1 ? "renderer.initialized" : "renderer.restored",
      () => ({
        session: this.session,
        generation: this.generation,
        width: this.canvas.width,
        height: this.canvas.height,
      }),
    );
  }

  private readonly lost = (event: Event): void => {
    event.preventDefault();
    if (this.closed || this.lossObserved) return;
    this.suspend();
    this.lossObserved = true;
    this.logger.log("info", "renderer.lost", () => ({
      session: this.session,
      generation: this.generation,
    }));
    this.restoreIfRequested();
  };

  private readonly restored = (): void => {
    if (this.closed) return;
    try {
      this.attach();
    } catch (error) {
      this.fail(asError(error));
    }
  };

  private suspend(): void {
    if (this.attached) this.runtime?.ipp_render_detach();
    this.attached = false;
  }

  private restoreIfRequested(): void {
    if (
      !this.restoreRequested ||
      !this.lossObserved ||
      this.closed ||
      this.restoreTimer !== undefined
    )
      return;
    // WEBGL_lose_context permits restoration after the cancelled loss event has
    // finished dispatching. The caller need not guess that event's timing.
    this.restoreTimer = setTimeout(() => {
      this.restoreTimer = undefined;
      this.restoreRequested = false;
      try {
        this.device.restoreContext();
      } catch (error) {
        this.fail(asError(error));
      }
    }, 0);
  }

  beforeFrame(): void {
    // Loss may become observable before the browser dispatches its event.
    if (this.device.isContextLost()) this.suspend();
  }

  /** Called by the frame loop immediately before it evaluates the World. */
  beforeTick(): void {
    this.beforeFrame();
    // Most frames have no pending request; they make no renderer calls.
    this.tickBeforeFrame =
      this.requests.size !== 0 && this.attached
        ? this.runtime?.ipp_render_tick()
        : undefined;
  }

  afterFrame(): void {
    const before = this.tickBeforeFrame;
    this.tickBeforeFrame = undefined;
    if (this.requests.size === 0) return;
    const runtime = this.runtime;
    if (!runtime || !this.attached || this.device.isContextLost()) return;
    const tick = runtime.ipp_render_tick();
    if (tick === 0n) return;
    // The drawing buffer holds a frame only until the browser composites it,
    // after this task; read back only a frame rendered in this task.
    const rendered = before !== undefined && tick !== before;
    for (const [id, request] of this.requests) {
      if (tick < request.afterTick || (request.readback && !rendered)) continue;
      this.requests.delete(id);
      if (!request.readback) {
        this.post({ type: "frame-result", id, frame: this.summary(tick) }, []);
        continue;
      }
      // capture finishes pending GPU work and copies top-left RGBA pixels.
      const readbackStarted = performance.now();
      const pixels = this.device.capture();
      const readbackMs = performance.now() - readbackStarted;
      const statistics = this.statistics(runtime, readbackMs);
      const frame: FrameCapture = {
        ...this.summary(tick),
        pixels: pixels.buffer,
        ...(statistics ? { statistics } : {}),
      };
      this.post({ type: "frame-result", id, frame }, [pixels.buffer]);
    }
  }

  private summary(tick: bigint): FrameSummary {
    const runtime = this.runtime!;
    return {
      session: this.session,
      tick,
      width: this.canvas.width,
      height: this.canvas.height,
      drawCalls: runtime.ipp_render_draw_calls(),
      triangles: runtime.ipp_render_triangles(),
      failedDrawCalls: runtime.ipp_render_failed_draw_calls(),
      invalidCamera: runtime.ipp_render_invalid_camera() !== 0,
      contextGeneration: this.generation,
    };
  }

  /** Read the packed record of a diagnostics build; only captures call this. */
  private statistics(
    runtime: RenderHostExports & RenderDiagnosticsExports,
    readbackMs: number,
  ): RenderStatisticsSnapshot | undefined {
    if (
      !runtime.ipp_render_statistics_ptr ||
      !runtime.ipp_render_statistics_len
    )
      return undefined;
    const pointer = runtime.ipp_render_statistics_ptr() >>> 0;
    const length = runtime.ipp_render_statistics_len() >>> 0;
    if (pointer === 0 || length !== RECORD_WORDS)
      throw new Error(
        `Render statistics record has ${length} words; this worker reads ${RECORD_WORDS}`,
      );
    const words = new Uint32Array(
      runtime.memory.buffer,
      pointer,
      RECORD_WORDS,
    ).slice();
    const flags = words[RECORD.flags]!;
    return {
      readbackMs,
      frame: pick(words, [
        "uploadedBytes",
        "totalUploadedBytes",
        "unshadowedLights",
      ]),
      ...(flags & RECORD_SHADOWS
        ? {
            shadows: pick(words, ["shadowDrawCalls", "shadowResidentBytes"]),
          }
        : {}),
      ...(flags & RECORD_GUI
        ? {
            gui: pick(words, [
              "guiBatches",
              "guiRebuilds",
              "guiAllocations",
              "guiResidentBytes",
              "glyphMisses",
              "glyphPopulates",
              "glyphPopulationFailures",
              "glyphPageRetirements",
              "glyphPages",
              "glyphResidentBytes",
              "totalGuiRebuilds",
              "totalGuiAllocations",
              "totalGlyphMisses",
              "totalGlyphPopulates",
              "totalGlyphPopulationFailures",
              "totalGlyphPageRetirements",
            ]),
          }
        : {}),
      ...(flags & RECORD_SURFACES
        ? {
            surfaces: {
              ...pick(words, [
                "analyticGlyphResidentBytes",
                "surfaceCacheRepaints",
                "surfaceCacheReuses",
                "surfaceCacheDirect",
                "surfaceCacheFallbacks",
                "surfaceCacheAnimated",
                "surfaceCacheAllocations",
                "surfaceCacheEntries",
                "surfaceCacheResidentBytes",
                "totalSurfaceCacheRepaints",
                "totalSurfaceCacheReuses",
                "totalSurfaceCacheDirect",
                "totalSurfaceCacheFallbacks",
                "totalSurfaceCacheAllocations",
              ]),
              surfaceCaches: this.surfaceCacheRecords(runtime),
            },
          }
        : {}),
      ...(this.ingress ? { ingress: { ...this.ingress } } : {}),
      device: this.device.info() as RenderDeviceInfo,
    };
  }

  /** Records the runtime builds on demand for the last completed frame. */
  private surfaceCacheRecords(
    runtime: RenderHostExports & RenderDiagnosticsExports,
  ): SurfaceCacheRecord[] {
    if (
      !runtime.ipp_render_surface_cache_records_ptr ||
      !runtime.ipp_render_surface_cache_records_len
    )
      throw new Error(
        "WASM runtime reports Surface statistics without Surface cache records",
      );
    // The pointer export builds the records; read the length after it.
    const pointer = runtime.ipp_render_surface_cache_records_ptr() >>> 0;
    const length = runtime.ipp_render_surface_cache_records_len() >>> 0;
    if (length % SURFACE_CACHE_RECORD_WORDS !== 0)
      throw new Error("Surface cache records are not whole records");
    // Copy immediately: the runtime reuses this storage for its next read.
    const words =
      length === 0
        ? new Uint32Array(0)
        : new Uint32Array(runtime.memory.buffer, pointer, length).slice();
    const records: SurfaceCacheRecord[] = [];
    for (let at = 0; at < words.length; at += SURFACE_CACHE_RECORD_WORDS) {
      const mode = SURFACE_CACHE_MODES[words[at + 2]!];
      if (!mode) throw new Error("Unknown Surface cache presentation code");
      records.push({
        entity: BigInt(words[at]!) | (BigInt(words[at + 1]!) << 32n),
        mode,
        band: words[at + 3]!,
        width: words[at + 4]!,
        height: words[at + 5]!,
        repaints: words[at + 6]!,
        reuses: words[at + 7]!,
        paintedAtMs: words[at + 8]!,
        residentBytes: words[at + 9]!,
      });
    }
    return records;
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "frame") {
      if (
        !Number.isSafeInteger(data.id) ||
        (data.id as number) <= 0 ||
        typeof data.session !== "bigint" ||
        typeof data.afterTick !== "bigint" ||
        typeof data.readback !== "boolean" ||
        data.afterTick < 0n ||
        data.afterTick > 0xffff_ffff_ffff_ffffn
      )
        throw new Error("Invalid frame request");
      const id = data.id as number;
      if (
        data.session !== this.session ||
        this.requests.size >= 4 ||
        this.requests.has(id)
      ) {
        this.post(
          {
            type: "frame-error",
            id,
            message: "Frame session mismatch or queue full",
          },
          [],
        );
      } else
        this.requests.set(id, {
          id,
          session: data.session,
          afterTick: data.afterTick,
          readback: data.readback,
        });
      return true;
    }
    if (data.type === "frame-cancel") {
      this.requests.delete(data.id as number);
      return true;
    }
    if (data.type === "resize") {
      validateViewport(data.width as number, data.height as number);
      this.requestedWidth = data.width as number;
      this.requestedHeight = data.height as number;
      if (!this.attached || this.device.isContextLost()) return true;
      const { width, height } = this.viewport;
      this.device.resize(width, height);
      if (this.runtime?.ipp_render_resize(width, height) !== 1)
        throw new Error("Rust renderer resize failed");
      this.logger.log("debug", "renderer.resized", () => ({
        session: this.session,
        width,
        height,
      }));
      return true;
    }
    return this.receiveTesting(data);
  }

  /**
   * Controls of `@ipp/client/testing`. Each requires the matching diagnostics
   * export; a build without it fails the connection with a clear error.
   */
  private receiveTesting(data: Record<string, unknown>): boolean {
    if (data.type === "glyph-atlas-limits") {
      const limits = {
        maxPages: data.maxPages as number,
        idlePagePublications: data.idlePagePublications as number,
      };
      validateGlyphAtlasLimits(limits);
      this.glyphAtlasLimits = limits;
      this.applyTestingOverrides();
      return true;
    }
    if (data.type === "surface-cache-budget") {
      const bytes = data.bytes as number;
      validateSurfaceCacheBudget(bytes);
      this.surfaceCacheBudget = bytes;
      this.applyTestingOverrides();
      return true;
    }
    if (data.type === "exhaustive-draw-checks") {
      if (typeof data.enabled !== "boolean")
        throw new Error("Invalid exhaustive draw check request");
      this.exhaustiveDrawChecks = data.enabled;
      this.applyTestingOverrides();
      return true;
    }
    if (data.type === "context-loss") {
      this.requireLossSimulation();
      // Exists only for the simulated loss: stop Host graphics loading before
      // the extension begins its asynchronous loss transition. Otherwise a
      // resource upload can report context loss as a permanent resource
      // failure while the device is being detached. A real loss is observed
      // through beforeFrame and the webglcontextlost event instead.
      this.suspend();
      this.device.loseContext();
      return true;
    }
    if (data.type === "context-restore") {
      this.requireLossSimulation();
      if (!this.device.isContextLost() && !this.lossObserved) return true;
      this.restoreRequested = true;
      this.restoreIfRequested();
      return true;
    }
    return false;
  }

  private requireLossSimulation(): void {
    if (!this.runtime || !this.reportsStatistics(this.runtime))
      throw new Error(
        "Context loss simulation requires a diagnostics render runtime",
      );
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.restoreTimer);
    this.canvas.removeEventListener("webglcontextlost", this.lost);
    this.canvas.removeEventListener("webglcontextrestored", this.restored);
    this.suspend();
    this.requests.clear();
    this.device.dispose();
    this.logger.log("info", "renderer.closed", () => ({
      session: this.session,
    }));
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
