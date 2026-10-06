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
  Behavior,
  Button,
  Layout,
  Overlay,
  ScrollView,
  Skin,
  Style,
} from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import type { GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";

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
const cyan = ([r, g, b]: Rgb) => g > 150 && b > 150 && r < 90;
const white = ([r, g, b]: Rgb) => r > 220 && g > 220 && b > 220;
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
            corner_cut: [0, 0, 0, 0],
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
                        <Style y={2} />
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
            <Style y={50} layer={0xffff_ffff} />
            <Skin parts={solid([1, 1, 0])} />
            <Button label="" onPress={() => presses.push("later")} />
          </Entity>
        </Children>
      </Entity>
      <Entity id="overlay-context">
        <Overlay side={1} align={0} />
        <Style x={150} y={80} />
        <Layout width={40} height={30} />
        <Skin parts={solid([1, 0, 1])} />
        <Button label="" onPress={() => presses.push("context")} />
      </Entity>
    </>
  );

  /**
   * Independent canvas rectangles: old dialog 8..128 x 8..104, its centred
   * popup 18..118 x 26..86; later dialog 60..140 x 32..104, its centred
   * popup 80..120 x 53..83; notification 88..136 x 64..104.
   * All modes are manual so band priority is tested independently of modality.
   */
  const priorityScene = (later: boolean, notification: boolean) => (
    <>
      <Entity id="priority-content">
        <Layout width={VIEW[0]} height={VIEW[1]} align_x={-1} align_y={-1} />
        <Style layer={0xffff_ffff} red={0} green={0} blue={1} />
        <Box width={VIEW[0]} height={VIEW[1]} />
      </Entity>
      <Entity id="priority-old-dialog">
        <Overlay side={1} band={2} mode={0} />
        <Layout width={120} height={96} align_x={-1} align_y={-1} />
        <Style x={8} y={8} />
        <Skin parts={solid([1, 0, 0])} />
        <Button label="" onPress={() => presses.push("old-dialog")} />
        <Children>
          <Entity id="priority-old-raised">
            <Style x={4} y={4} layer={0xffff_ffff} red={1} green={1} blue={1} />
            <Box width={112} height={88} />
          </Entity>
          <Entity id="priority-old-popup">
            <Overlay side={4} align={1} band={1} mode={0} />
            <Layout width={100} height={60} />
            <Skin parts={solid([0, 1, 0])} />
            <Button label="" onPress={() => presses.push("old-popup")} />
          </Entity>
        </Children>
      </Entity>
      <Entity id="priority-later-dialog">
        <Overlay side={1} band={2} mode={0} />
        <Behavior visible={later} />
        <Layout width={80} height={72} align_x={-1} align_y={-1} />
        <Style x={60} y={32} />
        <Skin parts={solid([1, 1, 0])} />
        <Button label="" onPress={() => presses.push("later-dialog")} />
        <Children>
          <Entity id="priority-later-popup">
            <Overlay side={4} align={1} band={1} mode={0} />
            <Layout width={40} height={30} />
            <Skin parts={solid([0, 1, 1])} />
            <Button label="" onPress={() => presses.push("later-popup")} />
          </Entity>
        </Children>
      </Entity>
      <Entity id="priority-notification">
        <Overlay side={1} band={3} mode={0} />
        <Behavior visible={notification} />
        <Layout width={48} height={40} />
        <Style x={88} y={64} />
        <Skin parts={solid([1, 0, 1])} />
        <Button label="" onPress={() => presses.push("notification")} />
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
    await root.render(priorityScene(true, true));
    await captureUntil(session, "overlays-priority-scopes", [
      ["huge content offset remains below a dialog", 10, 10, red],
      ["raised old-dialog decoration", 16, 20, white],
      ["nested popup inherits dialog band above raised owner", 24, 34, green],
      ["later complete dialog above older nested popup", 68, 40, yellow],
      ["later nested popup above its owner", 86, 60, cyan],
      ["notification above both complete dialog scopes", 100, 80, magenta],
      ["content outside overlays", 150, 114, blue],
    ]);
    for (const [x, y, target] of [
      [24, 34, "old-popup"],
      [68, 40, "later-dialog"],
      [86, 60, "later-popup"],
      [100, 80, "notification"],
    ] as const)
      check(
        (await press(session, x, y)) === target,
        `Scoped band input at ${x},${y} must reach ${target}: ${presses}`,
      );
    await root.render(priorityScene(true, false));
    await captureUntil(session, "overlays-notification-closed", [
      ["closing notification reveals later nested popup", 100, 80, cyan],
      ["later dialog still contains the older scope", 68, 40, yellow],
    ]);
    await root.render(priorityScene(false, false));
    await captureUntil(session, "overlays-later-scope-closed", [
      ["closing dialog hides its complete nested scope", 100, 80, green],
      ["older nested popup revealed", 68, 40, green],
    ]);
    check(
      (await press(session, 100, 80)) === "old-popup",
      `Closing later scopes must remove their input targets: ${presses}`,
    );
    return {
      images,
      assertions: [
        "list paints outside its scroll view's clip and over a later sibling",
        "press on a list row reaches the row, not the sibling beneath",
        "list flips above a trigger near the bottom edge, keeping its gap",
        "context overlay at a canvas point shifts inside the right edge",
        "overlay bands outrank maximum content priorities independently of interaction mode",
        "nested popups inherit the owner's band above all owner component layers",
        "later dialogs outrank older complete scopes, notifications remain highest",
        "closed overlays remove nested paint and input targets",
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
