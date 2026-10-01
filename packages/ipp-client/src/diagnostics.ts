/**
 * Read-only diagnostic observations that every build answers: renderer
 * statistics of a presenting Host and the lifecycle publisher's counters of a
 * World session. They are separate from outcomes and events, never a readiness
 * or capture fence, and they change nothing in the runtime. Controls that do
 * change runtime behaviour for internal tests live in `@ipp/client/testing`.
 */
import { presentationOf, type RenderDiagnostics } from "./presentation.js";

export { lifecycleDiagnostics } from "./lifecycle-diagnostics.js";
export type {
  LifecycleDiagnosticSample,
  LifecycleDiagnostics,
} from "./lifecycle-diagnostics.js";
export type {
  FrameRenderStatistics,
  GuiLayoutWorkStatistics,
  GuiRenderStatistics,
  HostGuiLayoutStatistics,
  IngressStatistics,
  RenderDeviceInfo,
  RenderDiagnostics,
  RenderStatisticsSnapshot,
  ShadowRenderStatistics,
  SurfaceCacheMode,
  SurfaceCacheRecord,
  SurfaceRenderStatistics,
} from "./presentation.js";

/**
 * Renderer statistics of the presentation behind `target`, a Host client or its
 * transport; undefined when the connection presents nothing, such as a
 * headless worker or a plain WebSocket.
 */
export function renderDiagnostics(
  target: object,
): RenderDiagnostics | undefined {
  return presentationOf(target);
}
