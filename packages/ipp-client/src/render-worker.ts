import { SURFACE_CACHE_MODES, validateViewport } from "./presentation.js";
import type {
  IngressStatistics,
  RenderDeviceInfo,
  RenderStatisticsSnapshot,
  HostGuiLayoutStatistics,
  SurfaceCacheRecord,
  ViewportLimits,
} from "./presentation.js";
import { DiagnosticLogger, type LogLevel } from "./logging.js";
import type { RenderTesting } from "./render-testing.js";

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

/** Presentation and statistics exports of every render build. */
interface RenderHostExports {
  memory: WebAssembly.Memory;
  ipp_render_attach(width: number, height: number): number;
  ipp_render_detach(): number;
  ipp_render_max_viewport_width(): number;
  ipp_render_max_viewport_height(): number;
  ipp_render_statistics_ptr(): number;
  ipp_render_statistics_len(): number;
  ipp_render_gui_layout_ptr(): number;
  ipp_render_gui_layout_len(): number;
  ipp_render_surface_cache_records_ptr(): number;
  ipp_render_surface_cache_records_len(): number;
}

/** The worker-side testing module that instrumentation distributions ship. */
type RenderTestingModule = typeof import("./render-testing.js");

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
  guiLayoutReflows: 36,
  guiTextMeasurements: 37,
  totalGuiLayoutReflows: 38,
  totalGuiTextMeasurements: 39,
  guiHashes: 40,
  totalGuiHashes: 41,
} as const;

const RECORD_WORDS = 42;
/** Bit of `RECORD.flags`: the GUI layout words hold a Host sample. */
const RECORD_LAYOUT = 1;

/** Words per exported Surface cache record; see `ipp-wasm` `services/render.rs`. */
const SURFACE_CACHE_RECORD_WORDS = 12;

function pick<K extends keyof typeof RECORD>(
  words: Uint32Array,
  keys: readonly K[],
): Record<K, number> {
  return Object.fromEntries(
    keys.map((key) => [key, words[RECORD[key]]!]),
  ) as Record<K, number>;
}

export class RenderWorkerService {
  readonly imports: WebAssembly.Imports;
  private runtime: RenderHostExports | undefined;
  private ingress: IngressStatistics | undefined;
  /** Testing controls; present only in instrumentation distributions. */
  private readonly testing: RenderTesting | undefined;
  private session = 0n;
  private generation = 0;
  private attached = false;
  private closed = false;
  private lossObserved = false;
  private readbackMs = 0;
  private limits: ViewportLimits | undefined;

  private constructor(
    private readonly canvas: OffscreenCanvas,
    private readonly device: WebGlHostExports,
    private readonly post: (message: unknown, transfer: Transferable[]) => void,
    private readonly fail: (error: Error) => void,
    private readonly logger: DiagnosticLogger,
    testing: RenderTestingModule | undefined,
  ) {
    this.testing =
      testing &&
      new testing.RenderTesting({
        logger,
        session: () => this.session,
        lossObserved: () => this.lossObserved,
        isContextLost: () => device.isContextLost(),
        loseContext: () => device.loseContext(),
        restoreContext: () => device.restoreContext(),
        suspend: () => this.suspend(),
        fail,
      });
    // The build configuration comes first, so the client knows before any
    // call whether this presentation honours testing controls.
    post(
      {
        type: "presentation-configuration",
        instrumentation: this.testing !== undefined,
      },
      [],
    );
    this.imports = {
      ipp_gl: device.imports,
      ipp_presentation: {
        resize: (width: number, height: number): number => {
          try {
            validateViewport(width, height);
            if (device.isContextLost()) return 0;
            device.resize(width, height);
            return Number(canvas.width === width && canvas.height === height);
          } catch {
            return 0;
          }
        },
        capture: (pointer: number, length: number): number => {
          try {
            const runtime = this.runtime;
            if (
              !runtime ||
              device.isContextLost() ||
              length !== canvas.width * canvas.height * 4
            )
              return 0;
            const started = performance.now();
            const pixels = device.capture();
            this.readbackMs = performance.now() - started;
            if (pixels.byteLength !== length || device.isContextLost())
              return 0;
            new Uint8Array(runtime.memory.buffer, pointer >>> 0, length).set(
              pixels,
            );
            return 1;
          } catch {
            return 0;
          }
        },
      },
    };
    canvas.addEventListener("webglcontextlost", this.lost);
    canvas.addEventListener("webglcontextrestored", this.restored);
  }

  static async create(
    canvas: OffscreenCanvas,
    wasmUrl: string,
    post: (message: unknown, transfer: Transferable[]) => void,
    fail: (error: Error) => void,
    logLevel: LogLevel,
    testing: RenderTestingModule | undefined,
  ): Promise<RenderWorkerService> {
    validateViewport(canvas.width, canvas.height);
    // Only rendering distributions ship this binding; a headless worker has no GL dependency.
    const module = (await import(new URL("webgl.js", wasmUrl).href)) as {
      createWebGlDevice(canvas: OffscreenCanvas): WebGlHostExports;
    };
    return new RenderWorkerService(
      canvas,
      module.createWebGlDevice(canvas),
      post,
      fail,
      new DiagnosticLogger("renderer", logLevel),
      testing,
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
      "ipp_render_detach",
      "ipp_render_max_viewport_width",
      "ipp_render_max_viewport_height",
      "ipp_render_statistics_ptr",
      "ipp_render_statistics_len",
      "ipp_render_gui_layout_ptr",
      "ipp_render_gui_layout_len",
      "ipp_render_surface_cache_records_ptr",
      "ipp_render_surface_cache_records_len",
    ]) {
      if (typeof candidate[name] !== "function")
        throw new Error(
          `WASM runtime is missing ${name}; select the render build`,
        );
    }
    this.runtime = exports as RenderHostExports;
    this.ingress = ingress;
    this.session = session;
    this.device.setMemory(this.runtime.memory);
    this.testing?.initialize(exports);
    if (!this.device.isContextLost()) this.attach();
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

  private attach(): void {
    if (!this.runtime || this.closed) return;
    this.refreshLimits();
    const { width, height } = this.canvas;
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
    this.testing?.lost();
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
    if (this.attached && this.runtime?.ipp_render_detach() !== 1)
      throw new Error("Rust renderer detach failed");
    this.attached = false;
  }

  beforeFrame(): void {
    // Loss may become observable before the browser dispatches its event.
    if (this.device.isContextLost()) this.suspend();
  }

  /** Read the packed statistics record on request. */
  private statistics(
    runtime: RenderHostExports,
    readbackMs: number,
  ): RenderStatisticsSnapshot {
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
    const layoutPointer = runtime.ipp_render_gui_layout_ptr() >>> 0;
    const layoutLength = runtime.ipp_render_gui_layout_len() >>> 0;
    if (layoutPointer === 0 || layoutLength === 0)
      throw new Error("GUI layout diagnostics unavailable");
    const guiLayout: HostGuiLayoutStatistics | null = JSON.parse(
      new TextDecoder("utf-8", { fatal: true }).decode(
        new Uint8Array(runtime.memory.buffer, layoutPointer, layoutLength),
      ),
    );
    if (flags & RECORD_LAYOUT && !guiLayout)
      throw new Error("GUI layout counters have no membership evidence");
    return {
      readbackMs,
      guiLayout,
      frame: pick(words, [
        "uploadedBytes",
        "totalUploadedBytes",
        "unshadowedLights",
      ]),
      shadows: pick(words, ["shadowDrawCalls", "shadowResidentBytes"]),
      gui: {
        ...pick(words, [
          "guiBatches",
          "guiRebuilds",
          "guiHashes",
          "guiAllocations",
          "guiResidentBytes",
          "glyphMisses",
          "glyphPopulates",
          "glyphPopulationFailures",
          "glyphPageRetirements",
          "glyphPages",
          "glyphResidentBytes",
          "totalGuiRebuilds",
          "totalGuiHashes",
          "totalGuiAllocations",
          "totalGlyphMisses",
          "totalGlyphPopulates",
          "totalGlyphPopulationFailures",
          "totalGlyphPageRetirements",
        ]),
        ...(flags & RECORD_LAYOUT
          ? pick(words, [
              "guiLayoutReflows",
              "guiTextMeasurements",
              "totalGuiLayoutReflows",
              "totalGuiTextMeasurements",
            ])
          : {}),
      },
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
      ...(this.ingress ? { ingress: { ...this.ingress } } : {}),
      device: this.device.info() as RenderDeviceInfo,
    };
  }

  /** Records the runtime builds on demand for the last completed frame. */
  private surfaceCacheRecords(
    runtime: RenderHostExports,
  ): SurfaceCacheRecord[] {
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
        capacityWidth: words[at + 10]!,
        capacityHeight: words[at + 11]!,
      });
    }
    return records;
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "render-statistics") {
      const runtime = this.runtime;
      if (!runtime)
        throw new Error("Render statistics before the runtime loaded");
      this.post(
        {
          type: "render-statistics",
          id: data.id,
          statistics: this.statistics(runtime, this.readbackMs),
        },
        [],
      );
      return true;
    }
    return this.testing?.receive(data) ?? false;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.testing?.close();
    this.canvas.removeEventListener("webglcontextlost", this.lost);
    this.canvas.removeEventListener("webglcontextrestored", this.restored);
    this.suspend();
    this.device.dispose();
    this.logger.log("info", "renderer.closed", () => ({
      session: this.session,
    }));
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
