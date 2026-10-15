import { Entity } from "@ipp/react";
import { Font, Layout } from "@ipp/react/gui";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import { GlassSwitch } from "../../../../packages/ipp-react/src/gui-glass/values.js";
import type { GlassContract } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";
import { SpecimenCaption } from "./caption.js";

const sources = [
  { name: "unlit", x: 0.5, y: 0.5, intensity: 0 },
  { name: "upper-left", x: -0.15, y: -0.25, intensity: 1.8 },
  { name: "upper-right", x: 1.15, y: -0.25, intensity: 1.8 },
  { name: "lower-right", x: 1.15, y: 1.25, intensity: 1.8 },
] as const;

/** The same native controls under a point source at three local positions. */
export default function switchLightReference(contract: GlassContract) {
  return defineSpecimen({
    extent: [320, 352],
    states: sources.flatMap(({ name }, row) =>
      [0, 1].map((column) => ({
        name: `${name}-${column ? "focus" : "hover"}`,
        cell: [column * 160, row * 88, 160, 88] as const,
        pin: column
          ? [
              {
                kind: "action" as const,
                control: `${name}-${column}`,
                action: { kind: "focus" as const },
              },
            ]
          : [
              {
                kind: "hover" as const,
                at: [column * 160 + 80, row * 88 + 40] as const,
              },
            ],
      })),
    ),
    render: (lab) => (
      <>
        {sources.map((source, row) => (
          <GlassSkin
            key={source.name}
            id={row ? `switch-light-${row}` : "ipp-glass"}
            contract={contract}
            font={lab.font}
            reducedMotion
            light={{
              x: source.x,
              y: source.y,
              intensity: source.intensity,
              height: 0.65,
            }}
          >
            {[0, 1].map((column) => (
              <Entity key={column} id={`${source.name}-${column}`}>
                <Layout
                  kind={0}
                  width={124}
                  height={54}
                  margin_left={column * 160 + 18}
                  margin_top={row * 88 + 14}
                  align_x={-1}
                  align_y={-1}
                />
                <Font source={lab.font} font_size={16} />
                <GlassSwitch
                  checked={column === 1}
                  label=""
                  ref={lab.control(`${source.name}-${column}`)}
                />
              </Entity>
            ))}
            {[0, 1].map((column) => (
              <SpecimenCaption
                key={column}
                id={`${source.name}-${column}`}
                text={`${source.name} / ${column ? "focus" : "hover"}`}
                x={column * 160 + 18}
                y={row * 88 + 74}
                width={144}
              />
            ))}
          </GlassSkin>
        ))}
      </>
    ),
  });
}
