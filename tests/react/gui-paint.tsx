import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type PresentedCapture,
  type Command,
} from "@ipp/client";
import { createRef } from "react";
import {
  Entity,
  Children,
  Asset,
  CanvasWorld,
  assetRef,
  assetField,
  type CanvasWorldHandle,
} from "@ipp/react";
import {
  Style,
  Layout,
  Theme,
  Skin,
  Button,
  Checkbox,
  Text,
  Drawing,
  Image,
  Font,
  Box,
  VirtualList,
  type GuiControlHandle,
} from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import { check, deferred, type GuiContract } from "./gui-authoring.js";
export { nativePresentationTransport } from "../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../packages/ipp-client/src/worker.js";
export { guiLayerTransitions } from "./gui-layer-transitions.js";
export { guiLayers } from "./gui-layers.js";
export { guiOverlays } from "./gui-overlays.js";
export { guiProjectedAdvanced } from "./gui-projected-advanced.js";
export { guiProjectedSurfaces } from "./gui-projected-surfaces.js";

export interface GuiPaintAssets {
  font: Uint8Array<ArrayBuffer>;
  drawing: Uint8Array<ArrayBuffer>;
  bitmap: Uint8Array<ArrayBuffer>;
}

function encoded(bytes: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  return bytes;
}

/** One generated GuiTheme part row. */
type PaintRow =
  Parameters<
    GuiContract["GuiTheme"]["encodeParts"]
  >[0]["rows"] extends ReadonlyMap<number, infer Row>
    ? Row
    : never;

/** Device pixels per logical unit of the primitive-shape capture. */
const SHAPE_DPR = 3;

/** Device pixels per logical unit of the colour-fill capture. */
const COLOUR_DPR = 2;

export async function guiPaint(
  host: HostClientBase<Client>,
  contract: GuiContract,
  assets: GuiPaintAssets,
) {
  const world = (
    await host.createWorld({
      selectedSystems: [
        "ipp.animation",
        "ipp.gui",
        "ipp.gui-layout",
        "ipp.canvas",
        "ipp.asset-dependencies",
        "ipp.lifecycle-publisher",
      ],
    })
  ).reference;
  const client = await host.openWorld(world);
  const writes: Command[][] = [];
  const batch = client.batch.bind(client);
  client.batch = async (commands) => {
    writes.push(commands);
    return batch(commands);
  };
  const errors: Error[] = [];
  const session = new CanvasWorldSession({
    host,
    client,
    onError: (error) => errors.push(error),
  });
  const root = session.createRoot();
  const press = createRef<GuiControlHandle>();
  const callbackPaint = deferred<Promise<void>>();
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const part = contract.guiPaintPartIndex({ part: "background" });
  // Plain boxes: the rows state away the default button look's corner cuts,
  // which they would otherwise sit on.
  const square = [0, 0, 0, 0] as const;
  const skin = contract.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([
      [
        0,
        {
          part,
          color: [0, 0, 1, 1],
          corner_radius: [0, 0],
          border_width: 0,
          corner_cut: square,
        },
      ],
    ]),
  });
  // The bitmap background and a white label, instead of the default look's
  // accent label.
  const labelParts = contract.GuiSkin.encodeParts({
    nextSlot: 7,
    rows: new Map([
      [5, { part, color: [1, 1, 1, 1] }],
      [
        6,
        {
          part: contract.guiPaintPartIndex({ part: "label" }),
          color: [1, 1, 1, 1],
        },
      ],
    ]),
  });
  function scene(width: number, green: boolean) {
    const theme = contract.GuiTheme.encodeParts({
      nextSlot: 2,
      rows: new Map([
        [
          1,
          {
            part,
            color: green ? [0, 1, 0, 1] : [1, 0, 0, 1],
            corner_radius: [0, 0],
            border_width: 0,
            corner_cut: square,
          },
        ],
      ]),
    });
    return (
      <>
        <Asset id="font" kind={17} data={assets.font} encode={encoded} />
        <Asset id="drawing" kind={18} data={assets.drawing} encode={encoded} />
        <Asset id="bitmap" kind={2} data={assets.bitmap} encode={encoded} />
        <Entity id="theme">
          <Theme parts={theme} />
        </Entity>
        <Entity id="canvas">
          <Layout
            kind={3}
            width={width}
            height={64}
            align_x={-1}
            align_y={-1}
          />
          <Children>
            <Entity id="background">
              <Layout width={width} height={64} align_x={-1} align_y={-1} />
              <Style red={0} green={0} blue={0} />
              <Box width={width} height={64} />
            </Entity>
            <Entity id="clip">
              <Layout
                kind={3}
                width={64}
                height={24}
                align_x={-1}
                align_y={-1}
              />
              <Style
                clipped
                clip_min_x={0}
                clip_min_y={0}
                clip_max_x={48}
                clip_max_y={24}
              />
              <Children>
                <Entity id="red">
                  <Layout width={32} height={24} align_x={-1} align_y={-1} />
                  <Skin theme="theme" />
                  <Button
                    label=""
                    ref={press}
                    onPress={() =>
                      callbackPaint.resolve(root.render(scene(width, true)))
                    }
                  />
                </Entity>
                <Entity id="blue">
                  <Layout width={32} height={24} align_x={-1} align_y={-1} />
                  <Style x={32} />
                  <Skin theme="theme" parts={skin} />
                  <Button label="" />
                </Entity>
              </Children>
            </Entity>
            <Entity id="text">
              <Layout width={24} height={20} align_x={-1} align_y={-1} />
              <Style y={32} />
              <Text text="M" source={assetRef("font")} font_size={18} />
            </Entity>
            <Entity id="label">
              <Layout width={24} height={20} align_x={-1} align_y={-1} />
              <Style x={24} y={32} />
              <Font source={assetRef("font")} font_size={12} />
              <Skin parts={labelParts} />
              <Skin
                fields={[
                  assetField(
                    contract.GuiSkin.partsOffset(5, "asset"),
                    assetRef("bitmap"),
                  ),
                ]}
              />
              <Button label="A" />
            </Entity>
            <Entity id="drawing">
              <Layout width={16} height={16} align_x={-1} align_y={-1} />
              <Style x={50} y={32} scale_x={0.5} scale_y={0.5} />
              <Drawing source={assetRef("drawing")} />
            </Entity>
            <Entity id="bitmap">
              <Layout width={16} height={16} align_x={-1} align_y={-1} />
              <Style x={72} y={36} />
              <Image source={assetRef("bitmap")} width={16} height={16} />
            </Entity>
          </Children>
        </Entity>
      </>
    );
  }
  function region(
    frame: PresentedCapture,
    bounds: readonly [number, number, number, number],
    expected: readonly [number, number, number],
  ): boolean {
    const width = frame.view.binding.viewport.width;
    const pixels = new Uint8Array(frame.pixels);
    for (let row = bounds[1]; row < bounds[3]; row++) {
      for (let column = bounds[0]; column < bounds[2]; column++) {
        const offset = (row * width + column) * 4;
        if (
          expected.some(
            (value, channel) => Math.abs(pixels[offset + channel]! - value) > 8,
          )
        )
          return false;
      }
    }
    return true;
  }
  async function capture(label: string, width: number, green: boolean) {
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const matches =
        frame.view.binding.viewport.width === width &&
        region(frame, [4, 4, 28, 20], green ? [0, 255, 0] : [255, 0, 0]) &&
        region(frame, [36, 4, 44, 20], [0, 0, 255]) &&
        region(frame, [52, 4, width - 4, 20], [0, 0, 0]) &&
        coloredPixels(
          frame,
          [0, 32, 22, 54],
          (red, green, blue) => red > 160 && green > 160 && blue > 160,
        ) > 8 &&
        coloredPixels(
          frame,
          [24, 32, 48, 54],
          (red, green, blue) => red > 160 && green > 160 && blue > 160,
        ) > 3 &&
        coloredPixels(
          frame,
          [50, 32, 66, 46],
          (red, green, blue) => red > 150 && green > 100 && blue < 130,
        ) > 2 &&
        coloredPixels(
          frame,
          [74, 38, 86, 50],
          (red, green, blue) => red > 120 && green > 60 && blue < 100,
        ) > 30;
      if (matches) {
        images.push({
          label,
          width,
          height: 64,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        return frame;
      }
      if (performance.now() >= deadline) {
        images.push({
          label: `failed-${label}`,
          width,
          height: 64,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        throw new Error(
          `Independent GUI paint regions failed: ${label}; samples=${[8, 40, 52].map((column) => Array.from(new Uint8Array(frame.pixels).slice((8 * width + column) * 4, (8 * width + column) * 4 + 4)).join(",")).join(";")}`,
        );
      }
      sequence = frame.sequence;
    }
  }
  /**
   * Box primitive vocabulary at three device pixels per logical unit, asserted on
   * captured pixels: 45-degree cuts with a glow following the cut, an inner glow
   * over a transparent fill, a two-segment check-mark stroke, a thin frame with
   * corner accents, accents alone, and degenerate cuts making a pointed bar and a
   * triangle. Expectations follow from the authored rows: the black page, the
   * cyan border and stroke, the dark panel fill and a partial-strength glow.
   */
  async function primitiveShapes() {
    const background = contract.guiPaintPartIndex({ part: "background" });
    const icon = contract.guiPaintPartIndex({ part: "icon" });
    const cyan = [0, 0.9, 1, 1] as const;
    const clear = [0, 0, 0, 0] as const;
    const panel = [0, 0.02, 0.035, 1] as const;
    const themeOf = (...rows: PaintRow[]) =>
      contract.GuiTheme.encodeParts({
        nextSlot: rows.length,
        rows: new Map(rows.map((row, slot) => [slot, row])),
      });
    const themes = {
      cut: themeOf({
        part: background,
        color: panel,
        border_width: 2,
        border_color: cyan,
        corner_cut: [12, 0, 12, 0],
        glow_color: cyan,
        glow_intensity: 0.9,
        glow_radius: 4,
        glow_falloff: 1.5,
      }),
      inner: themeOf({
        part: background,
        color: clear,
        corner_cut: [0, 0, 0, 0],
        border_width: 1.5,
        border_color: cyan,
        glow_color: cyan,
        glow_intensity: 1,
        glow_inner_radius: 8,
      }),
      bar: themeOf({ part: background, color: cyan, corner_cut: [6, 6, 6, 6] }),
      triangle: themeOf({
        part: background,
        color: cyan,
        corner_cut: [8, 8, 0, 0],
      }),
      frame: themeOf({
        part: background,
        color: clear,
        corner_cut: [0, 0, 0, 0],
        border_width: 1,
        border_color: cyan,
        corner_accent: [12, 12, 12, 12],
        corner_accent_width: 4,
      }),
      brackets: themeOf({
        part: background,
        color: clear,
        corner_cut: [0, 0, 0, 0],
        border_width: 0,
        border_color: cyan,
        corner_accent: [12, 12, 12, 12],
        corner_accent_width: 3,
      }),
      check: themeOf(
        {
          part: background,
          color: panel,
          border_width: 1,
          border_color: cyan,
          corner_cut: [0, 0, 0, 0],
        },
        {
          part: icon,
          color: cyan,
          shape: 1,
          border_width: 3,
          // Each segment runs half the 3-unit thickness (0.075 of the 20-unit
          // indicator) past the joint at (0.4, 0.8), so their butt ends close
          // its outer corner.
          stroke_a: [0.15, 0.55, 0.453, 0.853],
          stroke_b: [0.3525, 0.858, 0.85, 0.25],
        },
      ),
    };
    const buttons = [
      ["cut", [8, 8, 72, 32]],
      ["inner", [88, 8, 72, 32]],
      ["bar", [168, 8, 64, 12]],
      ["triangle", [168, 28, 16, 8]],
      ["frame", [56, 56, 80, 40]],
      ["brackets", [144, 56, 80, 40]],
    ] as const;
    await root.render(
      <>
        {Object.entries(themes).map(([name, parts]) => (
          <Entity key={name} id={`theme-${name}`}>
            <Theme parts={parts} />
          </Entity>
        ))}
        <Entity id="shapes">
          <Layout kind={3} width={240} height={112} align_x={-1} align_y={-1} />
          <Children>
            <Entity id="shapes-page">
              <Layout width={240} height={112} align_x={-1} align_y={-1} />
              <Style red={0} green={0} blue={0} />
              <Box width={240} height={112} />
            </Entity>
            {buttons.map(([name, [x, y, width, height]]) => (
              <Entity key={name} id={`shape-${name}`}>
                <Layout
                  width={width}
                  height={height}
                  align_x={-1}
                  align_y={-1}
                />
                <Style x={x} y={y} />
                <Skin theme={`theme-${name}`} />
                <Button label="" />
              </Entity>
            ))}
            <Entity id="shape-check">
              <Layout width={40} height={40} align_x={-1} align_y={-1} />
              <Style x={8} y={56} />
              <Skin theme="theme-check" />
              <Checkbox label="" checked />
            </Entity>
          </Children>
        </Entity>
      </>,
    );
    // The size is in CSS pixels; the drawing buffer is SHAPE_DPR times larger.
    await session.selectOutput(canvasOutput(world), {
      width: 240,
      height: 112,
      devicePixelRatio: SHAPE_DPR,
    });

    type Rgb = readonly [number, number, number];
    const page = ([red, green, blue]: Rgb) =>
      red < 24 && green < 24 && blue < 24;
    const line = ([red, green, blue]: Rgb) =>
      red < 70 && green > 170 && blue > 170;
    const fill = ([red, green, blue]: Rgb) =>
      red < 24 && green > 24 && green < 64 && blue > 32 && blue < 80;
    const glow = ([red, green, blue]: Rgb) =>
      red < 40 && green > 60 && green < 220 && blue > 60;
    // [label, logical x, logical y, expected]; the cut box spans (8, 8)-(80, 40)
    // with 12-unit cuts at its top-left and bottom-right corners, and the check
    // mark's indicator spans (18, 66)-(38, 86).
    const probes: [string, number, number, (pixel: Rgb) => boolean][] = [
      ["page inside a cut corner", 8.2, 8.2, page],
      ["page inside the opposite cut corner", 79.8, 39.8, page],
      ["border along the cut", 14.71, 14.71, line],
      ["glow beyond the cut", 12.59, 12.59, glow],
      ["panel fill", 44, 24, fill],
      ["inner glow over a transparent fill", 124, 12, glow],
      ["inner glow ends before the centre", 124, 24, page],
      ["border over the inner glow", 124, 8.5, line],
      ["pointed bar end removed", 168.4, 8.4, page],
      ["pointed bar body near its point", 171, 14, line],
      ["triangle corner removed", 168.4, 28.4, page],
      ["triangle body", 176, 34, line],
      ["first stroke segment", 23.5, 79.5, line],
      ["second stroke segment", 30.5, 76.5, line],
      ["inside the stroke joint", 26, 81, line],
      ["outside corner of the stroke joint", 26, 82.9, line],
      ["no stroke above the mark", 20, 68, fill],
      ["no stroke below the mark", 35, 83, fill],
      ["thick border inside an accent span", 62, 59, line],
      ["thick left border inside an accent span", 59, 62, line],
      ["thin border between spans", 96, 56.5, line],
      ["no thick border between spans", 96, 59, page],
      ["bracket", 150, 57.5, line],
      ["nothing between brackets on the top edge", 184, 56.5, page],
      ["nothing between brackets on the left edge", 144.5, 76, page],
    ];
    const sample = (frame: PresentedCapture, x: number, y: number): Rgb => {
      const width = frame.view.binding.viewport.width;
      const offset =
        (Math.floor(y * SHAPE_DPR) * width + Math.floor(x * SHAPE_DPR)) * 4;
      const pixels = new Uint8Array(frame.pixels);
      return [pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!];
    };
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const sized = frame.view.binding.viewport.width === 240 * SHAPE_DPR;
      const failed = sized
        ? probes.filter(([, x, y, expected]) => !expected(sample(frame, x, y)))
        : probes;
      if (failed.length === 0 || performance.now() >= deadline) {
        images.push({
          label:
            failed.length === 0
              ? "primitive-shapes"
              : "failed-primitive-shapes",
          width: frame.view.binding.viewport.width,
          height: frame.view.binding.viewport.height,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(
          failed.length === 0,
          `Primitive shape pixels failed: ${failed.map(([label, x, y]) => `${label} ${sized ? sample(frame, x, y).join(",") : "unsized"}`).join("; ")}`,
        );
        return;
      }
      sequence = frame.sequence;
    }
  }
  /**
   * Skinned entities that are not controls, at SHAPE_DPR device pixels per logical
   * unit: a page styled by skin rows alone, and a padded column skinned by a shared
   * theme as a thin frame with thick corner accents over a dark fill. The column
   * holds a plain red box, a separator styled by its own rows and a highlight whose
   * Box the skin paints instead of the plain white box. Expectations follow from
   * the authored rows and the logical layout below.
   */
  async function skinnedPanel() {
    const background = contract.guiPaintPartIndex({ part: "background" });
    const rowsOf = (...rows: PaintRow[]) => ({
      nextSlot: rows.length,
      rows: new Map(rows.map((row, slot) => [slot, row])),
    });
    const frameTheme = contract.GuiTheme.encodeParts(
      rowsOf({
        part: background,
        color: [0, 0.02, 0.035, 1],
        border_width: 1,
        border_color: [0, 0.9, 1, 1],
        corner_accent: [14, 14, 14, 14],
        corner_accent_width: 3,
      }),
    );
    const skinRows = (color: readonly [number, number, number, number]) =>
      contract.GuiSkin.encodeParts(rowsOf({ part: background, color }));
    // The frame spans (16, 16)-(144, 80); its 8-unit padding places the 112-unit
    // children at x 24: the red box over y 24-40, the separator over y 46-48 and
    // the highlight over y 54-66.
    await root.render(
      <>
        <Entity id="theme-panel-frame">
          <Theme parts={frameTheme} />
        </Entity>
        <Entity id="skinned-page">
          <Layout kind={3} width={160} height={96} align_x={-1} align_y={-1} />
          <Skin parts={skinRows([0, 0, 0, 1])} />
          <Children>
            <Entity id="skinned-frame">
              <Layout
                kind={2}
                width={128}
                height={64}
                padding_top={8}
                padding_right={8}
                padding_bottom={8}
                padding_left={8}
                align_x={-1}
                align_y={-1}
              />
              <Style x={16} y={16} />
              <Skin theme="theme-panel-frame" />
              <Children>
                <Entity id="skinned-frame-child">
                  <Layout width={112} height={16} />
                  <Style red={1} green={0} blue={0} />
                  <Box width={112} height={16} />
                </Entity>
                <Entity id="skinned-separator">
                  <Layout width={112} height={2} margin_top={6} />
                  <Skin parts={skinRows([0.99, 0.89, 0.24, 1])} />
                </Entity>
                <Entity id="skinned-highlight">
                  <Layout width={112} height={12} margin_top={6} />
                  <Box width={112} height={12} />
                  <Skin parts={skinRows([0.03, 0.12, 0.16, 1])} />
                </Entity>
              </Children>
            </Entity>
          </Children>
        </Entity>
      </>,
    );
    await session.selectOutput(canvasOutput(world), {
      width: 160,
      height: 96,
      devicePixelRatio: SHAPE_DPR,
    });

    type Rgb = readonly [number, number, number];
    const page = ([red, green, blue]: Rgb) =>
      red < 24 && green < 24 && blue < 24;
    const line = ([red, green, blue]: Rgb) =>
      red < 70 && green > 170 && blue > 170;
    const fill = ([red, green, blue]: Rgb) =>
      red < 24 && green > 24 && green < 64 && blue > 32 && blue < 80;
    const child = ([red, green, blue]: Rgb) =>
      red > 200 && green < 40 && blue < 40;
    const amber = ([red, green, blue]: Rgb) =>
      red > 220 && green > 200 && blue > 90 && blue < 180;
    const highlight = ([red, green, blue]: Rgb) =>
      red < 70 && green > 80 && green < 120 && blue > 95 && blue < 135;
    const probes: [string, number, number, (pixel: Rgb) => boolean][] = [
      ["page styled by skin rows alone", 8, 8, page],
      ["thin frame border between accents", 80, 16.5, line],
      ["frame fill inside the thin border", 80, 18, fill],
      ["thin left border between accents", 16.5, 50, line],
      ["thick top-left accent", 24, 18, line],
      ["thick left accent", 18, 24, line],
      ["thick bottom-right accent", 136, 78.5, line],
      ["child painted over the frame fill", 80, 32, child],
      ["frame fill between child and separator", 80, 43, fill],
      ["separator", 80, 47, amber],
      ["skinned box instead of the plain box", 80, 60, highlight],
      ["frame fill beside the padding", 20, 60, fill],
    ];
    const sample = (frame: PresentedCapture, x: number, y: number): Rgb => {
      const width = frame.view.binding.viewport.width;
      const offset =
        (Math.floor(y * SHAPE_DPR) * width + Math.floor(x * SHAPE_DPR)) * 4;
      const pixels = new Uint8Array(frame.pixels);
      return [pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!];
    };
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const sized = frame.view.binding.viewport.width === 160 * SHAPE_DPR;
      const failed = sized
        ? probes.filter(([, x, y, expected]) => !expected(sample(frame, x, y)))
        : probes;
      if (failed.length === 0 || performance.now() >= deadline) {
        images.push({
          label: failed.length === 0 ? "skinned-panel" : "failed-skinned-panel",
          width: frame.view.binding.viewport.width,
          height: frame.view.binding.viewport.height,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(
          failed.length === 0,
          `Skinned panel pixels failed: ${failed.map(([label, x, y]) => `${label} ${sized ? sample(frame, x, y).join(",") : "unsized"}`).join("; ")}`,
        );
        return;
      }
      sequence = frame.sequence;
    }
  }
  /**
   * Ring arcs at SHAPE_DPR device pixels per logical unit, painted by skin rows
   * on entities that are not controls: a knob's tick ring, 270-degree track and
   * value arc; a progress ring's whole track under its value from twelve
   * o'clock; a thin spinner arc across twelve o'clock over its track; a glowing
   * arc; and a zero sweep. Angles are turns clockwise from twelve o'clock, and
   * each ring's outer edge is the circle inscribed in its square. Expectations
   * follow from the authored rows and this layout.
   */
  async function arcShapes() {
    const background = contract.guiPaintPartIndex({ part: "background" });
    const cyan = [0, 0.9, 1, 1] as const;
    // The design language's quiet line and neutral, as linear RGBA.
    const line = [0.036, 0.107, 0.162, 1] as const;
    const neutral = [0.279, 0.434, 0.552, 1] as const;
    type Arc = {
      color: readonly [number, number, number, number];
      width: number;
      start?: number;
      sweep?: number;
      dashes?: readonly [number, number];
      glow?: number;
    };
    const arcRows = (arc: Arc) =>
      contract.GuiSkin.encodeParts({
        nextSlot: 1,
        rows: new Map([
          [
            0,
            {
              part: background,
              color: arc.color,
              border_width: arc.width,
              shape: 2,
              ...(arc.start === undefined ? {} : { arc_start: arc.start }),
              ...(arc.sweep === undefined ? {} : { arc_sweep: arc.sweep }),
              ...(arc.dashes === undefined ? {} : { arc_dashes: arc.dashes }),
              ...(arc.glow === undefined
                ? {}
                : {
                    glow_color: cyan,
                    glow_intensity: 1,
                    glow_radius: arc.glow,
                    glow_falloff: 1,
                  }),
            },
          ],
        ]),
      });
    // [id, square [x, y, size], arc], in painter order.
    const knobValue = 0.65 * 0.75;
    const arcs: [string, readonly [number, number, number], Arc][] = [
      // 37 ticks centred on 48 cells a turn from half past seven: the arc runs
      // half a cell further at both ends.
      [
        "knob-ticks",
        [8, 8, 64],
        {
          color: neutral,
          width: 4,
          start: 0.625 - 1 / 96,
          sweep: 37 / 48,
          dashes: [48, 0.25],
        },
      ],
      [
        "knob-track",
        [14, 14, 52],
        { color: line, width: 2, start: 0.625, sweep: 0.75 },
      ],
      [
        "knob-value",
        [14, 14, 52],
        { color: cyan, width: 4, start: 0.625, sweep: knobValue },
      ],
      ["progress-track", [88, 8, 64], { color: line, width: 6, start: 0.8 }],
      ["progress-value", [88, 8, 64], { color: cyan, width: 6, sweep: 0.65 }],
      ["spinner-track", [184, 24, 32], { color: line, width: 2.5 }],
      [
        "spinner",
        [184, 24, 32],
        { color: cyan, width: 2.5, start: 0.9, sweep: 0.25 },
      ],
      [
        "glowing",
        [180, 70, 40],
        { color: cyan, width: 3, start: 0.8, sweep: 0.4, glow: 6 },
      ],
      ["empty", [100, 80, 32], { color: cyan, width: 4, start: 0.4, sweep: 0 }],
    ];
    await root.render(
      <Entity id="arcs">
        <Layout kind={3} width={240} height={120} align_x={-1} align_y={-1} />
        <Children>
          <Entity id="arcs-page">
            <Layout width={240} height={120} align_x={-1} align_y={-1} />
            <Style red={0} green={0} blue={0} />
            <Box width={240} height={120} />
          </Entity>
          {arcs.map(([id, [x, y, size], arc]) => (
            <Entity key={id} id={`arc-${id}`}>
              <Layout width={size} height={size} align_x={-1} align_y={-1} />
              <Style x={x} y={y} />
              <Skin parts={arcRows(arc)} />
            </Entity>
          ))}
        </Children>
      </Entity>,
    );
    await session.selectOutput(canvasOutput(world), {
      width: 240,
      height: 120,
      devicePixelRatio: SHAPE_DPR,
    });

    type Rgb = readonly [number, number, number];
    const page = ([red, green, blue]: Rgb) =>
      red < 24 && green < 24 && blue < 24;
    const lit = ([red, green, blue]: Rgb) =>
      red < 70 && green > 200 && blue > 220;
    const track = ([red, green, blue]: Rgb) =>
      red > 30 &&
      red < 80 &&
      green > 70 &&
      green < 115 &&
      blue > 90 &&
      blue < 135;
    const tick = ([red, green, blue]: Rgb) =>
      red > 120 && red < 170 && green > 155 && green < 200 && blue > 175;
    const glow = ([red, green, blue]: Rgb) =>
      red < 40 && green > 60 && green < 220 && blue > 60 && blue < 235;
    /** The point `radius` out from `center` at `turns` clockwise from twelve. */
    const at = (
      center: readonly [number, number],
      radius: number,
      turns: number,
    ): [number, number] => [
      center[0] + radius * Math.sin(turns * 2 * Math.PI),
      center[1] - radius * Math.cos(turns * 2 * Math.PI),
    ];
    const knob = [40, 40] as const;
    const progress = [120, 40] as const;
    const spinner = [200, 40] as const;
    const glowing = [200, 90] as const;
    const knobEnd = 0.625 + knobValue;
    const probes: [string, [number, number], (pixel: Rgb) => boolean][] = [
      // The knob's value band is 22-26 out, its track 24-26, its ticks 28-32.
      ["knob value arc body", at(knob, 24, 0.75), lit],
      ["knob track past the value", at(knob, 25, 0.25), track],
      ["inside the value arc's inner radius", at(knob, 21, 0.75), page],
      ["outside the value arc's outer radius", at(knob, 27, 0.75), page],
      ["before the value arc's butt end", at(knob, 23, knobEnd - 0.02), lit],
      ["beyond the value arc's butt end", at(knob, 23, knobEnd + 0.02), page],
      ["beyond the track's start", at(knob, 25, 0.625 - 0.02), page],
      ["between the track's ends", at(knob, 25, 0.5), page],
      ["on a tick", at(knob, 30, 0.75), tick],
      ["in a tick gap", at(knob, 30, 0.75 + 0.5 / 48), page],
      ["on the first tick", at(knob, 30, 0.625), tick],
      ["on the last tick", at(knob, 30, 1.375), tick],
      ["beyond the last tick", at(knob, 30, 1.375 + 1 / 48), page],
      // The progress ring's band is 26-32 out; its value runs from twelve o'clock.
      ["progress value", at(progress, 29, 0.3), lit],
      ["progress value after twelve o'clock", at(progress, 29, 0.01), lit],
      ["progress track before twelve o'clock", at(progress, 29, 0.99), track],
      ["progress value's butt end", at(progress, 29, 0.64), lit],
      ["progress track past the value", at(progress, 29, 0.66), track],
      ["whole track at its own start", at(progress, 29, 0.8), track],
      ["inside the progress ring", at(progress, 24, 0.3), page],
      ["outside the progress ring", at(progress, 34, 0.3), page],
      // The spinner's band is 13.5-16 out; its arc runs from 0.9 over twelve.
      ["spinner across the wrap", at(spinner, 14.75, 0), lit],
      ["spinner before the wrap", at(spinner, 14.75, 0.92), lit],
      ["spinner after the wrap", at(spinner, 14.75, 0.13), lit],
      ["spinner track beyond the arc", at(spinner, 14.75, 0.2), track],
      ["spinner track before the arc", at(spinner, 14.75, 0.85), track],
      // The glowing arc's band is 17-20 out, from 0.8 over twelve to 0.2.
      ["glowing arc body", at(glowing, 18.5, 0), lit],
      ["glow outside the arc", at(glowing, 22.5, 0), glow],
      ["glow into the hollow", at(glowing, 15, 0), glow],
      [
        "glow beyond the arc's end",
        at(glowing, 18.5, 0.2 + 2.5 / (2 * Math.PI * 18.5)),
        glow,
      ],
      ["no glow beyond its reach", at(glowing, 28, 0.9), page],
      [
        "a zero sweep paints nothing at its start",
        at([116, 96], 14, 0.4),
        page,
      ],
      ["a zero sweep paints nothing beyond it", at([116, 96], 14, 0.45), page],
    ];
    const sample = (frame: PresentedCapture, [x, y]: [number, number]): Rgb => {
      const width = frame.view.binding.viewport.width;
      const offset =
        (Math.floor(y * SHAPE_DPR) * width + Math.floor(x * SHAPE_DPR)) * 4;
      const pixels = new Uint8Array(frame.pixels);
      return [pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!];
    };
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const sized = frame.view.binding.viewport.width === 240 * SHAPE_DPR;
      const failed = sized
        ? probes.filter(
            ([, point, expected]) => !expected(sample(frame, point)),
          )
        : probes;
      if (failed.length === 0 || performance.now() >= deadline) {
        images.push({
          label: failed.length === 0 ? "arcs" : "failed-arcs",
          width: frame.view.binding.viewport.width,
          height: frame.view.binding.viewport.height,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(
          failed.length === 0,
          `Arc pixels failed: ${failed.map(([label, point]) => `${label} ${sized ? sample(frame, point).join(",") : "unsized"}`).join("; ")}`,
        );
        return;
      }
      sequence = frame.sequence;
    }
  }
  /**
   * Colour-model fills at COLOUR_DPR device pixels per logical unit, painted by
   * skin rows on entities that are not controls: saturation-value fields of two
   * hues, the second with a border, cut corners and glow; a vertical hue rail with
   * red at its bottom and a horizontal one with red at its left; an alpha rail from
   * transparent at its bottom to opaque at its top over a checker; a checker of the
   * absent colours under a transparent fill; a checker whose cells are smaller
   * than a pixel; and a translucent swatch over a checker. Expected colours come
   * from the textbook HSV model and the sRGB transfer function evaluated at each
   * sampled pixel's centre, independently of the runtime. Returns the largest
   * error in 8-bit sRGB levels per surface.
   */
  async function colourFills(): Promise<Record<string, number>> {
    const background = contract.guiPaintPartIndex({ part: "background" });
    type Rgb = readonly [number, number, number];
    /** Linear value of an 8-bit sRGB level. */
    const toLinear = (level: number) => {
      const encoded = level / 255;
      return encoded <= 0.04045
        ? encoded / 12.92
        : ((encoded + 0.055) / 1.055) ** 2.4;
    };
    /** 8-bit sRGB level, unrounded, of a linear value. */
    const toLevel = (value: number) =>
      (value <= 0.0031308
        ? 12.92 * value
        : 1.055 * value ** (1 / 2.4) - 0.055) * 255;
    const rgba = (hex: number, alpha = 1) =>
      [
        toLinear((hex >> 16) & 255),
        toLinear((hex >> 8) & 255),
        toLinear(hex & 255),
        alpha,
      ] as const;
    /** The textbook HSV model, by hue sector, in 8-bit sRGB levels. */
    const hsv = (hue: number, saturation: number, value: number): Rgb => {
      const sector = (((hue % 1) + 1) % 1) * 6;
      const index = Math.floor(sector);
      const f = sector - index;
      const p = value * (1 - saturation);
      const q = value * (1 - saturation * f);
      const t = value * (1 - saturation * (1 - f));
      const rgb = [
        [value, t, p],
        [q, value, p],
        [p, value, t],
        [p, q, value],
        [t, p, value],
        [value, p, q],
      ][index % 6]!;
      return [rgb[0]! * 255, rgb[1]! * 255, rgb[2]! * 255];
    };
    /** Linear-light composite of a straight linear colour over another. */
    const over = (
      top: readonly [number, number, number, number],
      under: readonly number[],
    ): Rgb =>
      [0, 1, 2].map((channel) =>
        toLevel(top[channel]! * top[3] + under[channel]! * (1 - top[3])),
      ) as unknown as Rgb;
    const lineColour = rgba(0x355c70);
    const cyan = rgba(0x54f4ff);
    const orange = rgba(0xff4000, 0.5);
    const checkerA = rgba(0x808080);
    const checkerB = rgba(0x404040);
    type Row = Omit<PaintRow, "part">;
    // [id, [x, y, width, height], row], in painter order.
    const surfaces: [string, readonly [number, number, number, number], Row][] =
      [
        ["field", [8, 8, 80, 80], { fill_mode: 4, fill_hue: 0.55 }],
        [
          "framed-field",
          [96, 8, 80, 80],
          {
            fill_mode: 4,
            fill_hue: 0.08,
            border_width: 1.5,
            border_color: lineColour,
            corner_cut: [8, 0, 8, 0],
            glow_color: [0, 0.9, 1, 1],
            glow_intensity: 1,
            glow_radius: 4,
            glow_falloff: 1,
          },
        ],
        [
          "hue-rail",
          [184, 8, 16, 80],
          { fill_mode: 3, gradient_start: [0, 80], gradient_end: [0, 0] },
        ],
        [
          "alpha-rail",
          [208, 8, 16, 80],
          {
            fill_mode: 1,
            gradient_start: [0, 80],
            gradient_end: [0, 0],
            gradient_color0: [cyan[0], cyan[1], cyan[2], 0],
            gradient_color1: cyan,
            checker_size: 4,
            checker_color0: checkerA,
            checker_color1: checkerB,
          },
        ],
        [
          "hue-row",
          [8, 96, 120, 16],
          { fill_mode: 3, gradient_start: [0, 0], gradient_end: [120, 0] },
        ],
        [
          "checker",
          [136, 96, 24, 24],
          { color: [0, 0, 0, 0], checker_size: 6 },
        ],
        [
          "fine-checker",
          [168, 96, 24, 24],
          {
            color: [0, 0, 0, 0],
            checker_size: 0.25,
            checker_color0: [1, 1, 1, 1],
            checker_color1: [0, 0, 0, 1],
          },
        ],
        [
          "swatch",
          [200, 96, 24, 24],
          {
            color: orange,
            checker_size: 6,
            checker_color0: checkerA,
            checker_color1: checkerB,
          },
        ],
      ];
    await root.render(
      <Entity id="colour-fills">
        <Layout kind={3} width={232} height={128} align_x={-1} align_y={-1} />
        <Children>
          <Entity id="colour-page">
            <Layout width={232} height={128} align_x={-1} align_y={-1} />
            <Style red={0} green={0} blue={0} />
            <Box width={232} height={128} />
          </Entity>
          {surfaces.map(([id, [x, y, width, height], row]) => (
            <Entity key={id} id={`colour-${id}`}>
              <Layout width={width} height={height} align_x={-1} align_y={-1} />
              <Style x={x} y={y} />
              <Skin
                parts={contract.GuiSkin.encodeParts({
                  nextSlot: 1,
                  rows: new Map([[0, { part: background, ...row }]]),
                })}
              />
            </Entity>
          ))}
        </Children>
      </Entity>,
    );
    await session.selectOutput(canvasOutput(world), {
      width: 232,
      height: 128,
      devicePixelRatio: COLOUR_DPR,
    });

    /**
     * A probe: the device pixel under a logical point, and the expected sRGB
     * levels at that pixel's centre in logical units.
     */
    type Probe = {
      surface: string;
      point: readonly [number, number];
      expected: (center: readonly [number, number]) => Rgb;
      tolerance: number;
    };
    const probes: Probe[] = [];
    /** `count` fractions from `margin` to `1 - margin`. */
    const grid = (count: number, margin: number) =>
      Array.from(
        { length: count },
        (_, index) => margin + ((1 - 2 * margin) * index) / (count - 1),
      );
    // Clear of the framed field's border and cut corners.
    for (const [id, hue, [x, y, size]] of [
      ["field", 0.55, [8, 8, 80]],
      ["framed-field", 0.08, [96, 8, 80]],
    ] as const)
      for (const across of grid(9, 0.1))
        for (const down of grid(9, 0.1))
          probes.push({
            surface: id,
            point: [x + across * size, y + down * size],
            // Saturation rises to the right and value upward.
            expected: ([cx, cy]) =>
              hsv(hue, (cx - x) / size, 1 - (cy - y) / size),
            tolerance: 2,
          });
    for (const along of grid(33, 0.03)) {
      probes.push({
        surface: "hue-rail",
        point: [192, 8 + along * 80],
        expected: ([, cy]) => hsv((88 - cy) / 80, 1, 1),
        tolerance: 2,
      });
      probes.push({
        surface: "hue-row",
        point: [8 + along * 120, 104],
        expected: ([cx]) => hsv((cx - 8) / 120, 1, 1),
        tolerance: 2,
      });
    }
    /** Checker cell colour at a logical point of a checker from `origin`. */
    const cellColour = (
      origin: readonly [number, number],
      size: number,
      colours: readonly [readonly number[], readonly number[]],
      [cx, cy]: readonly [number, number],
    ) =>
      colours[
        (Math.floor((cx - origin[0]) / size) +
          Math.floor((cy - origin[1]) / size)) %
          2
      ]!;
    // The alpha rail at every cell centre down its middle columns: alpha is the
    // fraction from the bottom, composited in linear light over the cell.
    for (let row = 0; row < 20; row++)
      for (const column of [1, 2])
        probes.push({
          surface: "alpha-rail",
          point: [208 + column * 4 + 2, 8 + row * 4 + 2],
          expected: (center) =>
            over(
              [cyan[0], cyan[1], cyan[2], 1 - (center[1] - 8) / 80],
              cellColour([208, 8], 4, [checkerA, checkerB], center),
            ),
          tolerance: 2,
        });
    for (const [cellX, cellY] of [
      [0, 0],
      [1, 0],
      [0, 1],
      [3, 3],
      [2, 3],
    ] as const) {
      probes.push({
        surface: "checker",
        point: [136 + cellX * 6 + 3, 96 + cellY * 6 + 3],
        // The absent colours: sRGB #CCCCCC on the corner cell, #999999 beside it.
        expected: () =>
          (cellX + cellY) % 2 ? [153, 153, 153] : [204, 204, 204],
        tolerance: 2,
      });
      probes.push({
        surface: "swatch",
        point: [200 + cellX * 6 + 3, 96 + cellY * 6 + 3],
        expected: (center) =>
          over(orange, cellColour([200, 96], 6, [checkerA, checkerB], center)),
        tolerance: 2,
      });
    }
    // Cells of a quarter unit are half a pixel: the mean of white and black in
    // linear light, sRGB 188.
    for (const [x, y] of [
      [172, 100],
      [180, 108.3],
      [187.7, 115.1],
    ] as const)
      probes.push({
        surface: "fine-checker",
        point: [x, y],
        expected: () => [toLevel(0.5), toLevel(0.5), toLevel(0.5)],
        tolerance: 2,
      });
    const framed: [string, number, number, (pixel: Rgb) => boolean][] = [
      [
        "border of the framed field",
        96.6,
        48,
        ([red, green, blue]) =>
          Math.abs(red - 53) < 8 &&
          Math.abs(green - 92) < 8 &&
          Math.abs(blue - 112) < 8,
      ],
      // Beyond the glow's reach from the cut, where the field would be white.
      [
        "cut corner of the framed field",
        96.6,
        8.6,
        ([red, green, blue]) => red < 24 && green < 24 && blue < 24,
      ],
      [
        "glow beside the framed field",
        94,
        48,
        ([red, green, blue]) => red < 40 && green > 40 && blue > 50,
      ],
    ];
    const sample = (
      frame: PresentedCapture,
      [x, y]: readonly [number, number],
    ) => {
      const width = frame.view.binding.viewport.width;
      const column = Math.floor(x * COLOUR_DPR);
      const row = Math.floor(y * COLOUR_DPR);
      const offset = (row * width + column) * 4;
      const pixels = new Uint8Array(frame.pixels);
      return {
        pixel: [
          pixels[offset]!,
          pixels[offset + 1]!,
          pixels[offset + 2]!,
        ] as Rgb,
        center: [
          (column + 0.5) / COLOUR_DPR,
          (row + 0.5) / COLOUR_DPR,
        ] as const,
      };
    };
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const sized = frame.view.binding.viewport.width === 232 * COLOUR_DPR;
      const errors: Record<string, number> = {};
      const failed: string[] = [];
      if (sized) {
        for (const probe of probes) {
          const { pixel, center } = sample(frame, probe.point);
          const expected = probe.expected(center);
          const error = Math.max(
            ...pixel.map((value, channel) =>
              Math.abs(value - expected[channel]!),
            ),
          );
          errors[probe.surface] = Math.max(errors[probe.surface] ?? 0, error);
          if (error > probe.tolerance)
            failed.push(
              `${probe.surface} at ${probe.point.join(",")}: ${pixel.join(",")} vs ${expected.map((value) => value.toFixed(1)).join(",")}`,
            );
        }
        for (const [label, x, y, expected] of framed) {
          const { pixel } = sample(frame, [x, y]);
          if (!expected(pixel)) failed.push(`${label} ${pixel.join(",")}`);
        }
      } else failed.push("unsized");
      if (failed.length === 0 || performance.now() >= deadline) {
        images.push({
          label: failed.length === 0 ? "colour-fills" : "failed-colour-fills",
          width: frame.view.binding.viewport.width,
          height: frame.view.binding.viewport.height,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(
          failed.length === 0,
          `Colour fill pixels failed: ${failed.slice(0, 12).join("; ")}${failed.length > 12 ? `; and ${failed.length - 12} more` : ""}`,
        );
        return errors;
      }
      sequence = frame.sequence;
    }
  }
  function coloredPixels(
    frame: PresentedCapture,
    bounds: readonly [number, number, number, number],
    matches: (red: number, green: number, blue: number) => boolean,
  ): number {
    const width = frame.view.binding.viewport.width;
    const pixels = new Uint8Array(frame.pixels);
    let count = 0;
    for (let row = bounds[1]; row < bounds[3]; row++) {
      for (let column = bounds[0]; column < bounds[2]; column++) {
        const offset = (row * width + column) * 4;
        if (matches(pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!))
          count++;
      }
    }
    return count;
  }
  try {
    await root.render(scene(96, false));
    // The session's World selects the Canvas System, so it is the canvas.
    const output = canvasOutput(world);
    await session.selectOutput(output, {
      width: 96,
      height: 64,
      devicePixelRatio: 1,
    });
    await capture("theme-red-clip-blue-override", 96, false);
    const labelEntity = (await client.inspect()).entities.find(
      (entry) => entry.metadata.symbolicId === "label",
    );
    const labelTable = labelEntity?.components.find(
      (entry) => entry.component === contract.GuiSkin.id,
    )?.fields.parts;
    check(
      labelTable &&
        typeof labelTable === "object" &&
        "rows" in labelTable &&
        labelTable.rows.get(5)?.asset,
      `Generated row asset reference was not acknowledged: ${JSON.stringify({ labelTable, writes: writes.flat().filter((command) => (command.kind === "setField" || command.kind === "insertComponent") && command.component === contract.GuiSkin.id) }, (_, value) => (value instanceof Map ? [...value] : typeof value === "bigint" ? String(value) : value))}; errors=${errors.map((error) => error.message).join("; ")}`,
    );
    check(press.current, "Callback form omitted its acknowledged target");
    const pressed = await press.current.action({ kind: "press" });
    check(pressed.ok, "Callback form press rejected");
    await callbackPaint.promise;
    await capture("callback-theme-green-preserves-blue", 96, true);
    await root.render(scene(128, true));
    await session.selectOutput(output, {
      width: 128,
      height: 64,
      devicePixelRatio: 1,
    });
    await capture("resized-explicit-canvas", 128, true);
    await primitiveShapes();
    await skinnedPanel();
    await arcShapes();
    const colourErrors = await colourFills();
    const refs = Array.from({ length: 140 }, () =>
      createRef<GuiControlHandle>(),
    );
    const gridParts = (green: boolean) =>
      contract.GuiTheme.encodeParts({
        nextSlot: 1,
        rows: new Map([
          [
            0,
            {
              part,
              color: green ? [0, 1, 0, 1] : [1, 0, 0, 1],
              corner_radius: [0, 0],
              border_width: 0,
              corner_cut: square,
            },
          ],
        ]),
      });
    await root.render(
      <>
        <Entity id="red-theme">
          <Theme parts={gridParts(false)} />
        </Entity>
        <Entity id="green-theme">
          <Theme parts={gridParts(true)} />
        </Entity>
        <Entity id="grid">
          <Layout kind={3} width={56} height={40} align_x={-1} align_y={-1} />
          <Children>
            {refs.map((ref, index) => (
              <Entity key={index} id={`pixel-control-${index}`}>
                <Layout width={4} height={4} align_x={-1} align_y={-1} />
                <Style x={(index % 14) * 4} y={Math.floor(index / 14) * 4} />
                <Skin theme={index % 2 ? "green-theme" : "red-theme"} />
                <Button label="" ref={ref} />
              </Entity>
            ))}
          </Children>
        </Entity>
      </>,
    );
    check(
      refs.every((ref) => ref.current),
      "Large painted scene omitted acknowledged refs",
    );
    await session.selectOutput(output, {
      width: 56,
      height: 40,
      devicePixelRatio: 1,
    });
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const matches = refs.every((_, index) => {
        const column = (index % 14) * 4;
        const row = Math.floor(index / 14) * 4;
        return region(
          frame,
          [column + 1, row + 1, column + 3, row + 3],
          index % 2 ? [0, 255, 0] : [255, 0, 0],
        );
      });
      if (matches || performance.now() >= deadline) {
        images.push({
          label: matches
            ? "140-indexed-control-refs"
            : "failed-140-indexed-control-refs",
          width: 56,
          height: 40,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(
          matches,
          "140 control refs did not produce independent alternating paint regions",
        );
        break;
      }
      sequence = frame.sequence;
    }
    const retained = refs[0]!.current!;
    const scroll = createRef<GuiControlHandle>();
    const listScene = (count: number) => (
      <Entity id="scroll-pixels">
        <Layout width={56} height={40} />
        {/* A plain black list: the rows drop the default look's frame line
            and corner cuts, which would reach the checked rows and cut away
            most of a 2-unit thumb. */}
        <Skin
          parts={contract.GuiSkin.encodeParts({
            nextSlot: 2,
            rows: new Map([
              [
                0,
                {
                  part,
                  color: [0, 0, 0, 1],
                  border_width: 0,
                  corner_cut: [0, 0, 0, 0],
                },
              ],
              [
                1,
                {
                  part: contract.guiPaintPartIndex({ part: "scrollThumbY" }),
                  corner_cut: [0, 0, 0, 0],
                },
              ],
            ]),
          })}
        />
        {/* The bar along the right edge, as thick as 5% of the viewport. */}
        <VirtualList
          ref={scroll}
          item_count={count}
          item_extent={8}
          overscan={1}
          bar_thickness={2}
          bar_inset={0}
          bar_end_inset={0}
          renderItem={(index) => (
            <>
              <Layout width={56} height={8} />
              <Style
                red={index % 2 ? 0 : 1}
                green={index % 2 ? 1 : 0}
                blue={0}
              />
              <Box width={56} height={8} />
            </>
          )}
        />
      </Entity>
    );
    await root.render(listScene(100_000));
    await session.selectOutput(output, {
      width: 56,
      height: 40,
      devicePixelRatio: 1,
    });
    const listCapture = async (label: string, first: number, count: number) => {
      const deadline = performance.now() + 20_000;
      let sequence: bigint | undefined;
      for (;;) {
        const frame = await session.capture(
          sequence === undefined ? {} : { afterSequence: sequence },
        );
        const rows = Array.from({ length: 5 }, (_, row) => {
          const index = first + row;
          const color: [number, number, number] =
            index >= count ? [0, 0, 0] : index % 2 ? [0, 255, 0] : [255, 0, 0];
          return region(frame, [2, row * 8 + 2, 51, row * 8 + 6], color);
        });
        const rowsMatch = rows.every(Boolean);
        // The thumb travels the 38 units between the track's pointed ends,
        // at least two thicknesses long, in the default look's cyan.
        const thumbLength = Math.max(4, (38 * 40) / (count * 8));
        const thumbCenter =
          count > 5
            ? Math.floor(
                1 +
                  ((38 - thumbLength) * first * 8) / (count * 8 - 40) +
                  thumbLength / 2,
              )
            : 2;
        const thumbPixel = (thumbCenter * 56 + 55) * 4;
        const bar = new Uint8Array(frame.pixels).slice(
          thumbPixel,
          thumbPixel + 3,
        );
        const matches =
          rowsMatch &&
          (count <= 5 || (bar[0]! < 80 && bar[1]! > 200 && bar[2]! > 200));
        if (matches || performance.now() >= deadline) {
          images.push({
            label: matches ? label : `failed-${label}`,
            width: 56,
            height: 40,
            pixels: [...new Uint8Array(frame.pixels)],
            sequence: frame.sequence,
          });
          const pixels = new Uint8Array(frame.pixels);
          check(
            matches,
            `Ordinary virtual item placement/clip pixels failed: ${label} ${JSON.stringify(
              {
                rows,
                bar: [...bar],
                // Left, centre and right of each row's checked region.
                samples: rows.map((_, row) =>
                  [3, 26, 50].map((column) => {
                    const at = ((row * 8 + 4) * 56 + column) * 4;
                    return [...pixels.slice(at, at + 3)];
                  }),
                ),
              },
            )}`,
          );
          return;
        }
        sequence = frame.sequence;
      }
    };
    await listCapture("virtual-list-initial-range", 0, 100_000);
    check(scroll.current, "Painted virtual list omitted acknowledged ref");
    check(
      (await scroll.current.action({ kind: "scrollToIndex", index: 99_991 }))
        .ok,
      "Painted virtual list scroll-to-index failed",
    );
    await listCapture("virtual-list-distant-ordinary-items", 99_991, 100_000);
    await root.render(listScene(4));
    await listCapture("virtual-list-shrink-clip", 0, 4);
    await root.render(listScene(100_000));
    await listCapture("virtual-list-regrow-keeps-clamped-anchor", 0, 100_000);
    // An omitted IppCanvas output presents the root CanvasWorld's canvas;
    // an explicit one would conflict with it.
    await session.selectOutput(undefined, {
      width: 56,
      height: 40,
      devicePixelRatio: 1,
    });
    const panel = createRef<CanvasWorldHandle>();
    await root.render(
      <CanvasWorld
        create={{
          selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.gui-layout"],
        }}
        extent={[56, 40]}
        presentation={{ root: true }}
        ref={panel}
      >
        <Entity id="owned-panel">
          <Layout width={56} height={40} align_x={-1} align_y={-1} />
          <Style red={0} green={0} blue={1} />
          <Box width={56} height={40} />
        </Entity>
      </CanvasWorld>,
    );
    const readyBy = performance.now() + 20_000;
    while (!panel.current && performance.now() < readyBy)
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    const owned = panel.current;
    check(owned, "Root CanvasWorld did not become ready");
    const ownedWorld = owned.world;
    check(
      owned.output.kind === "canvas" &&
        owned.output.world.id === ownedWorld.id &&
        owned.output.world.incarnation === ownedWorld.incarnation,
      "Root CanvasWorld did not report its World's canvas",
    );
    while (
      session.view?.binding.output.world.id !== ownedWorld.id &&
      performance.now() < readyBy
    )
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    check(
      session.view?.binding.output.world.id === ownedWorld.id,
      "The Canvas root did not present the root CanvasWorld",
    );
    const ownedBy = performance.now() + 20_000;
    let ownedSequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        ownedSequence === undefined ? {} : { afterSequence: ownedSequence },
      );
      const matches =
        frame.view.binding.output.world.id === ownedWorld.id &&
        region(frame, [2, 2, 54, 38], [0, 0, 255]);
      if (matches || performance.now() >= ownedBy) {
        images.push({
          label: matches ? "canvas-world-root" : "failed-canvas-world-root",
          width: 56,
          height: 40,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(matches, "Root CanvasWorld did not paint its own World");
        break;
      }
      ownedSequence = frame.sequence;
    }
    const ownedClosed = owned.closed;
    await root.render(null);
    await ownedClosed;
    check(
      !(await host.listWorlds()).some((item) => item.id === ownedWorld.id),
      "Removing the root CanvasWorld left its World",
    );
    await root.unmount();
    check(
      refs.every((ref) => ref.current === null),
      "Painted refs survived unmount",
    );
    await retained.read().then(
      () => {
        throw new Error("Painted retired handle remained live");
      },
      () => {},
    );
    check(
      errors.length === 0,
      `GUI paint root errors: ${errors.map((error) => error.message).join(", ")}`,
    );
    return {
      images,
      assertions: [
        "shared theme",
        "observed press callback reauthors theme before completed pixels",
        "per-control precedence",
        "layout",
        "clip",
        "resize",
        "raw font/text",
        "drawing",
        "bitmap",
        "generated row asset references",
        "140 live indexed refs with independent painted regions and exact cleanup",
        "cut corners, two-sided glow, corner accents and stroke marks in captured pixels",
        "skinned non-control frame, accents, separator and box beneath their children in captured pixels",
        "ring arcs: track and value, tick dashes, butt ends, the wrap, a whole ring, glow and a zero sweep in captured pixels",
        "saturation-value fields and hue rails match independently computed HSV sRGB colours at sampled pixels; alpha rail, translucent swatch and checker cells composite in linear light; sub-pixel checker cells show their mean",
        "root CanvasWorld presents and destroys its own World",
      ],
      colourErrors,
      failure: null,
    };
  } catch (error) {
    return {
      images,
      assertions: [],
      colourErrors: null,
      failure: error instanceof Error ? error.message : String(error),
    };
  } finally {
    await session.close();
    await client.close();
    await host.destroyWorld(world);
  }
}
