/**
 * Overlays through the generated client, React declarations and real
 * presentation on a root canvas: a list opened below a trigger inside a
 * clipped scroll view paints outside the scroll view's clip and above a later
 * sibling, and takes a press where that sibling lies beneath it; the same
 * trigger near the bottom edge flips the list above it; and a context
 * overlay at a canvas point near the right edge shifts inside the canvas.
 *
 * Probe points follow from the authored geometry: the canvas is 160 x 120
 * logical units at one pixel per unit, the trigger is 64 x 16 at (8, 4) in a
 * scroll view whose 80 x 40 viewport sits at (8, top), and the list is three
 * 16-unit rows two units from the trigger.
 */
import {
  canvasOutput,
  type Client,
  type GuiPhysicalContext,
  type HostClientBase,
  type PresentedCapture,
} from "@ipp/client";
import { Children, Entity } from "@ipp/react";
import {
  Box,
  Button,
  Layout,
  Overlay,
  ScrollView,
  Skin,
  Style,
} from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import { check, type GuiContract } from "./gui-authoring.js";

type Rgb = readonly [number, number, number];

interface OverlayImage {
  label: string;
  width: number;
  height: number;
  pixels: number[];
  sequence: bigint;
}

/** Systems of the overlay canvas World. */
const SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

const VIEW = [160, 120] as const;

/** Canvas top of the scroll viewport with room below, and near the bottom. */
const OPEN_BELOW = 4;
const FLIPPED_ABOVE = 76;

const red = ([r, g, b]: Rgb) => r > 150 && g < 90 && b < 90;
const green = ([r, g, b]: Rgb) => g > 150 && r < 90 && b < 90;
const blue = ([r, g, b]: Rgb) => b > 150 && r < 90 && g < 90;
const yellow = ([r, g, b]: Rgb) => r > 200 && g > 200 && b < 60;
const magenta = ([r, g, b]: Rgb) => r > 150 && b > 150 && g < 90;
const black = ([r, g, b]: Rgb) => r < 24 && g < 24 && b < 24;
const grey = ([r, g, b]: Rgb) =>
  r > 90 && r < 230 && Math.abs(r - g) < 12 && Math.abs(r - b) < 12;

function sample(frame: PresentedCapture, x: number, y: number): Rgb {
  const width = frame.view.binding.viewport.width;
  const offset = (y * width + x) * 4;
  const pixels = new Uint8Array(frame.pixels);
  return [pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!];
}

export async function guiOverlays(
  host: HostClientBase<Client>,
  contract: GuiContract,
) {
  const images: OverlayImage[] = [];
  const presses: string[] = [];
  const part = contract.guiPaintPartIndex({ part: "background" });
  const solid = (color: readonly [number, number, number]) =>
    contract.GuiSkin.encodeParts({
      nextSlot: 1,
      rows: new Map([
        [
          0,
          {
            part,
            color: [...color, 1],
            corner_radius: [0, 0],
            border_width: 0,
          },
        ],
      ]),
    });

  const item = (name: string, color: readonly [number, number, number]) => (
    <Entity id={`overlay-${name}`}>
      <Layout kind={3} height={16} />
      <Skin parts={solid(color)} />
      <Button label="" onPress={() => presses.push(name)} />
    </Entity>
  );

  /** The scene with the scroll viewport's top at `top`. */
  const scene = (top: number) => (
    <>
      <Entity id="overlay-root">
        <Layout
          kind={3}
          width={VIEW[0]}
          height={VIEW[1]}
          align_x={-1}
          align_y={-1}
        />
        <Children>
          <Entity id="overlay-page">
            <Style red={0} green={0} blue={0} />
            <Box width={VIEW[0]} height={VIEW[1]} />
          </Entity>
          <Entity id="overlay-scroll">
            <Layout width={80} height={40} align_x={-1} align_y={-1} />
            <Style x={8} y={top} />
            <ScrollView axis={1} />
            <Children>
              <Entity id="overlay-content">
                <Layout
                  kind={3}
                  width={80}
                  height={120}
                  align_x={-1}
                  align_y={-1}
                />
                <Children>
                  <Entity id="overlay-filler">
                    <Style red={0.4} green={0.4} blue={0.4} />
                    <Box width={80} height={120} />
                  </Entity>
                  <Entity id="overlay-trigger">
                    <Layout width={64} height={16} align_x={-1} align_y={-1} />
                    <Style x={8} y={4} />
                    <Skin parts={solid([1, 0, 0])} />
                    <Button label="" onPress={() => presses.push("trigger")} />
                    <Children>
                      <Entity id="overlay-list">
                        <Overlay side={0} align={3} />
                        <Style y={2} layer={1} />
                        <Layout kind={2} />
                        <Children>
                          {item("first", [0, 1, 0])}
                          {item("second", [0, 0, 1])}
                          {item("third", [0, 1, 0])}
                        </Children>
                      </Entity>
                    </Children>
                  </Entity>
                </Children>
              </Entity>
            </Children>
          </Entity>
          <Entity id="overlay-later">
            <Layout width={VIEW[0]} height={20} align_x={-1} align_y={-1} />
            <Style y={50} />
            <Skin parts={solid([1, 1, 0])} />
            <Button label="" onPress={() => presses.push("later")} />
          </Entity>
        </Children>
      </Entity>
      <Entity id="overlay-context">
        <Overlay side={1} align={0} />
        <Style x={150} y={80} layer={1} />
        <Layout width={40} height={30} />
        <Skin parts={solid([1, 0, 1])} />
        <Button label="" onPress={() => presses.push("context")} />
      </Entity>
    </>
  );

  const cleanup: (() => Promise<unknown>)[] = [];
  let input: GuiPhysicalContext | undefined;

  /** Capture until every probe passes, keeping the last frame either way. */
  async function captureUntil(
    session: CanvasWorldSession,
    label: string,
    probes: readonly [string, number, number, (rgb: Rgb) => boolean][],
  ) {
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const sized = frame.view.binding.viewport.width === VIEW[0];
      const failed = sized
        ? probes.filter(([, x, y, test]) => !test(sample(frame, x, y)))
        : probes;
      if (failed.length === 0 || performance.now() >= deadline) {
        images.push({
          label: failed.length === 0 ? label : `failed-${label}`,
          width: frame.view.binding.viewport.width,
          height: frame.view.binding.viewport.height,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(
          failed.length === 0,
          `${label}: ${failed
            .map(([name, x, y]) =>
              sized ? `${name} ${sample(frame, x, y).join(",")}` : name,
            )
            .join("; ")}`,
        );
        return frame;
      }
      sequence = frame.sequence;
    }
  }

  /** Press and release at a canvas point and return the controls pressed. */
  async function press(session: CanvasWorldSession, x: number, y: number) {
    const view = session.view;
    check(view, "The overlay canvas has no presented view");
    if (!input) input = await host.input.open(view);
    presses.length = 0;
    const point = [(x + 0.5) / VIEW[0], (y + 0.5) / VIEW[1]] as const;
    for (const kind of ["pointerDown", "pointerUp"] as const) {
      const outcome = await input.send({ kind, pointer: 1n, point });
      check(
        outcome.disposition === "routed",
        `${kind} at ${x},${y} was not routed: ${JSON.stringify(outcome)}`,
      );
    }
    const deadline = performance.now() + 10_000;
    while (presses.length === 0 && performance.now() < deadline)
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    return presses.join(",");
  }

  try {
    const world = (await host.createWorld({ selectedSystems: [...SYSTEMS] }))
      .reference;
    cleanup.push(() => host.destroyWorld(world));
    const client = await host.openWorld(world);
    cleanup.push(() => client.close());
    const session = new CanvasWorldSession({ host, client });
    cleanup.push(
      () => session.close(),
      async () => input?.close(),
    );
    const root = session.createRoot();
    await root.render(scene(OPEN_BELOW));
    await session.selectOutput(canvasOutput(world), {
      width: VIEW[0],
      height: VIEW[1],
      devicePixelRatio: 1,
    });

    // Trigger 16..80 x 8..24; list rows at 26, 42 and 58; the viewport
    // ends at 44 and the later sibling spans 50..70.
    await captureUntil(session, "overlays-open-below", [
      ["trigger", 48, 16, red],
      ["first row inside the viewport", 48, 34, green],
      ["second row outside the viewport's clip", 48, 47, blue],
      ["second row over the later sibling", 48, 54, blue],
      ["third row over the later sibling", 48, 64, green],
      ["later sibling beside the list", 100, 60, yellow],
      ["scroll content inside the viewport", 84, 30, grey],
      ["page below the clipped viewport", 84, 47, black],
      ["below the list", 48, 76, black],
      ["context overlay shifted inside", 140, 95, magenta],
      ["context overlay's right edge", 158, 95, magenta],
      ["left of the shifted context overlay", 116, 95, black],
    ]);
    check(
      (await press(session, 48, 64)) === "third",
      `Press on the list row over the later sibling: ${presses}`,
    );
    check(
      (await press(session, 100, 60)) === "later",
      `Press on the later sibling beside the list: ${presses}`,
    );

    // Near the bottom edge the list lacks room below and flips above its
    // trigger, keeping its gap: rows at 30, 46 and 62 above the trigger at 80.
    await root.render(scene(FLIPPED_ABOVE));
    await captureUntil(session, "overlays-flipped-above", [
      ["trigger", 48, 88, red],
      ["first row", 48, 38, green],
      ["second row over the later sibling", 48, 56, blue],
      ["third row over the later sibling", 48, 66, green],
      ["scroll content in the gap above the trigger", 48, 79, grey],
      ["later sibling beside the list", 100, 60, yellow],
      ["scroll content below the trigger", 48, 104, grey],
      ["context overlay shifted inside", 140, 95, magenta],
    ]);
    check(
      (await press(session, 48, 56)) === "second",
      `Press on the flipped list over the later sibling: ${presses}`,
    );
    return {
      images,
      assertions: [
        "list paints outside its scroll view's clip and over a later sibling",
        "press on a list row reaches the row, not the sibling beneath",
        "list flips above a trigger near the bottom edge, keeping its gap",
        "context overlay at a canvas point shifts inside the right edge",
      ],
      failure: null,
    };
  } catch (error) {
    return {
      images,
      assertions: [],
      failure: error instanceof Error ? error.message : String(error),
    };
  } finally {
    for (const release of cleanup.reverse()) await release().catch(() => {});
  }
}
