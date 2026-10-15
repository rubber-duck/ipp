import { Children, Entity } from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import { Knob, ProgressBar } from "@ipp/react/gui-kit";
import { GlassGuiKit } from "@ipp/react/gui-glass";
import { defineSpecimen } from "../../skin-lab/specimen.js";
import type { valuesSpecimen } from "./values-specimen.js";

/** Bounded visual study: one real dial and one real progress bar. */
export function valuesPrototype(
  contract: Parameters<typeof valuesSpecimen>[0],
) {
  return defineSpecimen({
    extent: [260, 300],
    states: [{ name: "prototype", cell: [0, 0, 260, 300] }],
    render: (lab) => (
      <>
        <Entity id="prototype-background">
          <Layout kind={0} width={260} height={300} />
          <Style red={0.001} green={0.008} blue={0.016} />
          <Box width={260} height={300} />
        </Entity>
        <GlassGuiKit
          contract={contract}
          font={lab.font}
          fontSize={16}
          reducedMotion
          light={{ x: 0.3, y: 0.2, intensity: 1 }}
        >
          <Entity id="prototype-knob">
            <Layout
              kind={3}
              width={260}
              height={170}
              align_x={-1}
              align_y={-1}
              padding_left={72}
              padding_top={20}
            />
            <Children>
              <Knob
                id="prototype-dial"
                min={-100}
                max={100}
                step={1}
                origin={0}
                defaultValue={35}
                size={116}
                bounds={false}
                ref={lab.control("dial")}
              />
            </Children>
          </Entity>
        </GlassGuiKit>
        <GlassGuiKit
          id="prototype-progress-glass"
          contract={contract}
          font={lab.font}
          fontSize={7}
          reducedMotion
          light={{ x: 0.3, y: 0.2, intensity: 1 }}
        >
          <Entity id="prototype-progress">
            <Layout
              kind={3}
              width={260}
              height={100}
              margin_top={200}
              align_x={-1}
              align_y={-1}
              padding_left={26}
              padding_right={53}
            />
            <Children>
              <ProgressBar id="prototype-bar" label="Progress" value={0.62} />
            </Children>
          </Entity>
        </GlassGuiKit>
      </>
    ),
  });
}
