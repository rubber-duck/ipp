import { Entity } from "@ipp/react";
import { Behavior, Box, Font, Layout, Style } from "@ipp/react/gui";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import { GlassSlider } from "../../../../packages/ipp-react/src/gui-glass/values.js";
import type { GlassContract } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { defineSpecimen, srgb } from "../../skin-lab/specimen.js";

export default (contract: GlassContract) =>
  defineSpecimen({
    extent: [610, 29],
    states: ["idle", "hover", "pressed", "focus", "disabled"].map(
      (name, index) => ({
        name,
        cell: [index * 122, 0, 122, 29] as const,
        ...(name === "hover"
          ? {
              pin: [
                { kind: "hover" as const, at: [index * 122 + 60, 13] as const },
              ],
            }
          : {}),
        ...(name === "pressed"
          ? {
              pin: [
                { kind: "press" as const, at: [index * 122 + 60, 13] as const },
              ],
            }
          : {}),
        ...(name === "focus"
          ? {
              pin: [
                {
                  kind: "action" as const,
                  control: name,
                  action: { kind: "focus" as const },
                },
              ],
            }
          : {}),
      }),
    ),
    render: (lab) => (
      <GlassSkin
        contract={contract}
        font={lab.font}
        fontSize={10}
        reducedMotion
      >
        <Entity id="back">
          <Layout kind={0} width={610} height={29} align_x={-1} align_y={-1} />
          <Style
            red={srgb("#00101b")[0]}
            green={srgb("#00101b")[1]}
            blue={srgb("#00101b")[2]}
            alpha={1}
          />
          <Box width={610} height={29} />
        </Entity>
        {["idle", "hover", "pressed", "focus", "disabled"].map(
          (name, index) => (
            <Entity key={name} id={name}>
              <Layout
                kind={0}
                width={111}
                height={20}
                margin_left={index * 122 + 5}
                margin_top={3}
                align_x={-1}
                align_y={-1}
              />
              <Font source={lab.font} font_size={10} />
              <Behavior enabled={name !== "disabled"} />
              <GlassSlider
                min={0}
                max={100}
                value={50}
                step={1}
                ref={lab.control(name)}
              />
            </Entity>
          ),
        )}
      </GlassSkin>
    ),
  });
