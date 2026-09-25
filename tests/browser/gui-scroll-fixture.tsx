/** Mounted nested ScrollViews and a React VirtualList: React DOM ->
 * IppCanvas -> generated worker client -> WebGL frames. */
import { useCallback, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import type {
  CameraWorldClient,
  GuiSemanticNode,
  GuiWorldClient,
} from "@ipp/client";
import {
  Camera,
  Entity,
  Surface,
  Transform,
} from "../../packages/ipp-react/src/index.js";
import {
  Column,
  GuiRoot,
  ScrollView,
  SizedBox,
  VirtualList,
  type GuiControlTheme,
  type GuiVirtualRange,
} from "../../packages/ipp-react/src/gui.js";
import {
  IppCanvas,
  World,
  type CanvasRuntimeConfiguration,
  type IppCanvasHandle,
} from "../../packages/ipp-react/src/web.js";

/** Canvas size: the orthographic camera maps the 4x3 panel onto it at 60
 * pixels per GUI logical unit, top-left origin. */
const WIDTH = 240;
const HEIGHT = 180;

/** Linear RGBA fills of the scrolled content. */
export const SCROLL_COLORS = {
  outer: [0.2, 0.2, 0.2, 1],
  inner: [0.05, 0.05, 0.3, 1],
  first: [0.8, 0.1, 0.1, 1],
  second: [0.1, 0.8, 0.1, 1],
  narrow: [0.1, 0.1, 0.8, 1],
  last: [0.8, 0.8, 0.1, 1],
} as const;

/** Opaque scroll bar skin on the outer ScrollView, so frames classify the
 * bar column by colour: a cyan track, a magenta thumb that turns white
 * while pressed. */
const scrollBarTheme: GuiControlTheme = {
  name: "scroll-bars",
  parts: {
    scrollTrackY: { base: { color: [0.1, 0.8, 0.8, 1], opacity: 1 } },
    scrollThumbY: {
      base: { color: [0.8, 0.1, 0.8, 1], opacity: 1 },
      pressed: { color: [0.9, 0.9, 0.9, 1] },
    },
  },
};

/** Opaque inner scroll bar skin: a blue track and a yellow thumb, so frames
 * tell the inner bar column apart from the outer bar beside it. */
const innerScrollBarTheme: GuiControlTheme = {
  name: "inner-scroll-bars",
  parts: {
    scrollTrackY: { base: { color: [0.1, 0.1, 0.8, 1], opacity: 1 } },
    scrollThumbY: { base: { color: [0.8, 0.8, 0.1, 1], opacity: 1 } },
  },
};

let root: Root | undefined;
let handle: IppCanvasHandle | undefined;
let cameraReady = false;
let commits = 0;
let errors: string[] = [];
let ranges: GuiVirtualRange[] = [];

/** Items of the mounted VirtualList: estimate, count and per-index fills. */
export const VIRTUAL_LIST = {
  count: 100_000,
  estimate: 0.75,
  /** Even items are half a unit tall, odd items one unit. */
  heights: [0.5, 1],
  /** Fills by index modulo four: red, green, blue, yellow. */
  colors: [
    [0.8, 0.1, 0.1, 1],
    [0.1, 0.8, 0.1, 1],
    [0.1, 0.1, 0.8, 1],
    [0.8, 0.8, 0.1, 1],
  ],
} as const;

/** A 4x3 VirtualList over 100000 items estimated at 0.75 units whose
 * declared items measure 0.5 or 1 unit, with the opaque scroll bar skin. */
function virtualListPanel(): ReactElement {
  return (
    <Column width={4} height={3}>
      <VirtualList
        width={4}
        height={3}
        itemCount={VIRTUAL_LIST.count}
        itemExtent={VIRTUAL_LIST.estimate}
        overscan={1}
        backgroundColor={SCROLL_COLORS.outer}
        theme={scrollBarTheme}
        onRangeChange={(range) => ranges.push(range)}
        renderItem={(index) => (
          <SizedBox
            width={4}
            height={VIRTUAL_LIST.heights[index % 2]}
            backgroundColor={VIRTUAL_LIST.colors[index % 4]}
          />
        )}
      />
    </Column>
  );
}

async function activateCamera(next: IppCanvasHandle): Promise<void> {
  const client = next.client as CameraWorldClient;
  const deadline = performance.now() + 10_000;
  for (;;) {
    const camera = (await client.inspect()).entities.find(
      (entity) => entity.metadata.symbolicId === "scroll-camera",
    );
    if (camera !== undefined) {
      await new Promise<void>((resolve, reject) => {
        let stop = (): void => {};
        const timeout = setTimeout(() => {
          stop();
          reject(
            new Error("scroll fixture camera activation was not observed"),
          );
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
      throw new Error("scroll fixture camera did not acknowledge");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
}

/**
 * An outer 4x3 ScrollView over 5 units of content: an inner 4x2 ScrollView
 * over 3 units (a 2-unit red block then a 1-unit green block), a narrow
 * 2x2 blue block at the left and a 1-unit yellow block. The outer view can
 * scroll by 2 and the inner view by 1. The outer vertical scroll bar spans
 * x 3.85..4 with a 1.8-unit thumb travelling 1.2 units over that capacity.
 * The inner bar, 0.1 thick, would share that edge, so it ends at the outer
 * track's inner edge: x 3.75..3.85, with a 4/3-unit thumb.
 * Wheel input keeps the relay's default step of 0.25 units per notch.
 */
function nestedScrollPanel(): ReactElement {
  return (
    <Column width={4} height={3}>
      <ScrollView
        width={4}
        height={3}
        backgroundColor={SCROLL_COLORS.outer}
        theme={scrollBarTheme}
      >
        <Column width={4}>
          <ScrollView
            width={4}
            height={2}
            backgroundColor={SCROLL_COLORS.inner}
            theme={innerScrollBarTheme}
          >
            <Column width={4}>
              <SizedBox
                width={4}
                height={2}
                backgroundColor={SCROLL_COLORS.first}
              />
              <SizedBox
                width={4}
                height={1}
                backgroundColor={SCROLL_COLORS.second}
              />
            </Column>
          </ScrollView>
          <SizedBox
            width={2}
            height={2}
            backgroundColor={SCROLL_COLORS.narrow}
          />
          <SizedBox width={4} height={1} backgroundColor={SCROLL_COLORS.last} />
        </Column>
      </ScrollView>
    </Column>
  );
}

function Application({
  runtime,
  panel,
}: {
  readonly runtime: CanvasRuntimeConfiguration;
  readonly panel: () => ReactElement;
}): ReactElement {
  const ready = useCallback((next: IppCanvasHandle) => {
    handle = next;
    void activateCamera(next).catch((error: unknown) => {
      errors.push(error instanceof Error ? error.message : String(error));
    });
  }, []);
  return (
    <IppCanvas
      runtime={runtime}
      width={WIDTH}
      height={HEIGHT}
      canvasProps={{ id: "scroll-gui-canvas" }}
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
        <Entity id="scroll-camera">
          <Transform z={6} />
          <Camera projection={1} ortho_height={3} />
        </Entity>
        <Entity id="scroll-panel">
          <Transform />
          <Surface width={4} height={3} />
          <GuiRoot>{panel()}</GuiRoot>
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

/** Mount the nested ScrollView panel and wait for its first commit. */
export async function mountScrollCanvas(
  runtime: CanvasRuntimeConfiguration,
): Promise<void> {
  await mount(runtime, nestedScrollPanel);
}

/** Mount the VirtualList panel and wait for its first commit. */
export async function mountVirtualListCanvas(
  runtime: CanvasRuntimeConfiguration,
): Promise<void> {
  await mount(runtime, virtualListPanel);
}

/** Wanted ranges the mounted VirtualList observed, oldest first. */
export function virtualListRanges(): readonly GuiVirtualRange[] {
  return [...ranges];
}

/** The mounted VirtualList's semantic node: its scroll and anchor. */
export async function virtualListSemantics(): Promise<GuiSemanticNode> {
  const current = handle;
  if (current === undefined) throw new Error("scroll fixture is not mounted");
  const client = current.client as unknown as GuiWorldClient;
  const panel = (await client.inspect()).entities.find(
    (entity) => entity.metadata.symbolicId === "scroll-panel",
  );
  if (panel === undefined) throw new Error("scroll panel is missing");
  const node = (await client.semanticSnapshot({ entity: panel.id })).nodes.find(
    (item) => item.role === "virtualList",
  );
  if (node === undefined) throw new Error("VirtualList has no semantic node");
  return node;
}

async function mount(
  runtime: CanvasRuntimeConfiguration,
  panel: () => ReactElement,
): Promise<void> {
  await closeScrollCanvas();
  cameraReady = false;
  commits = 0;
  errors = [];
  ranges = [];
  const host = document.createElement("div");
  host.id = "scroll-gui-host";
  document.body.replaceChildren(host);
  root = createRoot(host);
  root.render(<Application runtime={runtime} panel={panel} />);
  await until(
    () => handle !== undefined && commits > 0 && cameraReady,
    `mounted scroll panel did not acknowledge: ${errors.join("; ")}`,
  );
  await handle!.flush();
}

/** Average linear-to-display RGB over a 7x7 block around one pixel. */
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

/** One completed frame: sampled colours at canvas pixels plus draw state. */
export async function scrollFrame(
  points: readonly (readonly [number, number])[],
): Promise<{
  readonly width: number;
  readonly height: number;
  readonly failedDrawCalls: number;
  readonly samples: readonly (readonly [number, number, number])[];
  readonly dataUrl: string;
}> {
  const current = handle;
  if (current === undefined) throw new Error("scroll fixture is not mounted");
  await current.flush();
  const frame = await current.capture();
  const pixels = new Uint8Array(frame.pixels);
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d");
  if (context === null) throw new Error("2D evidence canvas is unavailable");
  context.putImageData(
    new ImageData(new Uint8ClampedArray(pixels), frame.width, frame.height),
    0,
    0,
  );
  return {
    width: frame.width,
    height: frame.height,
    failedDrawCalls: frame.failedDrawCalls ?? -1,
    samples: points.map(([x, y]) =>
      averageRgb(pixels, frame.width, frame.height, x, y),
    ),
    dataUrl: canvas.toDataURL("image/png"),
  };
}

/** Fixture errors reported by the canvas or World. */
export function scrollErrors(): readonly string[] {
  return [...errors];
}

/** Unmount and wait for the canvas session to close. */
export async function closeScrollCanvas(): Promise<number> {
  const closing = handle;
  handle = undefined;
  const activeRoot = root;
  root = undefined;
  activeRoot?.unmount();
  if (closing !== undefined) await closing.closed;
  return document.querySelectorAll("canvas").length;
}
