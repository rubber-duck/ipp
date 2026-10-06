/**
 * Text and the shapes painted after it, through a real Host: labels under a
 * modal dialog, lines under a toast, a panel's text under a panel stacked on
 * it, and lines under a raised layer declared before them. The renderer draws a
 * run of retained GUI work as its shapes and then its glyphs, cutting the run
 * where a later shape covers earlier text; without the cut each case would
 * show the earlier text through the shape painted after it.
 *
 * The scene is captured five ways: as declared; with every overlay raised to a
 * canvas layer of its own, which paints in the same order but draws each layer
 * apart, so that capture never depends on cutting a run; with the text beneath
 * the overlays transparent; without the overlays; and with neither. The
 * declared scene must equal the layered one, the opaque overlays must hide the
 * text beneath them, and in every case the text must lie under its overlay and
 * show beside it, as the captures without overlays and without text tell.
 */
import { Children, Entity, type AssetReference } from "@ipp/react";
import { Layout, Style } from "@ipp/react/gui";
import { Panel, PanelHeader, TextLine, Toast } from "@ipp/react/gui-kit";
import type { Client, HostClientBase } from "@ipp/client";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { ReactNode } from "react";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import { Fill, Label, SHEET_PAGE, STACK, placed } from "../../skin-lab/kit.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import {
  CAPTURE_SCALE,
  defineSpecimen,
  srgb,
  type Point,
  type Rect,
} from "../../skin-lab/specimen.js";
import { TEXT_BODY } from "../../skin-lab/themes/geometry.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import type { ProbeResult } from "../default-skin-oracle.js";

export const EXTENT = [720, 270] as const;

/** The text beneath the overlays: a yellow no overlay paints. */
const BENEATH = srgb("#ffd21f");

/** The raised layer's opaque fill. */
const RAISED_FILL = srgb("#24384c");

/** One overlap case: its region of the canvas and an opaque overlay's interior. */
interface OverlapCase {
  readonly name: string;
  readonly region: Rect;
  /** Where the overlay is opaque and covers text, or nothing for the toast. */
  readonly opaque?: Rect;
}

const DIALOG: Rect = [56, 28, 150, 104];
const FIRST: Rect = [16, 150, 190, 104];
const SECOND: Rect = [104, 178, 180, 84];
const RAISED: Rect = [520, 34, 150, 74];
const TOAST_AT: Point = [272, 44];
const TOAST_WIDTH = 200;

const CASES: readonly OverlapCase[] = [
  {
    name: "dialog over labels",
    region: [0, 0, 236, 146],
    opaque: inset(DIALOG, 8),
  },
  { name: "toast over text", region: [236, 0, 250, 146] },
  {
    name: "stacked panels",
    region: [0, 146, 300, 124],
    opaque: inset(SECOND, 8),
  },
  {
    name: "raised layer over text",
    region: [486, 0, 234, 146],
    opaque: inset(RAISED, 4),
  },
];

function inset([x, y, width, height]: Rect, margin: number): Rect {
  return [x + margin, y + margin, width - 2 * margin, height - 2 * margin];
}

/** Which parts of the scene a capture declares. */
interface Variant {
  /** Overlays painted at the canvas's base layer, or each raised to its own. */
  readonly raised: boolean;
  /** Text beneath the overlays shown, or transparent. */
  readonly beneath: boolean;
  /** Overlays declared at all. */
  readonly overlays: boolean;
}

/** `count` lines of yellow `text` from `at`, `pitch` apart. */
function Lines({
  id,
  at,
  count,
  text,
  font,
  visible,
  pitch = 24,
}: {
  readonly id: string;
  readonly at: Point;
  readonly count: number;
  readonly text: string;
  readonly font: AssetReference;
  readonly visible: boolean;
  readonly pitch?: number;
}) {
  return (
    <>
      {Array.from({ length: count }, (_, index) => (
        <Label
          key={index}
          id={`${id}/${index}`}
          at={[at[0], at[1] + index * pitch]}
          text={text}
          font={font}
          size={TEXT_BODY}
          color={[BENEATH[0], BENEATH[1], BENEATH[2], visible ? 1 : 0]}
        />
      ))}
    </>
  );
}

/**
 * An overlay in a canvas-filling wrapper whose only difference between the
 * variants is its layer, so both place the overlay alike.
 */
function Overlay({
  id,
  layer,
  children,
}: {
  readonly id: string;
  readonly layer: number;
  readonly children: ReactNode;
}) {
  return (
    <Entity id={`overlay/${id}`}>
      <Layout
        kind={STACK}
        width={EXTENT[0]}
        height={EXTENT[1]}
        align_x={-1}
        align_y={-1}
      />
      <Style layer={layer} />
      <Children>{children}</Children>
    </Entity>
  );
}

function scene({ raised, beneath, overlays }: Variant) {
  const layer = raised ? 1 : 0;
  return defineSpecimen({
    extent: EXTENT,
    states: [{ name: "scene", cell: [0, 0, EXTENT[0], EXTENT[1]] }],
    render: (lab) => (
      <>
        <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />

        {/* A modal dialog over the labels painted before it. */}
        <Lines
          id="dialog-labels"
          at={[16, 16]}
          count={5}
          text="LABEL UNDER DIALOG"
          font={lab.font}
          visible={beneath}
        />
        {overlays && (
          <Overlay id="dialog" layer={layer}>
            <Panel
              id="dialog/panel"
              layout={{
                ...placed([DIALOG[0], DIALOG[1]], DIALOG[2]),
                height: DIALOG[3],
              }}
            >
              <PanelHeader id="dialog/header" title="DIALOG" />
              <TextLine
                id="dialog/body"
                text="Its own text"
                layout={{ margin_top: 12, margin_left: 12 }}
              />
            </Panel>
          </Overlay>
        )}

        {/* A toast over lines crossing its frame, mark and close button. */}
        <Lines
          id="toast-lines"
          at={[244, 34]}
          count={4}
          text="TEXT UNDER THE TOAST 0123"
          font={lab.font}
          visible={beneath}
          pitch={18}
        />
        {overlays && (
          <Overlay id="toast" layer={layer}>
            <Toast
              id="toast/toast"
              severity="success"
              text="Saved"
              persistent
              layout={placed(TOAST_AT, TOAST_WIDTH)}
            />
          </Overlay>
        )}

        {/* A panel stacked on another panel's text. */}
        <Panel
          id="first"
          layout={{
            ...placed([FIRST[0], FIRST[1]], FIRST[2]),
            height: FIRST[3],
          }}
        >
          <PanelHeader id="first/header" title="FIRST" />
        </Panel>
        <Lines
          id="first-lines"
          at={[28, 192]}
          count={3}
          text="TEXT OF THE FIRST PANEL"
          font={lab.font}
          visible={beneath}
        />
        {overlays && (
          <Overlay id="second" layer={layer}>
            <Panel
              id="second/panel"
              layout={{
                ...placed([SECOND[0], SECOND[1]], SECOND[2]),
                height: SECOND[3],
              }}
            >
              <PanelHeader id="second/header" title="SECOND" />
            </Panel>
          </Overlay>
        )}

        {/* A raised layer declared before the lines it covers. */}
        {overlays && (
          <Overlay id="raised" layer={1}>
            <Fill id="raised/fill" rect={RAISED} color={RAISED_FILL} />
          </Overlay>
        )}
        <Lines
          id="raised-lines"
          at={[494, 24]}
          count={5}
          text="TEXT UNDER THE LAYER"
          font={lab.font}
          visible={beneath}
        />
      </>
    ),
  });
}

/** Whether two captures differ at capture pixel `index` by more than `tolerance`. */
function differs(
  a: RgbaImage,
  b: RgbaImage,
  index: number,
  tolerance: number,
): boolean {
  for (let channel = 0; channel < 3; channel++)
    if (
      Math.abs(a.pixels[index + channel]! - b.pixels[index + channel]!) >
      tolerance
    )
      return true;
  return false;
}

/** Capture pixel offsets of the logical rectangle `rect`. */
function* pixels(image: RgbaImage, rect: Rect): Generator<number> {
  const [x, y, width, height] = rect.map((value) =>
    Math.round(value * CAPTURE_SCALE),
  ) as [number, number, number, number];
  for (
    let row = Math.max(y, 0);
    row < Math.min(y + height, image.height);
    row++
  )
    for (
      let column = Math.max(x, 0);
      column < Math.min(x + width, image.width);
      column++
    )
      yield (row * image.width + column) * 4;
}

export interface OverlapCapture {
  readonly name: string;
  readonly image: RgbaImage;
}

/**
 * Declare the overlap scene, capture its five variants and probe them. Returns
 * every result; the caller fails the scenario on any that did not pass.
 */
export async function overlapOrder(
  host: HostClientBase<Client>,
  contract: ThemeContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  capture: (image: OverlapCapture) => Promise<void>,
): Promise<ProbeResult[]> {
  const variants = {
    declared: { raised: false, beneath: true, overlays: true },
    layered: { raised: true, beneath: true, overlays: true },
    hidden: { raised: false, beneath: false, overlays: true },
    bare: { raised: false, beneath: true, overlays: false },
    empty: { raised: false, beneath: false, overlays: false },
  } as const satisfies Record<string, Variant>;
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: scene(variants.declared),
    themes: {},
    world: "gui-default-skin/overlap",
  });
  const images = {} as Record<keyof typeof variants, RgbaImage>;
  try {
    for (const [name, variant] of Object.entries(variants) as [
      keyof typeof variants,
      Variant,
    ][]) {
      await session.update({ specimen: scene(variant) });
      images[name] = (await session.capture(present)).image;
      await capture({ name: `overlap-${name}`, image: images[name] });
    }
  } finally {
    await session.close();
  }

  const results: ProbeResult[] = [];
  const probe = (name: string, passed: boolean, detail: string) =>
    results.push({ name, passed, detail });
  const { declared, layered, hidden, bare, empty } = images;

  // Layers paint the same order without depending on cut runs: any text drawn
  // over a shape painted after it would differ here.
  let changed = 0;
  for (const index of pixels(declared, [0, 0, ...EXTENT]))
    if (differs(declared, layered, index, 2)) changed++;
  probe(
    "the declared scene paints as its layered twin",
    changed === 0,
    `${changed} pixels differ`,
  );

  for (const overlap of CASES) {
    // Text pixels are where the bare scene differs from the empty one, and
    // overlay pixels where the hidden scene does.
    let under = 0;
    let beside = 0;
    for (const index of pixels(declared, overlap.region)) {
      if (!differs(bare, empty, index, 24)) continue;
      if (differs(hidden, empty, index, 2)) under++;
      else beside++;
    }
    probe(
      `${overlap.name}: text lies under the overlay and beside it`,
      under >= 40 && beside >= 40,
      `${under} text pixels under the overlay, ${beside} beside it`,
    );
    if (overlap.opaque) {
      let shown = 0;
      let covered = 0;
      for (const index of pixels(declared, overlap.opaque)) {
        if (differs(bare, empty, index, 24)) covered++;
        if (differs(declared, hidden, index, 2)) shown++;
      }
      probe(
        `${overlap.name}: no text shows through the overlay`,
        shown === 0 && covered >= 40,
        `${shown} pixels show text over ${covered} text pixels it covers`,
      );
    }
  }
  return results;
}
