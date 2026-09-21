/** Mounted browser GUI fixture: React DOM -> IppCanvas -> generated worker client. */
import { useCallback, useState, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import type {
  CameraWorldClient,
  GuiNodeHandle,
  GuiObservationBatch,
  GuiTextFocusState,
  GuiWorldClient,
} from "@ipp/client";
import {
  Camera,
  Entity,
  Surface,
  Transform,
} from "../../packages/ipp-react/src/index.js";
import {
  Button,
  Checkbox,
  Column,
  Drawing,
  GuiRoot,
  Row,
  Slider,
  TextInput,
  type GuiControlTheme,
} from "../../packages/ipp-react/src/gui.js";
import {
  IppCanvas,
  World,
  type CanvasRuntimeConfiguration,
  type IppCanvasHandle,
} from "../../packages/ipp-react/src/web.js";

const WIDTH = 240;
const HEIGHT = 180;
const controlTheme: GuiControlTheme = {
  font: {
    kind: 17,
    source: new URL(
      "/target/font-assets/shure-tech-mono.ippf",
      globalThis.location.href,
    ).href,
  },
  parts: {
    background: {
      base: { color: [0.08, 0.18, 0.42, 1], opacity: 1, scale: [1, 1] },
      hovered: { color: [0.14, 0.32, 0.7, 1] },
      pressed: { color: [0.04, 0.1, 0.28, 1] },
    },
    label: { base: { color: [0.96, 0.98, 1, 1] } },
    focusRing: { base: { color: [1, 0.72, 0.08, 1], opacity: 1 } },
  },
};
const iconTheme: GuiControlTheme = {
  parts: {
    icon: { base: { color: [0.15, 0.9, 0.55, 1], opacity: 1, scale: [1, 1] } },
  },
};
const textRef: { current: GuiNodeHandle | null } = {
  current: null,
};
const buttonRef: { current: GuiNodeHandle | null } = {
  current: null,
};
const checkboxRef: { current: GuiNodeHandle | null } = {
  current: null,
};
const sliderRef: { current: GuiNodeHandle | null } = {
  current: null,
};

let root: Root | undefined;
let handle: IppCanvasHandle | undefined;
let rerenderEquivalent: (() => void) | undefined;
let unsubscribe: (() => void) | undefined;
let editorIdentity: HTMLTextAreaElement | undefined;
let latestTextFocus: GuiTextFocusState | null | undefined;
let commits = 0;
let callbackValues: string[] = [];
let callbackRenders: number[] = [];
let presses = 0;
let errors: string[] = [];
let cameraReady = false;
let observationTrace: string[] = [];
let renderedRevision = 0;

function iconSource() {
  return {
    kind: 18,
    source: new URL(
      "/target/surface-assets/icon.ippd",
      globalThis.location.href,
    ).href,
  } as const;
}

const drawingControlTheme: GuiControlTheme = {
  ...controlTheme,
  parts: {
    ...controlTheme.parts,
    icon: {
      base: { asset: iconSource(), opacity: 1, scale: [1, 1] },
    },
  },
};

async function activateCamera(next: IppCanvasHandle): Promise<void> {
  const client = next.client as CameraWorldClient;
  const deadline = performance.now() + 10_000;
  for (;;) {
    const camera = (await client.inspect()).entities.find(
      (entity) => entity.metadata.symbolicId === "mounted-gui-camera",
    );
    if (camera !== undefined) {
      await new Promise<void>((resolve, reject) => {
        let stop = (): void => {};
        const timeout = setTimeout(() => {
          stop();
          reject(new Error("mounted GUI camera activation was not observed"));
        }, 10_000);
        stop = client.onCameraStateChanged((event) => {
          if (event.changes.activeCamera !== camera.id) return;
          clearTimeout(timeout);
          stop();
          resolve();
        });
        client.sendCommand({
          type: "CameraActivateCommand",
          entity: camera.id,
        });
      });
      cameraReady = true;
      return;
    }
    if (performance.now() >= deadline)
      throw new Error("mounted GUI camera did not acknowledge");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
}

function Application({
  runtime,
}: {
  readonly runtime: CanvasRuntimeConfiguration;
}): ReactElement {
  const [renderRevision, setRenderRevision] = useState(0);
  renderedRevision = renderRevision;
  rerenderEquivalent = () => setRenderRevision((value) => value + 1);
  const ready = useCallback((next: IppCanvasHandle) => {
    handle = next;
    unsubscribe?.();
    unsubscribe = (next.client as GuiWorldClient).subscribeGuiObservations?.(
      (batch: GuiObservationBatch) => {
        observationTrace.push(
          [
            batch.textFocus === undefined
              ? "focus:omitted"
              : batch.textFocus === null
                ? "focus:cleared"
                : `focus:${batch.textFocus.node}:${batch.textFocus.selectionStart}-${batch.textFocus.selectionEnd}`,
            `effects:${batch.effects.map((effect) => effect.kind).join(",")}`,
            `unhandled:${(batch.unhandled ?? [])
              .map((item) => item.reason.kind)
              .join(",")}`,
          ].join(" "),
        );
        if (observationTrace.length > 20) observationTrace.shift();
        if (batch.textFocus !== undefined) latestTextFocus = batch.textFocus;
      },
    );
    void activateCamera(next).catch((error: unknown) => {
      errors.push(error instanceof Error ? error.message : String(error));
    });
  }, []);
  return (
    <IppCanvas
      runtime={runtime}
      width={WIDTH}
      height={HEIGHT}
      canvasProps={{ id: "mounted-gui-canvas" }}
      guiInput={{}}
      onReady={ready}
      onError={(error) => errors.push(error.message)}
    >
      <World
        onCommit={() => {
          commits += 1;
        }}
        onError={(error) => errors.push(error.message)}
      >
        <Entity id="mounted-gui-camera">
          <Transform z={6} />
          <Camera projection={1} ortho_height={3} />
        </Entity>
        <Entity id="mounted-gui-panel">
          <Transform />
          <Surface width={4} height={3} />
          <GuiRoot>
            <Column width={4} height={3}>
              <TextInput
                nodeRef={textRef}
                width={4}
                height={1.25}
                text="a😀b"
                placeholder="Edit"
                theme={controlTheme}
                onTextCommit={(event) => {
                  callbackValues.push(event.value);
                  callbackRenders.push(renderRevision);
                }}
              />
              <Row width={4} height={0.5}>
                <Drawing
                  width={1}
                  height={0.5}
                  asset={iconSource()}
                  theme={iconTheme}
                />
                <Checkbox
                  nodeRef={checkboxRef}
                  width={0.75}
                  height={0.5}
                  checked={false}
                  theme={drawingControlTheme}
                />
                <Slider
                  nodeRef={sliderRef}
                  width={2.25}
                  height={0.5}
                  value={0.2}
                  min={0}
                  max={1}
                  theme={controlTheme}
                />
              </Row>
              <Button
                nodeRef={buttonRef}
                width={4}
                height={1.25}
                label="Done"
                theme={controlTheme}
                onPress={() => {
                  presses += 1;
                }}
              />
            </Column>
          </GuiRoot>
        </Entity>
      </World>
    </IppCanvas>
  );
}

async function until(predicate: () => boolean, message: string): Promise<void> {
  const deadline = performance.now() + 10_000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
}

export async function mountGuiCanvas(
  runtime: CanvasRuntimeConfiguration,
): Promise<void> {
  await closeGuiCanvas();
  handle = undefined;
  latestTextFocus = undefined;
  commits = 0;
  callbackValues = [];
  callbackRenders = [];
  presses = 0;
  errors = [];
  cameraReady = false;
  observationTrace = [];
  renderedRevision = 0;
  textRef.current = null;
  buttonRef.current = null;
  checkboxRef.current = null;
  sliderRef.current = null;
  const host = document.createElement("div");
  host.id = "mounted-gui-host";
  document.body.replaceChildren(host);
  root = createRoot(host);
  root.render(<Application runtime={runtime} />);
  await until(
    () =>
      handle !== undefined &&
      commits > 0 &&
      textRef.current !== null &&
      checkboxRef.current !== null &&
      sliderRef.current !== null &&
      cameraReady,
    "mounted IppCanvas GUI did not acknowledge",
  );
  await handle!.flush();
  const expectedAssets = new Set([
    iconSource().source,
    controlTheme.font!.source,
  ]);
  const deadline = performance.now() + 10_000;
  for (;;) {
    const assets = (await handle!.client.inspect()).resources.filter(
      (resource) => expectedAssets.has(resource.source),
    );
    const failed = assets.find((asset) => asset.status === "failed");
    if (failed !== undefined)
      throw new Error(
        `mounted GUI asset failed (${failed.source}): ${failed.error ?? "unknown"}`,
      );
    if (
      assets.length === expectedAssets.size &&
      assets.every((asset) => asset.status === "loaded")
    )
      break;
    if (performance.now() >= deadline)
      throw new Error("mounted GUI theme assets did not load");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
  editorIdentity = document.querySelector("textarea") ?? undefined;
}

function averageRgb(
  pixels: Uint8Array,
  width: number,
  height: number,
  x: number,
  y: number,
): readonly [number, number, number] {
  const sum = [0, 0, 0];
  let count = 0;
  for (let py = Math.max(0, y - 3); py <= Math.min(height - 1, y + 3); py++)
    for (let px = Math.max(0, x - 3); px <= Math.min(width - 1, x + 3); px++) {
      const offset = (py * width + px) * 4;
      sum[0] += pixels[offset]!;
      sum[1] += pixels[offset + 1]!;
      sum[2] += pixels[offset + 2]!;
      count += 1;
    }
  return [
    Math.round(sum[0]! / count),
    Math.round(sum[1]! / count),
    Math.round(sum[2]! / count),
  ];
}

export async function controlPaintObservation(): Promise<{
  readonly checked: boolean;
  readonly slider: number;
  readonly drawCalls: number;
  readonly failedDrawCalls: number;
  readonly checkboxIndicator: readonly [number, number, number];
  readonly checkboxOutsideIndicator: readonly [number, number, number];
  readonly sliderInitialThumb: readonly [number, number, number];
  readonly sliderMovedThumb: readonly [number, number, number];
}> {
  const current = handle;
  const checkbox = checkboxRef.current;
  const slider = sliderRef.current;
  if (current === undefined || checkbox === null || slider === null)
    throw new Error("GUI control paint fixture is not ready");
  await current.flush();
  const [checkboxInspection, sliderInspection, frame] = await Promise.all([
    (current.client as GuiWorldClient).inspectGui({
      entity: checkbox.entity,
      nodeId: checkbox.nodeId,
      maxDepth: 1,
    }),
    (current.client as GuiWorldClient).inspectGui({
      entity: slider.entity,
      nodeId: slider.nodeId,
      maxDepth: 1,
    }),
    current.capture(),
  ]);
  const checked = checkboxInspection.nodes[0]?.controlValue;
  const scalar = sliderInspection.nodes[0]?.controlValue;
  if (checked?.kind !== "bool" || scalar?.kind !== "scalar")
    throw new Error("GUI control paint values disappeared");
  const pixels = new Uint8Array(frame.pixels);
  const failedDrawCalls = frame.backend.failedDrawCalls;
  if (typeof failedDrawCalls !== "number")
    throw new Error("GUI control paint capture omitted failed draw calls");
  return {
    checked: checked.value,
    slider: scalar.value,
    drawCalls: frame.drawCalls,
    failedDrawCalls,
    checkboxIndicator: averageRgb(pixels, frame.width, frame.height, 83, 90),
    checkboxOutsideIndicator: averageRgb(
      pixels,
      frame.width,
      frame.height,
      98,
      90,
    ),
    sliderInitialThumb: averageRgb(pixels, frame.width, frame.height, 139, 90),
    sliderMovedThumb: averageRgb(pixels, frame.width, frame.height, 216, 90),
  };
}

export async function captureThemeEvidence(): Promise<{
  readonly width: number;
  readonly height: number;
  readonly drawCalls: number;
  readonly coloredPixels: number;
  readonly parts: readonly string[];
}> {
  const current = handle;
  if (current === undefined) throw new Error("GUI fixture is not mounted");
  const frame = await current.capture();
  const pixels = new Uint8Array(frame.pixels);
  let coloredPixels = 0;
  for (let index = 0; index < pixels.length; index += 4)
    if (
      pixels[index] !== 0 ||
      pixels[index + 1] !== 0 ||
      pixels[index + 2] !== 0
    )
      coloredPixels += 1;
  const inspection = await current.client.inspect();
  const panel = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === "mounted-gui-panel",
  );
  const descriptor = current.client.components.GuiRoot;
  const properties =
    descriptor === undefined
      ? undefined
      : panel?.effective.find(
          (component) => component.component === descriptor.id,
        )?.properties;
  const parts = new Set<string>();
  for (const name of Object.keys(properties ?? {})) {
    const match =
      /^node_[1-9][0-9]*_part_(background|label|icon|focusRing)(?:_|$)/.exec(
        name,
      );
    if (match?.[1] !== undefined) parts.add(match[1]);
  }
  return {
    width: frame.width,
    height: frame.height,
    drawCalls: frame.drawCalls,
    coloredPixels,
    parts: [...parts].sort(),
  };
}

export function equivalentRerender(): void {
  if (rerenderEquivalent === undefined)
    throw new Error("GUI fixture is not mounted");
  rerenderEquivalent();
}

export async function observation(): Promise<{
  readonly text: string;
  readonly revision: number;
  readonly callbackValues: readonly string[];
  readonly callbackRenders: readonly number[];
  readonly presses: number;
  readonly commits: number;
  readonly renderRevision: number;
  readonly errors: readonly string[];
  readonly activeEditor: boolean;
  readonly sameEditor: boolean;
  readonly editorCount: number;
  readonly selectionDirection: string | null;
  readonly domSelection: readonly [number, number] | null;
  readonly focusSelection: readonly [number, number] | null;
  readonly observationTrace: readonly string[];
  readonly scene: {
    readonly cameraTransform?: Readonly<Record<string, unknown>>;
    readonly camera?: Readonly<Record<string, unknown>>;
    readonly panelTransform?: Readonly<Record<string, unknown>>;
    readonly surface?: Readonly<Record<string, unknown>>;
    readonly canvasRect?: readonly [number, number, number, number];
  };
}> {
  const current = handle;
  const textHandle = textRef.current;
  if (current === undefined || textHandle === null)
    throw new Error("GUI fixture is not ready");
  await current.flush();
  const inspected = await (current.client as GuiWorldClient).inspectGui({
    entity: textHandle.entity,
    nodeId: textHandle.nodeId,
    maxDepth: 1,
  });
  const node = inspected.nodes[0];
  if (node?.controlValue.kind !== "text")
    throw new Error("mounted text input disappeared");
  const editor = document.querySelector("textarea");
  const world = await current.client.inspect();
  const cameraEntity = world.entities.find(
    (entity) => entity.metadata.symbolicId === "mounted-gui-camera",
  );
  const panelEntity = world.entities.find(
    (entity) => entity.metadata.symbolicId === "mounted-gui-panel",
  );
  const fields = (entity: typeof cameraEntity, component: string) => {
    const descriptor = current.client.components[component];
    return descriptor === undefined
      ? undefined
      : entity?.effective.find((value) => value.component === descriptor.id)
          ?.fields;
  };
  const canvas = document.querySelector("#mounted-gui-canvas");
  const rect = canvas?.getBoundingClientRect();
  return {
    text: node.controlValue.value,
    revision: node.controlRevision,
    callbackValues: [...callbackValues],
    callbackRenders: [...callbackRenders],
    presses,
    commits,
    renderRevision: renderedRevision,
    errors: [...errors],
    activeEditor: document.activeElement === editor,
    sameEditor: editor === editorIdentity,
    editorCount: document.querySelectorAll("textarea").length,
    selectionDirection: editor?.selectionDirection ?? null,
    domSelection:
      editor?.selectionStart === null || editor?.selectionEnd === null
        ? null
        : [editor.selectionStart, editor.selectionEnd],
    focusSelection:
      latestTextFocus && latestTextFocus !== null
        ? [latestTextFocus.selectionStart, latestTextFocus.selectionEnd]
        : null,
    observationTrace: [...observationTrace],
    scene: {
      cameraTransform: fields(cameraEntity, "Transform"),
      camera: fields(cameraEntity, "Camera"),
      panelTransform: fields(panelEntity, "Transform"),
      surface: fields(panelEntity, "Surface"),
      ...(rect === undefined
        ? {}
        : { canvasRect: [rect.left, rect.top, rect.width, rect.height] }),
    },
  };
}

export async function closeGuiCanvas(): Promise<{
  readonly canvasCount: number;
  readonly editorCount: number;
}> {
  unsubscribe?.();
  unsubscribe = undefined;
  const closing = handle;
  handle = undefined;
  rerenderEquivalent = undefined;
  const activeRoot = root;
  root = undefined;
  activeRoot?.unmount();
  if (closing !== undefined) await closing.closed;
  return {
    canvasCount: document.querySelectorAll("canvas").length,
    editorCount: document.querySelectorAll("textarea").length,
  };
}
