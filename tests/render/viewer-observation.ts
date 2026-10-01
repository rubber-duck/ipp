import type { RenderStatisticsSnapshot } from "@ipp/client/diagnostics";
import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import type {
  Client,
  ComponentDescriptor,
  Inspection,
  PresentedCapture,
} from "@ipp/client";
import type { IppCanvasHandle } from "@ipp/react/web";

export { VIEWER_ENTITY_ID } from "../../examples/world-gallery/worlds/geometry/world.js";
export { VIEWER_TEXTURE_SOURCE } from "../../examples/world-gallery/worlds/geometry/world.js";
export {
  VIEWER_MESH_SOURCES,
  type IsolatedShape,
  type ViewerFinish,
  type ViewerShape,
} from "../../examples/world-gallery/shared/geometry-catalog.js";
export interface ViewerObservation {
  readonly session: bigint;
  readonly inspection: Inspection;
  readonly componentIds: {
    readonly Transform: number;
    readonly UnlitMaterial: number;
    readonly UnlitTexture: number;
    readonly MeshInstance: number;
    readonly BoundingGeometry: number;
  };
}

export interface ViewerCapture extends ViewerObservation {
  readonly frame: PresentedCapture;
  /** Evaluated tick of the selected output in the captured draw. */
  readonly tick: bigint;
  /** Renderer statistics observed after the captured draw completed. */
  readonly statistics: RenderStatisticsSnapshot;
  /** Host draws presented after the captured draw before its pixels arrived. */
  readonly completionFrames: number;
}

/**
 * Host draws a capture may take to arrive after the draw it returns. The Host
 * answers chunk reads between frames; a worker that never idles, as on a
 * software renderer, answers one window of reads per frame. The client reads
 * windows of 16 chunks of 64 KiB (`host-presentation.ts`). Two draws per window
 * and a few for completion allow a page thread slowed by machine load while
 * the worker keeps drawing. Reading one chunk per Host frame took 150-230
 * draws for a 1714x1259 capture of 132 chunks.
 */
function maxCompletionFrames(bytes: number): number {
  return 2 * Math.ceil(bytes / (16 * 65_536)) + 6;
}

export async function observeViewer(
  handle: IppCanvasHandle,
): Promise<ViewerObservation> {
  await handle.flush();
  const { client } = handle;
  let inspection = await client.inspect();
  const deadline = performance.now() + 10_000;
  while (
    inspection.entities.some((entity) =>
      entity.metadata.symbolicId?.endsWith("-pending"),
    )
  ) {
    const failed = inspection.resources.find(
      (resource) => resource.status === "failed",
    );
    if (failed) throw new Error(`Resource ${failed.source}: ${failed.error}`);
    if (performance.now() >= deadline)
      throw new Error("Timed out waiting for prepared geometry replacement");
    await client.waitForFrame(inspection.tick);
    await handle.flush();
    inspection = await client.inspect();
  }
  return {
    session: client.session,
    inspection,
    componentIds: {
      Transform: requireComponent(client, "Transform").id,
      UnlitMaterial: requireComponent(client, "UnlitMaterial").id,
      UnlitTexture: requireComponent(client, "UnlitTexture").id,
      MeshInstance: requireComponent(client, "MeshInstance").id,
      BoundingGeometry: requireComponent(client, "BoundingGeometry").id,
    },
  };
}

export async function captureViewer(
  handle: IppCanvasHandle,
  options: { waitForResources?: boolean } = {},
): Promise<ViewerCapture> {
  let observation = await observeViewer(handle);
  const deadline = performance.now() + 10_000;
  while (options.waitForResources !== false) {
    const failure = observation.inspection.resources.find(
      (resource) => resource.status === "failed",
    );
    if (failure)
      throw new Error(`Resource ${failure.source}: ${failure.error}`);
    if (
      observation.inspection.resources.every(
        (resource) => resource.status === "loaded",
      )
    )
      break;
    if (performance.now() >= deadline)
      throw new Error("Timed out observing resource readiness");
    await handle.client.waitForFrame(observation.inspection.tick);
    observation = await observeViewer(handle);
  }
  const diagnostic = observation.inspection.renderDiagnostics[0];
  if (diagnostic)
    throw new Error(`Entity ${diagnostic.entity}: ${diagnostic.reason}`);
  const canvas = document.querySelector<HTMLCanvasElement>("#ipp-world-canvas");
  if (!canvas) throw new Error("Viewer canvas is missing from the DOM");
  const bounds = canvas.getBoundingClientRect();
  // The selected viewport follows CSS size × density negotiated with the surface limits.
  const surface = await handle.host.presentation.surface();
  const ratio = Math.min(
    window.devicePixelRatio,
    surface.maxWidth / bounds.width,
    surface.maxHeight / bounds.height,
  );
  const expected = {
    width: Math.max(1, Math.round(bounds.width * ratio)),
    height: Math.max(1, Math.round(bounds.height * ratio)),
  };
  const sizeDeadline = performance.now() + 10_000;
  let frame = await captureSelected(handle);
  let viewport = frame.view.binding.viewport;
  while (
    viewport.width !== expected.width ||
    viewport.height !== expected.height
  ) {
    if (performance.now() >= sizeDeadline) {
      throw new Error(
        `Timed out waiting for ${expected.width}x${expected.height} viewer capture; latest was ${viewport.width}x${viewport.height}`,
      );
    }
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    frame = await captureSelected(handle);
    viewport = frame.view.binding.viewport;
  }
  // The next completed draw after the pixels arrived bounds how many draws
  // the Host presented while the capture completed and transferred.
  const following = await handle.host.presentation.frame(frame.view, {
    afterSequence: frame.sequence,
  });
  const completionFrames = Number(following.sequence - frame.sequence) - 1;
  const allowedFrames = maxCompletionFrames(frame.pixels.byteLength);
  if (completionFrames > allowedFrames) {
    throw new Error(
      `Capture of draw ${frame.sequence} arrived after ${completionFrames} further draws; at most ${allowedFrames} are expected`,
    );
  }
  const statistics = await requireDiagnostics(handle).statistics();
  if (frame.view.binding.output.world.id !== handle.client.worldReference?.id) {
    throw new Error("Captured frame belongs to another runtime World");
  }
  const [source] = frame.sources;
  if (!source || source.tick < observation.inspection.tick) {
    throw new Error("Captured frame predates the inspected scene state");
  }
  return {
    ...observation,
    frame,
    tick: source.tick,
    statistics,
    completionFrames,
  };
}

/** A completed draw including the selected output's content admitted before the request. */
function captureSelected(handle: IppCanvasHandle): Promise<PresentedCapture> {
  const view = handle.view;
  if (!view) throw new Error("Viewer has no selected presentation view");
  return handle.capture({ afterOutputs: [view.binding.output] });
}

function requireDiagnostics(handle: IppCanvasHandle) {
  const diagnostics = renderDiagnostics(handle.host);
  if (!diagnostics)
    throw new Error("Viewer render diagnostics are unavailable");
  return diagnostics;
}

function requireComponent(client: Client, name: string): ComponentDescriptor {
  const component = client.components[name];
  if (!component) throw new Error(`The runtime does not expose ${name}`);
  return component;
}
