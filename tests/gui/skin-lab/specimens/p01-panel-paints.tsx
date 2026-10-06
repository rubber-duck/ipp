/**
 * Custom paints on panels in the default skin: the kit's container frame (the
 * `paint` theme module) with its fill painted by scanlines, by a grid inside
 * the lit frame's glow, and by a soft band that an ordinary clip on its
 * `sweep` property sweeps up the panel once, beside a plain panel and one
 * whose paint does not compile, which falls back to the panel's own colour.
 * Every pattern takes the panel colour as its input and draws its lines in the
 * accent, so the panels stay in the design language while the paint supplies
 * texture. The sweep plays once so that its end settles for a capture; the
 * frames of a moving paint are the Host's evidence rather than a state here.
 *
 * {@link PanelPaints} is shared with the maintained default-skin scenario,
 * which also holds the band still at a chosen `sweep` and moves it with a
 * property write.
 */
import type { ReactNode } from "react";
import { components } from "@ipp/host-contract";
import {
  Animation,
  AnimationAsset,
  Entity,
  PaintShader,
  ShaderAsset,
  assetRef,
} from "@ipp/react";
import { Layout, Paint } from "@ipp/react/gui";
import { Fill, Label, SHEET_PAGE } from "../kit.js";
import {
  defineSpecimen,
  type Rect,
  type SpecimenContext,
} from "../specimen.js";
import { TEXT_SMALL } from "../themes/geometry.js";
import { accent } from "../themes/palette.js";

export const EXTENT = [584, 184] as const;

/** Panels left to right; each is 96 units square below its label. */
export const PANELS = [
  "plain",
  "scanlines",
  "grid",
  "sweep",
  "fallback",
] as const;
export type PanelName = (typeof PANELS)[number];

const SIZE = 96;
const PITCH = 112;
const TOP = 56;

/** A panel's rectangle in canvas units. */
export function panelRect(name: PanelName): Rect {
  return [24 + PITCH * PANELS.indexOf(name), TOP, SIZE, SIZE];
}

/** Lines one unit and a half wide every four units, the accent at 45%. */
const SCANLINES = `
float distance = abs(fract(position.y / p_spacing) - 0.5) * p_spacing;
float footprint = max(fwidth(position.y), 1.0e-4);
float line = clamp((0.5 * p_width - distance) / footprint + 0.5, 0.0, 1.0);
return vec4(mix(color.rgb, p_line.rgb, line * p_line.a), color.a);`;

/** A square grid on the panel's own corner, its lines in the accent. */
const GRID = `
vec2 offset = abs(fract(position / p_cell + 0.5) - 0.5) * p_cell;
vec2 footprint = max(fwidth(position), vec2(1.0e-4));
vec2 lines = clamp((0.5 * p_width - offset) / footprint + 0.5, 0.0, 1.0);
return vec4(mix(color.rgb, p_line.rgb, max(lines.x, lines.y) * p_line.a), color.a);`;

/** A soft horizontal band centred `sweep` of the way down the panel. */
const SWEEP = `
float offset = (position.y / size.y - p_sweep) / p_width;
return vec4(mix(color.rgb, p_glow.rgb, exp(-offset * offset) * p_glow.a), color.a);`;

/** Names a tint it never declared, so it fails to compile alone. */
const BROKEN = "return color * p_tint;";

/**
 * Rest of the band; the clip's change of it, which animation adds to the
 * authored rest, so the band sweeps up half the panel; and the sweep's length.
 */
export const SWEEP_REST = 0.35;
export const SWEEP_CHANGE = -0.5;
const SWEEP_DURATION = 1.5;

const line = [accent[0], accent[1], accent[2], 0.45] as const;

/** The sweep clip: half the panel up from wherever the band rests. */
const SWEEP_CLIP = {
  duration: SWEEP_DURATION,
  tracks: [
    {
      property: { component: components.CanvasPaint.id, name: "sweep" },
      keys: [0, SWEEP_CHANGE].map((value, index) => ({
        time: index * SWEEP_DURATION,
        value: {
          kind: "dynamic" as const,
          value: { kind: "f32" as const, value },
        },
      })),
    },
  ],
};

/** A container panel at its rectangle in the theme `skin`, its fill painted by `children`. */
function PaintedPanel({
  name,
  skin,
  children,
}: {
  readonly name: PanelName;
  readonly skin: ReactNode;
  readonly children?: ReactNode;
}) {
  const [x, y, width, height] = panelRect(name);
  return (
    <Entity id={`panel/${name}`}>
      <Layout
        kind={0}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      {skin}
      {children}
    </Entity>
  );
}

/**
 * The five panels, their paint shaders and labels. The band rests at `sweep`
 * and, with `animated`, sweeps half the panel up once on the Host clock and
 * stays there.
 */
export function PanelPaints({
  lab,
  sweep = SWEEP_REST,
  animated = false,
}: {
  readonly lab: SpecimenContext;
  readonly sweep?: number;
  readonly animated?: boolean;
}) {
  const panel = lab.skin("panel");
  return (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {/* Assets are declared beside components, never as child entities. */}
      <Entity id="paints">
        <ShaderAsset
          id="paint/scanlines"
          recipe={{}}
          parameters={{ spacing: "f32", width: "f32", line: "vec4" }}
        >
          <PaintShader>{SCANLINES}</PaintShader>
        </ShaderAsset>
        <ShaderAsset
          id="paint/grid"
          recipe={{}}
          parameters={{ cell: "f32", width: "f32", line: "vec4" }}
        >
          <PaintShader>{GRID}</PaintShader>
        </ShaderAsset>
        <ShaderAsset
          id="paint/sweep"
          recipe={{}}
          parameters={{ sweep: "f32", width: "f32", glow: "vec4" }}
        >
          <PaintShader>{SWEEP}</PaintShader>
        </ShaderAsset>
        <ShaderAsset id="paint/broken" recipe={{}} parameters={{}}>
          <PaintShader>{BROKEN}</PaintShader>
        </ShaderAsset>
      </Entity>
      {PANELS.map((name) => {
        const [x, y] = panelRect(name);
        return (
          <Label
            key={name}
            id={`label/${name}`}
            at={[x, y - 28]}
            text={name.toUpperCase()}
            font={lab.font}
            size={TEXT_SMALL}
          />
        );
      })}
      <PaintedPanel name="plain" skin={panel} />
      <PaintedPanel name="scanlines" skin={panel}>
        <Paint
          source={assetRef("paint/scanlines")}
          spacing={4}
          width={1.5}
          line={line}
        />
      </PaintedPanel>
      <PaintedPanel name="grid" skin={lab.skin("lit")}>
        <Paint
          source={assetRef("paint/grid")}
          cell={16}
          width={1}
          line={line}
        />
      </PaintedPanel>
      <PaintedPanel name="sweep" skin={panel}>
        <Paint
          source={assetRef("paint/sweep")}
          sweep={sweep}
          width={0.12}
          glow={[accent[0], accent[1], accent[2], 0.6]}
        />
        {animated && (
          <>
            <AnimationAsset id="paint/sweep-clip" clip={SWEEP_CLIP} />
            <Animation
              source={assetRef("paint/sweep-clip")}
              target="panel/sweep"
              autoPlay
            />
          </>
        )}
      </PaintedPanel>
      <PaintedPanel name="fallback" skin={panel}>
        <Paint source={assetRef("paint/broken")} />
      </PaintedPanel>
    </>
  );
}

/** A state cell around each panel and the reach of its glow. */
const cell = (name: PanelName): Rect => {
  const [x, y, width, height] = panelRect(name);
  return [x - 8, y - 32, width + 16, height + 40];
};

export default defineSpecimen({
  extent: EXTENT,
  theme: "paint",
  failingAssets: ["paint/broken"],
  states: PANELS.map((name) => ({ name, cell: cell(name) })),
  render: (lab) => <PanelPaints lab={lab} sweep={0.85} animated />,
});
