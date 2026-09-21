import type { Client } from "@ipp/client";
import type { FrameCapture } from "@ipp/client";
import { MAX_CAPTURE_DIMENSION } from "@ipp/client";
import type { ComponentDescriptor, Inspection } from "@ipp/client";
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
  readonly frame: FrameCapture;
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
  const ratio = Math.min(
    window.devicePixelRatio,
    MAX_CAPTURE_DIMENSION / bounds.width,
    MAX_CAPTURE_DIMENSION / bounds.height,
  );
  const expected = {
    width: Math.max(1, Math.round(bounds.width * ratio)),
    height: Math.max(1, Math.round(bounds.height * ratio)),
  };
  const sizeDeadline = performance.now() + 10_000;
  let frame = await handle.capture();
  while (frame.width !== expected.width || frame.height !== expected.height) {
    if (performance.now() >= sizeDeadline) {
      throw new Error(
        `Timed out waiting for ${expected.width}x${expected.height} viewer capture; latest was ${frame.width}x${frame.height}`,
      );
    }
    await handle.client.waitForFrame(frame.tick);
    frame = await handle.capture();
  }
  if (frame.session !== observation.session) {
    throw new Error("Captured frame belongs to another runtime session");
  }
  if (frame.tick < observation.inspection.tick) {
    throw new Error("Captured frame predates the inspected scene state");
  }
  return { ...observation, frame };
}

function requireComponent(client: Client, name: string): ComponentDescriptor {
  const component = client.components[name];
  if (!component) throw new Error(`The runtime does not expose ${name}`);
  return component;
}
