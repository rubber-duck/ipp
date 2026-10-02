/** Mounted nested ScrollViews and a React VirtualList: React DOM ->
 * IppCanvas -> generated worker client -> WebGL frames, with ordinary GUI
 * entities in an attached child World. */
import { useCallback, useState, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Client, OutputReference } from "@ipp/client";
import {
  CanvasWorld,
  Children,
  Entity,
  Surface,
  Transform,
  type CanvasWorldHandle,
} from "../../packages/ipp-react/src/index.js";
import {
  Box,
  Layout,
  ScrollView,
  Skin,
  Style,
  Theme,
  VirtualList,
  type GuiControlHandle,
  type GuiVirtualRange,
} from "../../packages/ipp-react/src/gui.js";
import {
  IppCanvas,
  World,
  type CanvasRuntimeConfiguration,
  type IppCanvasHandle,
} from "../../packages/ipp-react/src/web.js";
import {
  HEIGHT,
  PANEL_SYSTEMS,
  UNITS_PER_METRE,
  WIDTH,
  averageRgb,
  encodeTheme,
  presentCamera,
  presentedFrame,
  until,
  type GuiPanelContract,
} from "./gui-panel.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  CAMERA,
  SURFACE,
  selectSystems,
} from "../integration/system-selections.js";

/** Linear RGBA fills of the scrolled content. */
export const SCROLL_COLORS = {
  outer: [0.2, 0.2, 0.2, 1],
  inner: [0.05, 0.05, 0.3, 1],
  first: [0.8, 0.1, 0.1, 1],
  second: [0.1, 0.8, 0.1, 1],
  narrow: [0.1, 0.1, 0.8, 1],
  last: [0.8, 0.8, 0.1, 1],
} as const;

/** Items of the mounted VirtualList: estimate, count and per-index fills,
 * in Canvas logical units (CSS pixels). */
export const VIRTUAL_LIST = {
  count: 100_000,
  estimate: 45,
  /** Even items are 30 units tall, odd items 60. */
  heights: [30, 60],
  /** Fills by index modulo four: red, green, blue, yellow. */
  colors: [
    [0.8, 0.1, 0.1, 1],
    [0.1, 0.8, 0.1, 1],
    [0.1, 0.1, 0.8, 1],
    [0.8, 0.8, 0.1, 1],
  ],
} as const;

let contract: GuiPanelContract | undefined;
let output: OutputReference | undefined;
let root: Root | undefined;
let handle: IppCanvasHandle | undefined;
let attached: CanvasWorldHandle | undefined;
let commits = 0;
let errors: string[] = [];
let ranges: GuiVirtualRange[] = [];
/** Canvas logical units one wheel notch scrolls: a quarter of the 60-unit
 * Surface metre, an eighth of the 2-metre inner viewport. */
const WHEEL_STEP = UNITS_PER_METRE / 4;

let inputTrace: string[] = [];
const listRef: { current: GuiControlHandle | null } = { current: null };
const outerRef: { current: GuiControlHandle | null } = { current: null };
let themes:
  | {
      readonly outer: Uint8Array<ArrayBuffer>;
      readonly inner: Uint8Array<ArrayBuffer>;
    }
  | undefined;

/** Opaque flat scroll bar skins so frames classify bar columns by colour:
 * the outer view has a cyan track and a magenta thumb that turns white while
 * pressed; the inner view a blue track and a yellow thumb. Backgrounds are
 * the ScrollView fills. Every part states away the default look's lines,
 * corner cuts and glow. */
function scrollThemes(current: GuiPanelContract) {
  const plain = {
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
    glow_intensity: 0,
  };
  return {
    outer: encodeTheme(current, [
      [
        "background",
        undefined,
        { color: SCROLL_COLORS.outer, opacity: 1, ...plain },
      ],
      [
        "scrollTrackY",
        undefined,
        { color: [0.1, 0.8, 0.8, 1], opacity: 1, ...plain },
      ],
      [
        "scrollThumbY",
        undefined,
        { color: [0.8, 0.1, 0.8, 1], opacity: 1, ...plain },
      ],
      ["scrollThumbY", "pressed", { color: [0.9, 0.9, 0.9, 1] }],
    ]),
    inner: encodeTheme(current, [
      [
        "background",
        undefined,
        { color: SCROLL_COLORS.inner, opacity: 1, ...plain },
      ],
      [
        "scrollTrackY",
        undefined,
        { color: [0.1, 0.1, 0.8, 1], opacity: 1, ...plain },
      ],
      [
        "scrollThumbY",
        undefined,
        { color: [0.8, 0.8, 0.1, 1], opacity: 1, ...plain },
      ],
    ]),
  };
}

/** Bars as thick as 5% of a viewport's shorter side, flush with the
 * control's right side and ends. */
const flushBar = (thickness: number) =>
  ({ bar_thickness: thickness, bar_inset: 0, bar_end_inset: 0 }) as const;

function Block({
  id,
  width,
  height,
  color,
}: {
  readonly id: string;
  readonly width: number;
  readonly height: number;
  readonly color: readonly number[];
}): ReactElement {
  return (
    <Entity id={id}>
      <Layout width={width} height={height} />
      <Style
        red={color[0]!}
        green={color[1]!}
        blue={color[2]!}
        alpha={color[3]!}
      />
      <Box width={width} height={height} />
    </Entity>
  );
}

/** A 240x180 VirtualList over 100000 items estimated at 45 units whose
 * declared items measure 30 or 60 units, with the opaque scroll bar skin. */
function virtualListPanel(): ReactElement {
  return (
    <Entity id="list">
      <Layout width={WIDTH} height={HEIGHT} />
      <Skin theme="scroll-bars" />
      <VirtualList
        ref={listRef}
        item_count={VIRTUAL_LIST.count}
        item_extent={VIRTUAL_LIST.estimate}
        overscan={1}
        axis={1}
        {...flushBar(9)}
        onRangeChange={(range) => ranges.push(range)}
        renderItem={(index) => {
          const height = VIRTUAL_LIST.heights[index % 2]!;
          const color = VIRTUAL_LIST.colors[index % 4]!;
          return (
            <>
              <Layout width={WIDTH} height={height} />
              <Style
                red={color[0]}
                green={color[1]}
                blue={color[2]}
                alpha={color[3]}
              />
              <Box width={WIDTH} height={height} />
            </>
          );
        }}
      />
    </Entity>
  );
}

/**
 * An outer 240x180 ScrollView over 300 units of content: an inner 240x120
 * ScrollView over 180 units (a 120-unit red block then a 60-unit green
 * block), a narrow 120x120 blue block at the left and a 60-unit yellow
 * block. The outer view can scroll by 120 and the inner view by 60. Track
 * thickness is 5% of the shorter viewport side: the outer vertical bar spans
 * x 231..240, and its thumb, 102.6 units long, travels 68.4 units between
 * the track's 4.5-unit pointed ends over that capacity. The inner 6-unit bar
 * would share that edge, so it ends at the outer track's inner edge: x
 * 225..231.
 */
function nestedScrollPanel(): ReactElement {
  return (
    <Entity id="outer">
      <Layout width={WIDTH} height={HEIGHT} />
      <Skin theme="scroll-bars" />
      <ScrollView ref={outerRef} axis={1} {...flushBar(9)} />
      <Children>
        <Entity id="outer-content">
          <Layout kind={2} width={WIDTH} />
          <Children>
            <Entity id="inner">
              <Layout width={WIDTH} height={120} />
              <Skin theme="inner-scroll-bars" />
              <ScrollView axis={1} {...flushBar(6)} />
              <Children>
                <Entity id="inner-content">
                  <Layout kind={2} width={WIDTH} />
                  <Children>
                    <Block
                      id="first"
                      width={WIDTH}
                      height={120}
                      color={SCROLL_COLORS.first}
                    />
                    <Block
                      id="second"
                      width={WIDTH}
                      height={60}
                      color={SCROLL_COLORS.second}
                    />
                  </Children>
                </Entity>
              </Children>
            </Entity>
            <Block
              id="narrow"
              width={120}
              height={120}
              color={SCROLL_COLORS.narrow}
            />
            <Block
              id="last"
              width={WIDTH}
              height={60}
              color={SCROLL_COLORS.last}
            />
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}

function Application({
  runtime,
  panel,
}: {
  readonly runtime: CanvasRuntimeConfiguration;
  readonly panel: () => ReactElement;
}): ReactElement {
  const [presented, setPresented] = useState<OutputReference | null>(null);
  const initialize = useCallback(
    async (
      client: Client,
      _signal: AbortSignal,
      host: IppCanvasHandle["host"],
    ) => {
      // Physical input settlements, kept as failure context.
      const open = host.input.open.bind(host.input);
      host.input.open = async (...args) => {
        const context = await open(...args);
        const send = context.send.bind(context);
        context.send = async (input) => {
          const label = `${input.kind}${"point" in input ? `@${input.point.map((value) => Math.round(value * 1000) / 1000)}` : ""}`;
          try {
            const outcome = await send(input);
            inputTrace.push(
              `${label}:${outcome.disposition}:${outcome.applied}/${outcome.rejected}/${outcome.cancelled}${outcome.error ? `:${outcome.error}` : ""}`,
            );
            return outcome;
          } catch (error) {
            inputTrace.push(`${label}:threw:${String(error)}`);
            throw error;
          } finally {
            if (inputTrace.length > 60) inputTrace.shift();
          }
        };
        return context;
      };
      output = await presentCamera(client, host, "scroll-camera");
    },
    [],
  );
  const ready = useCallback((next: IppCanvasHandle) => {
    handle = next;
    setPresented(output ?? null);
  }, []);
  const encoded = themes;
  return (
    <IppCanvas
      runtime={runtime}
      world={{
        create: {
          selectedSystems: selectSystems(
            ATTACHMENTS,
            CAMERA,
            SURFACE,
            LIFECYCLE,
          ),
        },
      }}
      output={presented}
      initialize={initialize}
      width={WIDTH}
      height={HEIGHT}
      canvasProps={{ id: "scroll-gui-canvas" }}
      guiInput={{ wheelStep: WHEEL_STEP }}
      onReady={ready}
      onError={(error) => errors.push(error.message)}
    >
      {presented && encoded ? (
        <World
          onCommit={() => {
            commits += 1;
          }}
          onError={(error) => errors.push(error.message)}
        >
          <Entity id="scroll-panel">
            <Transform />
            <Surface
              width={WIDTH / UNITS_PER_METRE}
              height={HEIGHT / UNITS_PER_METRE}
            />
          </Entity>
          <CanvasWorld
            presentation={{ anchor: "scroll-panel" }}
            create={{
              symbolicId: "scroll-panel-world",
              selectedSystems: PANEL_SYSTEMS,
            }}
            extent={[WIDTH, HEIGHT]}
            unitsPerMetre={UNITS_PER_METRE}
            onReady={(next) => {
              attached = next;
            }}
            onError={(error) => errors.push(error.message)}
          >
            <Entity id="scroll-bars">
              <Theme parts={encoded.outer} />
            </Entity>
            <Entity id="inner-scroll-bars">
              <Theme parts={encoded.inner} />
            </Entity>
            <Entity id="canvas">
              <Layout kind={2} width={WIDTH} height={HEIGHT} />
              <Children>{panel()}</Children>
            </Entity>
          </CanvasWorld>
        </World>
      ) : null}
    </IppCanvas>
  );
}

/** Mount the nested ScrollView panel and wait for its first commit. */
export async function mountScrollCanvas(
  runtime: CanvasRuntimeConfiguration,
): Promise<void> {
  await mount(runtime, nestedScrollPanel, false);
}

/** Mount the VirtualList panel and wait for its first commit. */
export async function mountVirtualListCanvas(
  runtime: CanvasRuntimeConfiguration,
): Promise<void> {
  await mount(runtime, virtualListPanel, true);
}

/** Wanted ranges the mounted VirtualList observed, oldest first. */
export function virtualListRanges(): readonly GuiVirtualRange[] {
  return [...ranges];
}

/** The mounted VirtualList's anchor and range fields. */
export async function virtualListSemantics(): Promise<{
  readonly offset: number;
  readonly anchorIndex: number;
  readonly anchorOffset: number;
  readonly first: number;
  readonly last: number;
}> {
  const list = listRef.current;
  if (!list) throw new Error("VirtualList ref is not acknowledged");
  const fields = await list.read();
  if (typeof fields.offset_y !== "number")
    throw new Error("VirtualList has no scroll fields");
  return {
    offset: fields.offset_y,
    anchorIndex: Number(fields.anchor_index),
    anchorOffset: Number(fields.anchor_offset),
    first: Number(fields.range_first),
    last: Number(fields.range_last),
  };
}

async function mount(
  runtime: CanvasRuntimeConfiguration,
  panel: () => ReactElement,
  list: boolean,
): Promise<void> {
  await closeScrollCanvas();
  contract = (await import(runtime.generatedModuleUrl)) as GuiPanelContract;
  themes = scrollThemes(contract);
  output = undefined;
  attached = undefined;
  commits = 0;
  errors = [];
  ranges = [];
  inputTrace = [];
  listRef.current = null;
  outerRef.current = null;
  const host = document.createElement("div");
  host.id = "scroll-gui-host";
  document.body.replaceChildren(host);
  root = createRoot(host);
  root.render(<Application runtime={runtime} panel={panel} />);
  await until(
    () =>
      handle !== undefined &&
      commits > 0 &&
      attached !== undefined &&
      (!list || (listRef.current !== null && ranges.length > 0)),
    () => `mounted scroll panel did not acknowledge: ${errors.join("; ")}`,
  );
  await handle!.flush();
  await handle!.frame();
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
  const frame = await presentedFrame(current);
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d");
  if (context === null) throw new Error("2D evidence canvas is unavailable");
  context.putImageData(
    new ImageData(
      new Uint8ClampedArray(frame.pixels),
      frame.width,
      frame.height,
    ),
    0,
    0,
  );
  return {
    width: frame.width,
    height: frame.height,
    failedDrawCalls: frame.failedDrawCalls,
    samples: points.map(([x, y]) =>
      averageRgb(frame.pixels, frame.width, frame.height, x, y),
    ),
    dataUrl: canvas.toDataURL("image/png"),
  };
}

/** Scroll fields of the mounted scroll controls, kept as failure context. */
export async function scrollDiagnostics(): Promise<string> {
  const entries = await Promise.all(
    Object.entries({ outer: outerRef, list: listRef }).map(
      async ([name, ref]) =>
        [name, await ref.current?.read().catch(String)] as const,
    ),
  );
  return JSON.stringify(
    {
      controls: Object.fromEntries(entries),
      errors,
      inputTrace,
    },
    (_, value: unknown) => (typeof value === "bigint" ? `${value}` : value),
  );
}

/** Fixture errors reported by the canvas or World. */
export function scrollErrors(): readonly string[] {
  return [...errors];
}

/** Unmount and wait for the canvas session to close. */
export async function closeScrollCanvas(): Promise<number> {
  const closing = handle;
  handle = undefined;
  attached = undefined;
  const activeRoot = root;
  root = undefined;
  activeRoot?.unmount();
  if (closing !== undefined) await closing.closed;
  return document.querySelectorAll("canvas").length;
}
