import { StrictMode } from "react";
import { createRoot, type Root as ReactDomRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import type {
  AssetWorldClient,
  FrameCapture,
  AssetResourceSnapshot,
} from "@ipp/client";
import {
  Entity,
  MeshInstance,
  Transform,
  UnlitMaterial,
  UnlitTexture,
} from "@ipp/react";
import {
  IppCanvas,
  World,
  type CanvasRuntimeConfiguration,
  type IppCanvasHandle,
} from "@ipp/react/web";
import { ReadyGeometry } from "../../examples/world-gallery/shared/ready-geometry.js";
import { activateFixtureCamera } from "../integration/camera-fixtures.js";
import { compareImages, summarizeImage } from "./image-assertions.js";

let root: ReactDomRoot;
let handle: IppCanvasHandle;
let configuration: CanvasRuntimeConfiguration;
let strict: boolean;
let baseline: FrameCapture;
let source = "ipp://mesh/cube?width=2&height=2&length=2";
let texture: string | undefined;
let mounted = true;
const events: AssetResourceSnapshot[] = [];
const errors: string[] = [];

export async function initialize(
  input: CanvasRuntimeConfiguration,
  strictMode: boolean,
) {
  configuration = input;
  strict = strictMode;
  const element = document.createElement("div");
  document.body.append(element);
  root = createRoot(element);
  let ready!: () => void;
  let reject!: (error: unknown) => void;
  const connected = new Promise<void>((resolve, fail) => {
    ready = resolve;
    reject = fail;
  });
  onReady = (canvas) => {
    handle = canvas;
    (canvas.client as AssetWorldClient).onResourceChange((event) =>
      events.push(event),
    );
    void activateFixtureCamera(canvas.client).then(ready, reject);
  };
  render();
  await connected;
  await waitForSource(source);
  baseline = await handle.capture();
}

let onReady: (canvas: IppCanvasHandle) => void;

function render() {
  const app = (
    <IppCanvas
      runtime={configuration}
      width={320}
      height={240}
      onReady={onReady}
      onError={(error) => errors.push(error.message)}
    >
      {mounted && (
        <World>
          <Entity id="replacement-shared">
            <MeshInstance source="ipp://mesh/cube?width=2&height=2&length=2" />
          </Entity>
          <ReadyGeometry
            id="replacement-pending"
            mesh={source}
            texture={texture}
          >
            {({ mesh, texture }) => (
              <Entity id="replacement-visible">
                <Transform />
                <UnlitMaterial />
                <MeshInstance source={mesh} />
                {texture !== undefined && <UnlitTexture source={texture} />}
              </Entity>
            )}
          </ReadyGeometry>
        </World>
      )}
    </IppCanvas>
  );
  flushSync(() => root.render(strict ? <StrictMode>{app}</StrictMode> : app));
}

export async function edit(mesh: string, nextTexture?: string) {
  source = mesh;
  texture = nextTexture;
  render();
  await handle.flush();
}

export async function inspect() {
  await handle.flush();
  const inspection = await handle.client.inspect();
  const entity = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === "replacement-visible",
  );
  const fields = (name: string) =>
    entity?.effective.find(
      (entry) => entry.component === handle.client.components[name]!.id,
    )?.fields;
  return {
    inspection,
    mesh: fields("MeshInstance")?.source,
    texture: fields("UnlitTexture")?.source,
    events: [...events],
    errors: [...errors],
  };
}

export async function waitForSource(mesh: string) {
  const deadline = performance.now() + 10_000;
  for (;;) {
    const result = await inspect();
    if (
      result.mesh === mesh &&
      !result.inspection.entities.some(
        (entity) => entity.metadata.symbolicId === "replacement-pending",
      ) &&
      result.inspection.resources.every(
        (resource) => resource.status === "loaded",
      )
    )
      return result;
    if (performance.now() >= deadline)
      throw new Error(`Replacement did not become ready: ${mesh}`);
    await handle.client.waitForFrame(result.inspection.tick);
  }
}

/** Capture during acquisition, deliberately without a resource-readiness barrier. */
export async function sample() {
  const frame = await handle.capture();
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(
        new Uint8ClampedArray(frame.pixels),
        frame.width,
        frame.height,
      ),
      0,
      0,
    );
  const { pixels: _pixels, ...metadata } = frame;
  return {
    ...(await inspect()),
    frame: metadata,
    summary: summarizeImage(frame),
    difference: compareImages(baseline, frame),
    dataUrl: canvas.toDataURL("image/png"),
  };
}

export async function removeScene() {
  mounted = false;
  render();
  return inspect();
}

export async function close() {
  flushSync(() => root.unmount());
  await handle?.closed;
}

export type ReplacementSample = Awaited<ReturnType<typeof sample>>;
