/**
 * Fault injection, renderer budget overrides and native GLES presentation for
 * integration tests.
 *
 * Production presentation has no such controls: the renderer owns its GPU
 * budgets and real context loss comes from the browser. These helpers send
 * worker control messages that only `diagnostics` runtime builds honour; any
 * other build fails the connection with an explanatory error. The native GLES
 * testing host of `ipp-server` presents through `nativePresentationTransport`
 * and honours the same controls. Import them from
 * `@ipp/client/testing`, never from application code.
 */
import {
  testingChannel,
  validateGlyphAtlasLimits,
  validateSurfaceCacheBudget,
  type RenderDiagnostics,
  type GlyphAtlasLimits,
} from "./presentation.js";

export { lifecycleTesting } from "./lifecycle-diagnostics.js";
export type {
  LifecycleDiagnosticSample,
  LifecycleTesting,
} from "./lifecycle-diagnostics.js";

export type { GlyphAtlasLimits } from "./presentation.js";
export { nativePresentationTransport } from "./native-presentation.js";

export interface PresentationTesting {
  /** Lose the worker's WebGL context through `WEBGL_lose_context`. */
  loseContext(): void;
  /** Restore a context lost through `loseContext` once the loss is observed. */
  restoreContext(): void;
  /** Override the glyph atlas budget; requires a GUI render build. */
  setGlyphAtlasLimits(limits: GlyphAtlasLimits): void;
  /**
   * Override resident whole-Surface cache image bytes; zero disables caching.
   * Requires a render build with Surfaces.
   */
  setSurfaceCacheBudget(bytes: number): void;
  /** Attribute each GL error to its failing call instead of sampled frame ends. */
  setExhaustiveDrawChecks(enabled: boolean): void;
}

/**
 * Testing controls of one worker presentation. Overrides persist in the worker
 * for later World sessions and through context loss.
 */
export function presentationTesting(
  presentation: RenderDiagnostics,
): PresentationTesting {
  const send = testingChannel(presentation);
  return {
    loseContext: () => send({ type: "context-loss" }),
    restoreContext: () => send({ type: "context-restore" }),
    setGlyphAtlasLimits: (limits) => {
      validateGlyphAtlasLimits(limits);
      send({
        type: "glyph-atlas-limits",
        maxPages: limits.maxPages,
        idlePageFrames: limits.idlePageFrames,
      });
    },
    setSurfaceCacheBudget: (bytes) => {
      validateSurfaceCacheBudget(bytes);
      send({ type: "surface-cache-budget", bytes });
    },
    setExhaustiveDrawChecks: (enabled) =>
      send({ type: "exhaustive-draw-checks", enabled }),
  };
}
