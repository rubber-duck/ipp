import { sameOutputReference } from "../../../packages/ipp-client/src/references.js";
import { StrictMode } from "react";
import { createRoot, type Root as ReactDomRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import type {
  AssetWorldClient,
  AssetResourceSnapshot,
  OutputReference,
  PresentationView,
  PresentedCapture,
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
import { ReadyGeometry } from "../../../examples/world-gallery/shared/ready-geometry.js";
import { createFixtureCamera } from "../../fixtures/cameras.js";
import {
  compareImages,
  summarizeImage,
  type FramePixels,
  rgbaDataUrl,
} from "../../harness/page/images.js";
import {
  capturedImage,
  worldReference,
} from "../../harness/page/presentation.js";
import {
  LIFECYCLE,
  SCENE,
  selectSystems,
} from "../../fixtures/system-selections.js";

let root: ReactDomRoot;
let handle: IppCanvasHandle;
let configuration: CanvasRuntimeConfiguration;
let strict: boolean;
let baseline: FramePixels;
let output: OutputReference | null = null;
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
  onViewChange = (view) => {
    if (view && output && sameOutputReference(view.binding.output, output))
      ready();
  };
  onReady = (canvas) => {
    handle = canvas;
    (canvas.client as AssetWorldClient).onResourceChange((event) =>
      events.push(event),
    );
    void (async () => {
      const camera = await createFixtureCamera(canvas.client);
      output = await canvas.host.bindOutput(
        worldReference(canvas.client),
        camera,
        "camera",
      );
      render();
    })().catch(reject);
  };
  render();
  await connected;
  await waitForSource(source);
  baseline = capturedImage(await capture());
}

let onReady: (canvas: IppCanvasHandle) => void;
let onViewChange: (view: PresentationView | null) => void;

/** A completed draw including the selected Camera's content admitted before the request. */
function capture(): Promise<PresentedCapture> {
  if (!output) throw new Error("Fixture Camera output is not bound");
  return handle.capture({ afterOutputs: [output] });
}

function render() {
  const app = (
    <IppCanvas
      runtime={configuration}
      world={{ create: { selectedSystems: selectSystems(SCENE, LIFECYCLE) } }}
      output={output}
      width={320}
      height={240}
      onReady={onReady}
      onViewChange={onViewChange}
      onError={(error) => errors.push(error.message)}
    >
      {/* Unmounting a World root deletes nothing, so the scene is removed
          as declarations while the root stays mounted. */}
      <World>
        {mounted && (
          <>
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
          </>
        )}
      </World>
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
    entity?.components.find(
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
  const frame = await capture();
  const image = capturedImage(frame);
  const { pixels: _pixels, ...metadata } = frame;
  const [selected] = metadata.sources;
  if (!selected || !output || !sameOutputReference(selected.output, output))
    throw new Error("Completed draw omitted the fixture Camera output");
  return {
    ...(await inspect()),
    frame: metadata,
    tick: selected.tick,
    summary: summarizeImage(image),
    difference: compareImages(baseline, image),
    dataUrl: rgbaDataUrl(image),
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
