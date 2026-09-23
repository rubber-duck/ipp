import { validateGlyphAtlasLimits, validateViewport } from "./presentation.js";
import type { FrameCapture, GlyphAtlasLimits } from "./presentation.js";
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
  ipp_render_uploaded_bytes(): number;
  ipp_render_failed_draw_calls(): number;
  ipp_render_invalid_camera(): number;
  ipp_render_unshadowed_lights(): number;
}

/** Retained GUI counters exported together by GUI-capable render builds. */
const RETAINED_GUI_STATISTICS = {
  guiBatches: "ipp_render_gui_batches",
  guiRebuilds: "ipp_render_gui_rebuilds",
  guiAllocations: "ipp_render_gui_allocations",
  guiResidentBytes: "ipp_render_gui_resident_bytes",
  glyphMisses: "ipp_render_glyph_misses",
  glyphPopulates: "ipp_render_glyph_populates",
  glyphPopulationFailures: "ipp_render_glyph_population_failures",
  glyphPageRetirements: "ipp_render_glyph_page_retirements",
  glyphPages: "ipp_render_glyph_pages",
  glyphResidentBytes: "ipp_render_glyph_resident_bytes",
} as const;

type RetainedGuiStatistics = Record<
  keyof typeof RETAINED_GUI_STATISTICS,
  () => number
>;

/** Per-submission counters also accumulated across every rendered tick, so a
 * capture observes work done by frames it did not capture. */
const ACCUMULATED_GUI_STATISTICS = {
  guiRebuilds: "totalGuiRebuilds",
  guiAllocations: "totalGuiAllocations",
  glyphMisses: "totalGlyphMisses",
  glyphPopulates: "totalGlyphPopulates",
  glyphPopulationFailures: "totalGlyphPopulationFailures",
  glyphPageRetirements: "totalGlyphPageRetirements",
} as const;

interface CaptureRequest {
  id: number;
  session: bigint;
  afterTick: bigint;
}

export class RenderWorkerService {
  readonly imports: WebAssembly.Imports;
  private runtime: RenderHostExports | undefined;
  /** Absent for builds without GUI rendering: those counters are unavailable, not zero. */
  private retainedStatistics: RetainedGuiStatistics | undefined;
  /** Present exactly when the runtime renders GUI content through a glyph atlas. */
  private glyphAtlasLimitsExport:
    | ((maxPages: number, idlePagePublications: number) => number)
    | undefined;
  /** Latest requested atlas bounds, applied again for a later World session. */
  private glyphAtlasLimits: GlyphAtlasLimits | undefined;
  private readonly retainedTotals: Record<string, number> = {};
  private session = 0n;
  private generation = 0;
  private totalUploadedBytes = 0;
  private observedRenderTick = 0n;
  private attached = false;
  private closed = false;
  private lossObserved = false;
  private restoreRequested = false;
  private restoreTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly captures = new Map<number, CaptureRequest>();
  private width: number;
  private height: number;

  private constructor(
    private readonly canvas: OffscreenCanvas,
    private readonly device: WebGlHostExports,
    private readonly post: (message: unknown, transfer: Transferable[]) => void,
    private readonly fail: (error: Error) => void,
    private readonly ingress: Record<string, number>,
    private readonly logger: DiagnosticLogger,
  ) {
    this.width = canvas.width;
    this.height = canvas.height;
    this.imports = { ipp_gl: device.imports };
    canvas.addEventListener("webglcontextlost", this.lost);
    canvas.addEventListener("webglcontextrestored", this.restored);
  }

  static async create(
    canvas: OffscreenCanvas,
    wasmUrl: string,
    post: (message: unknown, transfer: Transferable[]) => void,
    fail: (error: Error) => void,
    ingress: Record<string, number>,
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
      ingress,
      new DiagnosticLogger("renderer", logLevel),
    );
  }

  initialize(exports: object, session: bigint): void {
    const candidate = exports as Record<string, unknown>;
    for (const name of [
      "ipp_render_attach",
      "ipp_render_resize",
      "ipp_render_detach",
      "ipp_render_tick",
      "ipp_render_draw_calls",
      "ipp_render_triangles",
      "ipp_render_uploaded_bytes",
      "ipp_render_failed_draw_calls",
      "ipp_render_invalid_camera",
      "ipp_render_unshadowed_lights",
    ]) {
      if (typeof candidate[name] !== "function")
        throw new Error(
          `WASM runtime is missing ${name}; select the render build`,
        );
    }
    // A GUI render build exports every retained counter; a partial set is a
    // build error rather than a report of zero work.
    const statistics = Object.entries(RETAINED_GUI_STATISTICS);
    const present = statistics.filter(
      ([, name]) => typeof candidate[name] === "function",
    );
    if (present.length !== 0 && present.length !== statistics.length)
      throw new Error(
        `WASM runtime exports an incomplete retained GUI statistics set; missing ${statistics
          .filter(([, name]) => typeof candidate[name] !== "function")
          .map(([, name]) => name)
          .join(", ")}`,
      );
    this.retainedStatistics =
      present.length === 0
        ? undefined
        : (Object.fromEntries(
            statistics.map(([key, name]) => [
              key,
              candidate[name] as () => number,
            ]),
          ) as RetainedGuiStatistics);
    const limits = candidate.ipp_render_set_glyph_atlas_limits;
    if ((typeof limits === "function") !== (present.length !== 0))
      throw new Error(
        "WASM runtime glyph atlas limits differ from its retained GUI statistics",
      );
    this.glyphAtlasLimitsExport =
      typeof limits === "function"
        ? (limits as (maxPages: number, idlePagePublications: number) => number)
        : undefined;
    this.runtime = exports as RenderHostExports;
    this.session = session;
    this.observedRenderTick = 0n;
    this.device.setMemory(this.runtime.memory);
    this.applyGlyphAtlasLimits();
    if (!this.device.isContextLost()) this.attach();
  }

  /** The renderer keeps limits through context loss; a later session receives them again. */
  private applyGlyphAtlasLimits(): void {
    const limits = this.glyphAtlasLimits;
    if (!limits || !this.runtime) return;
    if (!this.glyphAtlasLimitsExport)
      throw new Error(
        "WASM runtime has no glyph atlas; select a GUI render build",
      );
    if (
      this.glyphAtlasLimitsExport(
        limits.maxPages,
        limits.idlePagePublications,
      ) !== 1
    )
      throw new Error("Rust renderer rejected the glyph atlas limits");
    this.logger.log("debug", "renderer.glyph_atlas_limits", () => ({
      session: this.session,
      maxPages: limits.maxPages,
      idlePagePublications: limits.idlePagePublications,
    }));
  }

  private attach(): void {
    if (!this.runtime || this.closed) return;
    // Layout can change while the context is unavailable. Apply the latest
    // surface intent before rebuilding graphics in the existing world session.
    this.device.resize(this.width, this.height);
    if (this.runtime.ipp_render_attach(this.width, this.height) !== 1)
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

  afterFrame(): void {
    const runtime = this.runtime;
    if (!runtime || !this.attached || this.device.isContextLost()) return;
    const tick = runtime.ipp_render_tick();
    if (tick === 0n) return;
    if (tick !== this.observedRenderTick) {
      this.totalUploadedBytes += runtime.ipp_render_uploaded_bytes();
      if (this.retainedStatistics)
        for (const [key, total] of Object.entries(ACCUMULATED_GUI_STATISTICS))
          this.retainedTotals[total] =
            (this.retainedTotals[total] ?? 0) +
            this.retainedStatistics[
              key as keyof typeof ACCUMULATED_GUI_STATISTICS
            ]();
      this.observedRenderTick = tick;
    }
    for (const [id, request] of this.captures) {
      if (tick < request.afterTick) continue;
      // capture finishes pending GPU work and copies top-left RGBA pixels.
      const pixels = this.device.capture();
      const frame: FrameCapture = {
        session: this.session,
        tick,
        width: this.canvas.width,
        height: this.canvas.height,
        pixels: pixels.buffer,
        drawCalls: runtime.ipp_render_draw_calls(),
        triangles: runtime.ipp_render_triangles(),
        contextGeneration: this.generation,
        backend: {
          ...this.device.info(),
          uploadedBytes: runtime.ipp_render_uploaded_bytes(),
          totalUploadedBytes: this.totalUploadedBytes,
          failedDrawCalls: runtime.ipp_render_failed_draw_calls(),
          // Unavailable counters are omitted, never reported as zero work.
          ...(this.retainedStatistics
            ? {
                ...Object.fromEntries(
                  Object.entries(this.retainedStatistics).map(([key, read]) => [
                    key,
                    read(),
                  ]),
                ),
                ...Object.fromEntries(
                  Object.values(ACCUMULATED_GUI_STATISTICS).map((total) => [
                    total,
                    this.retainedTotals[total] ?? 0,
                  ]),
                ),
              }
            : {}),
          invalidCamera: runtime.ipp_render_invalid_camera() !== 0,
          unshadowedLights: runtime.ipp_render_unshadowed_lights(),
          ingress: { ...this.ingress },
        },
      };
      this.captures.delete(id);
      this.post({ type: "capture-result", id, frame }, [pixels.buffer]);
    }
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "capture") {
      if (
        !Number.isSafeInteger(data.id) ||
        (data.id as number) <= 0 ||
        typeof data.session !== "bigint" ||
        typeof data.afterTick !== "bigint" ||
        data.afterTick < 0n ||
        data.afterTick > 0xffff_ffff_ffff_ffffn
      )
        throw new Error("Invalid capture request");
      const id = data.id as number;
      if (
        data.session !== this.session ||
        this.captures.size >= 4 ||
        this.captures.has(id)
      ) {
        this.post(
          {
            type: "capture-error",
            id,
            message: "Capture session mismatch or queue full",
          },
          [],
        );
      } else
        this.captures.set(id, {
          id,
          session: data.session,
          afterTick: data.afterTick,
        });
      return true;
    }
    if (data.type === "capture-cancel") {
      this.captures.delete(data.id as number);
      return true;
    }
    if (data.type === "resize") {
      validateViewport(data.width as number, data.height as number);
      this.width = data.width as number;
      this.height = data.height as number;
      if (!this.attached || this.device.isContextLost()) return true;
      this.device.resize(this.width, this.height);
      if (this.runtime?.ipp_render_resize(this.width, this.height) !== 1)
        throw new Error("Rust renderer resize failed");
      this.logger.log("debug", "renderer.resized", () => ({
        session: this.session,
        width: data.width as number,
        height: data.height as number,
      }));
      return true;
    }
    if (data.type === "glyph-atlas-limits") {
      const limits = {
        maxPages: data.maxPages as number,
        idlePagePublications: data.idlePagePublications as number,
      };
      validateGlyphAtlasLimits(limits);
      this.glyphAtlasLimits = limits;
      this.applyGlyphAtlasLimits();
      return true;
    }
    if (data.type === "context-loss") {
      // Stop Host graphics loading before the extension begins the asynchronous
      // loss transition. Otherwise a resource upload can report context loss as
      // a permanent resource failure while the device is being detached.
      this.suspend();
      this.device.loseContext();
      return true;
    }
    if (data.type === "context-restore") {
      if (!this.device.isContextLost() && !this.lossObserved) return true;
      this.restoreRequested = true;
      this.restoreIfRequested();
      return true;
    }
    return false;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.restoreTimer);
    this.canvas.removeEventListener("webglcontextlost", this.lost);
    this.canvas.removeEventListener("webglcontextrestored", this.restored);
    this.suspend();
    this.captures.clear();
    this.device.dispose();
    this.logger.log("info", "renderer.closed", () => ({
      session: this.session,
    }));
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
