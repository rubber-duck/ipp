import type { AnimationClipSource } from "@ipp/client";
import { Fragment } from "react";
import {
  Animation,
  AnimationAsset,
  Asset,
  Entity,
  ShaderAsset,
  PaintShader,
  assetRef,
} from "@ipp/react";
import { Box, Paint, Style, Text } from "@ipp/react/gui";
import { glassColor } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import { OPTICAL_PAINT, OPTICAL_PARAMETERS } from "./material.js";

export const OPTICAL_EXTENT = { width: 1200, height: 710, devicePixelRatio: 1 };
export const FINISHES = [
  { name: "CLEAR", roughness: 0.13, thickness: 0.8, edge: 1.0 },
  { name: "FROSTED", roughness: 0.48, thickness: 1.05, edge: 1.0 },
  { name: "SMOKED", roughness: 0.17, thickness: 2.5, edge: 1.15 },
] as const;
export const EYES = [
  { name: "FRONTAL", value: [0, 0, 8, 0] },
  { name: "OBLIQUE", value: [6, -2, 7, 0] },
  { name: "GRAZING", value: [-10, -1.8, 3, 0] },
] as const;
export const opticalRect = (column: number, row: number) =>
  [64 + column * 372, 190 + row * 162, 328, 82] as const;

export interface OpticalStudyOptions {
  readonly source?: readonly [number, number, number, number];
  readonly viewShift?: number;
  readonly accent?: string;
  readonly background?: string;
  readonly animated?: boolean;
}

function label(
  id: string,
  x: number,
  y: number,
  text: string,
  size = 17,
  color = "#B8D4E0",
) {
  const c = glassColor(color);
  return (
    <Entity id={id}>
      <Style x={x} y={y} layer={1} red={c[0]} green={c[1]} blue={c[2]} />
      <Text source={assetRef("optical/font")} text={text} font_size={size} />
    </Entity>
  );
}

export function opticalScene(
  font: Uint8Array<ArrayBuffer>,
  paintComponent: number,
  options: OpticalStudyOptions = {},
) {
  const background = glassColor(options.background ?? "#06131E");
  const accent = glassColor(options.accent ?? "#00E6FF");
  const source = options.source ?? [-3, -2.5, 5, 0.8];
  const clip: AnimationClipSource = {
    duration: 6,
    tracks: [
      {
        property: { component: paintComponent, name: "keyLight" },
        keys: [0, 1, 0].map((phase, i) => ({
          time: i * 3,
          value: {
            kind: "dynamic" as const,
            value: {
              kind: "vec4" as const,
              value: [phase * 6, phase * 3, 0, 0] as const,
            },
          },
        })),
      },
    ],
  };
  return (
    <>
      <Asset id="optical/font" kind={17} data={font} encode={(v) => v} />
      <ShaderAsset
        id="optical/paint"
        recipe={{}}
        parameters={OPTICAL_PARAMETERS}
      >
        <PaintShader>{OPTICAL_PAINT}</PaintShader>
      </ShaderAsset>
      {options.animated && <AnimationAsset id="optical/motion" clip={clip} />}
      <Entity id="page">
        <Style red={background[0]} green={background[1]} blue={background[2]} />
        <Box width={OPTICAL_EXTENT.width} height={OPTICAL_EXTENT.height} />
      </Entity>
      {label(
        "heading",
        64,
        35,
        "GLASS / OPTICAL MATERIAL STUDY",
        28,
        "#C8F3FA",
      )}
      {label(
        "subheading",
        64,
        77,
        "A tinted glass layer over a dark backing. One light. Three finishes.",
        16,
        "#758D9F",
      )}
      {FINISHES.map((finish, column) => (
        <Fragment key={finish.name}>
          {label(
            `title-${column}`,
            64 + column * 372,
            129,
            finish.name,
            22,
            "#55C7DC",
          )}
          {EYES.map((eye, row) => {
            const [x, y, width, height] = opticalRect(column, row);
            const id = `sample-${column}-${row}`;
            return (
              <Fragment key={id}>
                <Entity id={id}>
                  <Style x={x} y={y} />
                  <Box width={width} height={height} />
                  <Paint
                    source={assetRef("optical/paint")}
                    accent={accent}
                    tint={[0.22, 0.64, 0.82, 1]}
                    optics={[
                      1.5,
                      finish.roughness,
                      finish.thickness,
                      finish.edge,
                    ]}
                    keyLight={source}
                    eye={[
                      eye.value[0] + (options.viewShift ?? 0),
                      ...eye.value.slice(1),
                    ]}
                    state={[0.1, 0, 0, 0]}
                  />
                  {options.animated && (
                    <Animation
                      source={assetRef("optical/motion")}
                      looping
                      playing
                    />
                  )}
                </Entity>
                {label(
                  `button-label-${column}-${row}`,
                  x + 95,
                  y + 28,
                  "CONTINUE",
                  24,
                  "#D6E8EC",
                )}
                {label(
                  `view-label-${column}-${row}`,
                  x,
                  y + 104,
                  `${eye.name} / roughness ${finish.roughness}`,
                  14,
                  "#839AAB",
                )}
              </Fragment>
            );
          })}
        </Fragment>
      ))}
      {label(
        "footer",
        64,
        668,
        "Explicit view vectors in this study / analytic reflections / no scene capture or blur pass",
        14,
        "#758D9F",
      )}
    </>
  );
}
