/**
 * Fault injection, renderer budget overrides and native GLES presentation for
 * integration tests.
 *
 * Production presentation has no such controls: the renderer owns its GPU
 * budgets and real context loss comes from the browser. These helpers send
 * worker control messages that only `instrumentation` builds honour. Against
 * any other build, each call throws before sending anything. The native GLES
 * testing host of `ipp-server` presents through `nativePresentationTransport`
 * and honours the same controls in its `instrumentation` build. Import them
 * from `@ipp/client/testing`, never from application code; read-only
 * statistics live in `@ipp/client/diagnostics`.
 */
import {
  presentationOf,
  validateGlyphAtlasLimits,
  validateSurfaceCacheBudget,
  type GlyphAtlasLimits,
  type PresentationTestingMessage,
} from "./presentation.js";

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
 * Testing controls of the presentation linked to `target`: a Host client, its
 * transport, or the object `renderDiagnostics` returned for either. Overrides
 * persist in the worker for later World sessions and through context loss.
 */
export function presentationTesting(target: object): PresentationTesting {
  const port = presentationOf(target);
  if (!port) throw new TypeError("Expected an IPP worker presentation");
  const send = (message: PresentationTestingMessage) => {
    if (port.instrumentation === undefined)
      throw new Error(
        "Presentation testing controls are unavailable until the presentation reports its build configuration",
      );
    if (!port.instrumentation)
      throw new Error(
        "Presentation testing controls require an instrumentation build; this presentation runs a build without instrumentation",
      );
    port.post(message);
  };
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

export {
  hostProfiling,
  ProfileControlError,
  summarizeProfileCompositions,
} from "./profiling.js";
export type {
  HostProfiling,
  HostProfileStatus,
  ProfileCapture,
  ProfileTrace,
  ProfileSpan,
  ProfileClockCorrelation,
  ProfileIdentity,
  ProfilePhase,
  ProfileStage,
  ProfileCategory,
  ProfileAsyncIdentity,
  ProfileCompositionSummary,
  ProfileGpuCapture,
  ProfileGpuRecord,
} from "./profiling.js";
