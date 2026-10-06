import { createFixtureCamera } from "../../../fixtures/cameras.js";
import { StrictMode } from "react";
import { createRoot, type Root as ReactDomRoot } from "react-dom/client";
import type { Client, OutputReference } from "@ipp/client";
import { Entity, UnlitMaterial } from "@ipp/react";
import {
  IppCanvas,
  World,
  type CanvasRuntimeConfiguration,
  type IppCanvasHandle,
} from "@ipp/react/web";
import { summarizeImage, rgbaDataUrl } from "../../../harness/page/images.js";
import {
  type CanvasRuntimeInput,
  fields,
  requireComponent,
  entity,
  numericField,
  waitForResource,
  waitUntil,
  animationBarrier,
  installTransferObserver,
  releaseTransferObserver,
  transferObservation,
  capturedPixels,
} from "../support/canvas-page.js";
import {
  ATTACHMENTS,
  SCENE,
  selectSystems,
} from "../../../fixtures/system-selections.js";

const WIDTH = 320;
const HEIGHT = 240;
const CUBE_RECIPE = "ipp://mesh/cube?width=2&height=2&length=2";

interface SavedCanvasFixture {
  configuration: CanvasRuntimeInput;
  url: string;
  mode: "ready" | "gate" | "throw" | "mapped";
  strict: boolean;
  root: ReactDomRoot;
  handle?: IppCanvasHandle;
  output?: OutputReference;
  initialized: number;
  finished: number;
  ready: number;
  commits: number;
  aborted: number;
  callbackRevision: number;
  errors: string[];
  release?: () => void;
}
let savedCanvas: SavedCanvasFixture | undefined;

/** Generate a durable file using the real worker Host and production snapshot codec. */
export async function makeSavedCanvasWorld(
  configuration: CanvasRuntimeInput,
  source = "saved-fixture:///cube",
) {
  const module = await import(configuration.generatedModuleUrl);
  const host = await module.IppHostClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    { canvas: new OffscreenCanvas(WIDTH, HEIGHT), timeoutMs: 10000 },
  );
  try {
    const created = await host.createWorld({
      selectedSystems: selectSystems(SCENE, ATTACHMENTS),
      symbolicId: "canvas-saved-world",
    });
    const client: Client = await host.openWorld(created.reference);
    const child = await host.createWorld({
      selectedSystems: selectSystems(SCENE),
      symbolicId: "canvas-saved-child",
    });
    await createFixtureCamera(client);
    const transform = requireComponent(client, "Transform");
    const material = requireComponent(client, "UnlitMaterial");
    const mesh = requireComponent(client, "MeshInstance");
    const ref = { kind: "alias", alias: 1 } as const;
    const outcome = await client.batch([
      {
        kind: "create",
        alias: 1,
        metadata: { symbolicId: "saved-cube", classes: [] },
      },
      {
        kind: "insertComponent",
        entity: ref,
        component: transform.id,
        fields: [],
      },
      {
        kind: "insertComponent",
        entity: ref,
        component: material.id,
        fields: fields(material, { r: 0.8, g: 0.2, b: 0.1 }),
      },
      {
        kind: "insertComponent",
        entity: ref,
        component: mesh.id,
        fields: fields(mesh, { source }),
      },
    ]);
    if (!outcome.ok) throw new Error(outcome.error.reason);
    const attachment = requireComponent(client, "WorldAttachment");
    const attached = await client.batch([
      {
        kind: "insertComponent",
        entity: {
          kind: "handle",
          id: outcome.aliases.find((entry) => entry.alias === 1)!.id,
        },
        component: attachment.id,
        fields: [
          {
            offset: attachment.fields.child!.offset,
            value: { kind: "world", value: child.reference },
          },
        ],
      },
    ]);
    if (!attached.ok) throw new Error(attached.error.reason);
    return Array.from(await host.saveWorld(client.session));
  } finally {
    await host.close();
  }
}

export async function mountSavedCanvas(
  configuration: CanvasRuntimeInput,
  url: string,
  mode: "ready" | "gate" | "throw" | "mapped" = "ready",
  strict = true,
) {
  await closeSavedCanvas();
  installTransferObserver();
  const element = document.createElement("div");
  element.id = "saved-canvas-host";
  document.body.replaceChildren(element);
  savedCanvas = {
    configuration,
    url,
    mode,
    strict,
    root: createRoot(element),
    initialized: 0,
    finished: 0,
    ready: 0,
    commits: 0,
    aborted: 0,
    errors: [],
    callbackRevision: 0,
  };
  renderSavedCanvas(savedCanvas);
  await animationBarrier();
}

function renderSavedCanvas(state: SavedCanvasFixture) {
  const content = (
    <IppCanvas
      runtime={state.configuration}
      world={{ load: { url: state.url } }}
      output={state.output ?? null}
      width={WIDTH}
      height={HEIGHT}
      canvasProps={{ id: "saved-canvas" }}
      initialize={async (client, signal, host) => {
        state.initialized++;
        delete state.release;
        signal.addEventListener(
          "abort",
          () => {
            state.aborted++;
          },
          { once: true },
        );
        const inspection = await client.inspect();
        const cube = entity(inspection, "saved-cube");
        const camera = entity(inspection, "__fixture-camera");
        if (!cube || !camera)
          throw new Error("Initialization did not receive loaded entities");
        const material = requireComponent(client, "UnlitMaterial");
        if (numericField(cube, material.id, "g")! > 0.3)
          throw new Error("React binding ran before initialization");
        if (state.mode === "gate")
          await new Promise<void>((resolve) => {
            state.release = resolve;
          });
        signal.throwIfAborted();
        if (state.mode === "throw")
          throw new Error("Saved World initializer rejected");
        const mesh = requireComponent(client, "MeshInstance");
        const outcome = await client.batch(
          state.mode === "mapped"
            ? []
            : [
                {
                  kind: "setField",
                  entity: { kind: "handle", id: cube.id },
                  component: mesh.id,
                  field: fields(mesh, { source: CUBE_RECIPE })[0]!,
                },
              ],
        );
        if (!outcome.ok) throw new Error(outcome.error.reason);
        state.output = await host.bindOutput(
          client.worldReference!,
          camera.id,
          "camera",
        );
        await client.inspect();
        state.finished++;
        renderSavedCanvas(state);
      }}
      onReady={(handle) => {
        state.handle = handle;
        state.ready++;
      }}
      onError={(error) => state.errors.push(error.message)}
    >
      <World
        onCommit={() => {
          state.commits++;
        }}
        onError={(error) => state.errors.push(error.message)}
      >
        <Entity bindTo="saved-cube">
          <UnlitMaterial g={0.8} r={0.1} b={0.2} />
        </Entity>
      </World>
    </IppCanvas>
  );
  state.root.render(
    state.strict ? <StrictMode>{content}</StrictMode> : content,
  );
}

export async function savedCanvasObservation() {
  const state = savedCanvas!;
  const inspection = state.handle
    ? await state.handle.client.inspect()
    : undefined;
  return {
    initialized: state.initialized,
    finished: state.finished,
    ready: state.ready,
    commits: state.commits,
    aborted: state.aborted,
    gated: state.release !== undefined,
    errors: [...state.errors],
    entities: inspection?.entities.map((e) => e.metadata.symbolicId) ?? [],
    resources: inspection?.resources ?? [],
    transfers: transferObservation(),
  };
}

export async function waitForSavedCanvas(
  stage: "initialized" | "ready" | "failed",
  count = 1,
) {
  const state = savedCanvas!;
  await waitUntil(
    () =>
      stage === "initialized"
        ? state.initialized >= count &&
          (state.mode !== "gate" || state.release !== undefined)
        : stage === "failed"
          ? state.errors.length > 0
          : state.finished >= count &&
            state.ready >= count &&
            state.commits >= count,
    `saved canvas ${stage}`,
  );
  return savedCanvasObservation();
}

export async function releaseSavedInitialization() {
  savedCanvas?.release?.();
  await animationBarrier();
}

export async function captureSavedCanvas(source = CUBE_RECIPE) {
  const state = savedCanvas!;
  const client = state.handle!.client;
  await waitForResource(client, 1, source);
  const frame = await state.handle!.capture();
  return {
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    summary: summarizeImage(capturedPixels(frame)),
    dataUrl: rgbaDataUrl(capturedPixels(frame)),
  };
}

export async function replaceSavedWorld(url: string) {
  const state = savedCanvas!;
  const previous = state.handle!;
  state.url = url;
  delete state.output;
  delete state.handle;
  renderSavedCanvas(state);
  await previous.closed;
  return savedCanvasObservation();
}

export async function rerenderSavedInitializer() {
  const state = savedCanvas!;
  state.callbackRevision++;
  renderSavedCanvas(state);
  await animationBarrier();
  await state.handle?.flush();
  return savedCanvasObservation();
}

export async function unmountSavedCanvas() {
  const state = savedCanvas;
  state?.root.unmount();
  if (state?.handle) await state.handle.closed;
  await animationBarrier();
}

export async function closeSavedCanvas() {
  const state = savedCanvas;
  if (!state) return;
  await unmountSavedCanvas();
  state.release?.();
  await animationBarrier();
  savedCanvas = undefined;
  document.querySelector("#saved-canvas-host")?.remove();
  releaseTransferObserver();
}

export async function replaceSavedResourceUrls(
  resourceUrls: NonNullable<CanvasRuntimeConfiguration["resourceUrls"]>,
) {
  const state = savedCanvas!;
  const previous = state.handle!;
  const count = state.ready;
  const changed =
    JSON.stringify(resourceUrls) !==
    JSON.stringify(state.configuration.resourceUrls);
  state.configuration = { ...state.configuration, resourceUrls };
  if (changed) delete state.handle;
  renderSavedCanvas(state);
  if (changed) {
    await previous.closed;
    await waitUntil(() => state.ready > count, "replacement resource mapping");
  }
  await animationBarrier();
  await state.handle!.flush();
  return savedCanvasObservation();
}

/** Exercise live browser resize callbacks after failed startup has closed its worker. */
export async function resizeFailedSavedCanvas() {
  const canvas = document.querySelector<HTMLCanvasElement>("#saved-canvas")!;
  canvas.style.width = "217px";
  canvas.style.height = "143px";
  window.dispatchEvent(new Event("resize"));
  await animationBarrier();
  await animationBarrier();
  return savedCanvasObservation();
}
