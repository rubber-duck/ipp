/**
 * Worker side of the `@ipp/client/testing` presentation controls: renderer
 * budget overrides, exhaustive draw checks and simulated context loss. Only
 * `instrumentation` distributions ship this module, and their worker hands it
 * to the render service by build configuration.
 */
import {
  validateGlyphAtlasLimits,
  validateSurfaceCacheBudget,
  type GlyphAtlasLimits,
} from "./presentation.js";
import type { DiagnosticLogger } from "./logging.js";

/** Testing exports of an `instrumentation` render build. */
interface RenderTestingExports {
  ipp_render_set_glyph_atlas_limits(
    maxPages: number,
    idlePageFrames: number,
  ): number;
  ipp_render_set_surface_cache_budget(bytes: number): number;
  ipp_render_set_exhaustive_draw_checks(enabled: number): number;
}

/** What the controls need from the render service that owns them. */
export interface RenderTestingTarget {
  readonly logger: DiagnosticLogger;
  session(): bigint;
  /** Whether the service has observed the current context loss. */
  lossObserved(): boolean;
  isContextLost(): boolean;
  loseContext(): void;
  restoreContext(): void;
  /** Detach the renderer from its context. */
  suspend(): void;
  fail(error: Error): void;
}

export class RenderTesting {
  private runtime: RenderTestingExports | undefined;
  /** Latest atlas bounds, applied again for a later World session. */
  private glyphAtlasLimits: GlyphAtlasLimits | undefined;
  /** Latest cache image budget, applied again for a later World session. */
  private surfaceCacheBudget: number | undefined;
  private exhaustiveDrawChecks: boolean | undefined;
  private restoreRequested = false;
  private restoreTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly target: RenderTestingTarget) {}

  /** Bind a session's runtime; the renderer keeps overrides through context loss. */
  initialize(exports: object): void {
    const candidate = exports as Record<string, unknown>;
    for (const name of [
      "ipp_render_set_glyph_atlas_limits",
      "ipp_render_set_surface_cache_budget",
      "ipp_render_set_exhaustive_draw_checks",
    ])
      if (typeof candidate[name] !== "function")
        throw new Error(
          `This instrumentation distribution's runtime is missing ${name}; build it with the instrumentation feature`,
        );
    this.runtime = exports as RenderTestingExports;
    this.apply();
  }

  private apply(): void {
    const runtime = this.runtime;
    if (!runtime) return;
    const limits = this.glyphAtlasLimits;
    if (limits) {
      if (
        runtime.ipp_render_set_glyph_atlas_limits(
          limits.maxPages,
          limits.idlePageFrames,
        ) !== 1
      )
        throw new Error("Rust renderer rejected the glyph atlas limits");
      this.target.logger.log("debug", "renderer.glyph_atlas_limits", () => ({
        session: this.target.session(),
        maxPages: limits.maxPages,
        idlePageFrames: limits.idlePageFrames,
      }));
    }
    const bytes = this.surfaceCacheBudget;
    if (bytes !== undefined) {
      if (runtime.ipp_render_set_surface_cache_budget(bytes >>> 0) !== 1)
        throw new Error("Rust renderer rejected the Surface cache budget");
      this.target.logger.log("debug", "renderer.surface_cache_budget", () => ({
        session: this.target.session(),
        bytes,
      }));
    }
    const enabled = this.exhaustiveDrawChecks;
    if (
      enabled !== undefined &&
      runtime.ipp_render_set_exhaustive_draw_checks(Number(enabled)) !== 1
    )
      throw new Error("Rust renderer rejected exhaustive draw checks");
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "glyph-atlas-limits") {
      const limits = {
        maxPages: data.maxPages as number,
        idlePageFrames: data.idlePageFrames as number,
      };
      validateGlyphAtlasLimits(limits);
      this.glyphAtlasLimits = limits;
      this.apply();
      return true;
    }
    if (data.type === "surface-cache-budget") {
      const bytes = data.bytes as number;
      validateSurfaceCacheBudget(bytes);
      this.surfaceCacheBudget = bytes;
      this.apply();
      return true;
    }
    if (data.type === "exhaustive-draw-checks") {
      if (typeof data.enabled !== "boolean")
        throw new Error("Invalid exhaustive draw check request");
      this.exhaustiveDrawChecks = data.enabled;
      this.apply();
      return true;
    }
    if (data.type === "context-loss") {
      // Exists only for the simulated loss: stop Host graphics loading before
      // the extension begins its asynchronous loss transition. Otherwise a
      // resource upload can report context loss as a permanent resource
      // failure while the device is being detached. A real loss is observed
      // through beforeFrame and the webglcontextlost event instead.
      this.target.suspend();
      this.target.loseContext();
      return true;
    }
    if (data.type === "context-restore") {
      if (!this.target.isContextLost() && !this.target.lossObserved())
        return true;
      this.restoreRequested = true;
      this.lost();
      return true;
    }
    return false;
  }

  /** Restore a simulated loss once the service has observed it. */
  lost(): void {
    if (
      !this.restoreRequested ||
      !this.target.lossObserved() ||
      this.restoreTimer !== undefined
    )
      return;
    // WEBGL_lose_context permits restoration after the cancelled loss event has
    // finished dispatching. The caller need not guess that event's timing.
    this.restoreTimer = setTimeout(() => {
      this.restoreTimer = undefined;
      this.restoreRequested = false;
      try {
        this.target.restoreContext();
      } catch (error) {
        this.target.fail(
          error instanceof Error ? error : new Error(String(error)),
        );
      }
    }, 0);
  }

  close(): void {
    clearTimeout(this.restoreTimer);
  }
}
