import { Entity } from "@ipp/react";
import { Behavior, Layout } from "@ipp/react/gui";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import { GlassSwitch } from "../../../../packages/ipp-react/src/gui-glass/values.js";
import type { GlassContract } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";

const names = ["idle", "hover", "pressed", "focus", "disabled"] as const;
export default function switchReference(contract: GlassContract) {
  return defineSpecimen({
    extent: [380, 41],
    states: names.map((name, i) => ({
      name,
      cell: [i * 76, 0, 76, 41],
      ...(name === "hover"
        ? { pin: [{ kind: "hover" as const, at: [i * 76 + 38, 20] as const }] }
        : {}),
      ...(name === "pressed"
        ? { pin: [{ kind: "press" as const, at: [i * 76 + 38, 20] as const }] }
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
    })),
    render(lab) {
      return (
        <GlassSkin
          contract={contract}
          font={lab.font}
          fontSize={16}
          reducedMotion
        >
          {names.map((name, i) => (
            <Entity key={name} id={name}>
              <Layout
                kind={0}
                width={62}
                height={27}
                margin_left={i * 76 + 7}
                margin_top={6.5}
                align_x={-1}
                align_y={-1}
              />
              <Behavior enabled={name !== "disabled"} />
              <GlassSwitch checked={i >= 2} label="" ref={lab.control(name)} />
            </Entity>
          ))}
        </GlassSkin>
      );
    },
  });
}
