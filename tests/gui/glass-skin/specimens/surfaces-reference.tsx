import { Entity, Children } from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import {
  GlassPanel,
  GlassProgress,
} from "../../../../packages/ipp-react/src/gui-glass/surfaces.js";
import type { GlassContract } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { defineSpecimen, srgb } from "../../skin-lab/specimen.js";

const cases = [
  { name: "cyan", palette: {}, light: {}, back: "#00101b" },
  {
    name: "violet",
    palette: { primary: "#CF9CFF", secondary: "#785CFF" },
    light: {},
    back: "#00101b",
  },
  {
    name: "light-left",
    palette: {},
    light: { x: -0.2, y: 0.1, intensity: 1.8 },
    back: "#00101b",
  },
  {
    name: "light-right",
    palette: {},
    light: { x: 1.2, y: 0.9, intensity: 1.8 },
    back: "#00101b",
  },
  { name: "contrast", palette: {}, light: {}, back: "#607080" },
] as const;

export default (contract: GlassContract) =>
  defineSpecimen({
    extent: [1000, 230],
    states: cases.map((item, i) => ({
      name: item.name,
      cell: [i * 200, 0, 200, 230] as const,
      pin: [{ kind: "press" as const, at: [i * 200 + 90, 38] as const }],
    })),
    render: (lab) => (
      <>
        {cases.map((item, i) => (
          <GlassSkin
            key={item.name}
            id={`ipp-glass${i ? `-${i}` : ""}`}
            contract={contract}
            font={lab.font}
            reducedMotion
            palette={item.palette}
            light={item.light}
          >
            <Entity id={`${item.name}/background`}>
              <Layout
                kind={0}
                width={200}
                height={230}
                margin_left={i * 200}
                align_x={-1}
                align_y={-1}
              />
              <Style
                red={srgb(item.back)[0]}
                green={srgb(item.back)[1]}
                blue={srgb(item.back)[2]}
                alpha={1}
              />
              <Box width={200} height={230} />
            </Entity>
            <Entity id={`${item.name}/panel`}>
              <Layout
                kind={0}
                width={180}
                height={62}
                margin_left={i * 200 + 10}
                margin_top={10}
                align_x={-1}
                align_y={-1}
              />
              <GlassPanel />
            </Entity>
            {[0, 0.01, 0.35, 1, -0.5, 1.5].map((value, j) => (
              <Entity key={j} id={`${item.name}/row-${j}`}>
                <Layout
                  kind={0}
                  width={180}
                  height={12}
                  margin_left={i * 200 + 10}
                  margin_top={86 + j * 20}
                  align_x={-1}
                  align_y={-1}
                />
                <Children>
                  <GlassProgress
                    id={`${item.name}/progress-${j}`}
                    value={value}
                    width={180}
                    height={8}
                  />
                </Children>
              </Entity>
            ))}
            <Entity id={`${item.name}/narrow-row`}>
              <Layout
                kind={0}
                width={20}
                height={5}
                margin_left={i * 200 + 10}
                margin_top={214}
                align_x={-1}
                align_y={-1}
              />
              <Children>
                <GlassProgress
                  id={`${item.name}/narrow`}
                  value={0.05}
                  width={20}
                  height={3}
                />
              </Children>
            </Entity>
          </GlassSkin>
        ))}
      </>
    ),
  });
