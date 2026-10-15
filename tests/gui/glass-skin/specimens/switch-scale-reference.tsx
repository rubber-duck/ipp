import { Entity } from "@ipp/react";
import { Font, Layout } from "@ipp/react/gui";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import { GlassSwitch } from "../../../../packages/ipp-react/src/gui-glass/values.js";
import type { GlassContract } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";

/** Fixed font, with outline dimensions proportional to native control bounds. */
export default function switchScaleReference(contract: GlassContract) {
  const cases = [
    { scale: 1, top: 12, cell: [0, 0, 224, 48] as const },
    { scale: 2, top: 64, cell: [0, 48, 224, 84] as const },
    { scale: 3, top: 144, cell: [0, 132, 224, 108] as const },
  ];
  return defineSpecimen({
    extent: [224, 240],
    states: cases.map(({ scale, cell }) => ({
      name: `focus-${scale}x`,
      cell,
      pin: [
        {
          kind: "action" as const,
          control: `focus-${scale}x`,
          action: { kind: "focus" as const },
        },
      ],
    })),
    render: (lab) => (
      <GlassSkin contract={contract} font={lab.font} reducedMotion>
        {cases.map(({ scale, top }) => (
          <Entity key={scale} id={`focus-${scale}x`}>
            <Layout
              kind={0}
              width={62 * scale}
              height={27 * scale}
              margin_left={12}
              margin_top={top}
              align_x={-1}
              align_y={-1}
            />
            <Font source={lab.font} font_size={16} />
            <GlassSwitch
              checked
              label=""
              ref={lab.control(`focus-${scale}x`)}
            />
          </Entity>
        ))}
      </GlassSkin>
    ),
  });
}
