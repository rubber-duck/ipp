import { Entity } from "@ipp/react";
import { Behavior, Box, Layout, Style } from "@ipp/react/gui";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import { GlassCheckbox } from "../../../../packages/ipp-react/src/gui-glass/values.js";
import type { GlassContract } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { defineSpecimen, srgb } from "../../skin-lab/specimen.js";

const feedback = ["idle", "hover", "pressed", "focus", "disabled"] as const;
const variants = ["unchecked", "checked"] as const;
const studies = [
  {
    name: "violet",
    palette: { primary: "#CF9CFF", secondary: "#785CFF" },
    background: "#00101b",
  },
  {
    name: "light-left",
    light: { x: -0.3, y: 0.2, height: 0.8, intensity: 2 },
    background: "#00101b",
  },
  {
    name: "light-right",
    light: { x: 1.3, y: 0.8, height: 0.8, intensity: 2 },
    background: "#00101b",
  },
  { name: "bright-background", background: "#FFBB66" },
] as const;

export default function checkboxReference(contract: GlassContract) {
  return defineSpecimen({
    extent: [240, 288],
    states: [
      ...variants.flatMap((variant, row) =>
        feedback.map((state, column) => {
          const name = `${variant}-${state}`;
          const at = [column * 48 + 24, row * 48 + 24] as const;
          return {
            name,
            cell: [column * 48, row * 48, 48, 48] as const,
            ...(state === "hover"
              ? { pin: [{ kind: "hover" as const, at }] }
              : {}),
            ...(state === "pressed"
              ? { pin: [{ kind: "press" as const, at }] }
              : {}),
            ...(state === "focus"
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
          };
        }),
      ),
      ...studies.flatMap((study, row) =>
        variants.map((variant, column) => ({
          name: `${study.name}-${variant}`,
          cell: [column * 48, (row + 2) * 48, 48, 48] as const,
        })),
      ),
    ],
    render(lab) {
      const background = srgb("#00101b");
      return (
        <>
          <GlassSkin
            contract={contract}
            font={lab.font}
            fontSize={16}
            reducedMotion
          >
            <Entity id="background">
              <Layout
                kind={0}
                width={240}
                height={96}
                align_x={-1}
                align_y={-1}
              />
              <Style
                red={background[0]}
                green={background[1]}
                blue={background[2]}
                alpha={1}
              />
              <Box width={240} height={96} />
            </Entity>
            {variants.flatMap((variant, row) =>
              feedback.map((state, column) => {
                const name = `${variant}-${state}`;
                return (
                  <Entity key={name} id={name}>
                    <Layout
                      kind={0}
                      width={24}
                      height={24}
                      margin_left={column * 48 + 12}
                      margin_top={row * 48 + 12}
                      align_x={-1}
                      align_y={-1}
                    />
                    <Behavior enabled={state !== "disabled"} />
                    <GlassCheckbox
                      checked={variant === "checked"}
                      label=""
                      ref={lab.control(name)}
                    />
                  </Entity>
                );
              }),
            )}
          </GlassSkin>
          {studies.map((study, row) => {
            const background = srgb(study.background);
            return (
              <GlassSkin
                key={study.name}
                id={`ipp-glass-${study.name}`}
                contract={contract}
                font={lab.font}
                fontSize={16}
                reducedMotion
                palette={"palette" in study ? study.palette : {}}
                light={"light" in study ? study.light : {}}
              >
                <Entity id={`${study.name}-background`}>
                  <Layout
                    kind={0}
                    width={240}
                    height={48}
                    margin_top={(row + 2) * 48}
                    align_x={-1}
                    align_y={-1}
                  />
                  <Style
                    red={background[0]}
                    green={background[1]}
                    blue={background[2]}
                    alpha={1}
                  />
                  <Box width={240} height={48} />
                </Entity>
                {variants.map((variant, column) => (
                  <Entity key={variant} id={`${study.name}-${variant}`}>
                    <Layout
                      kind={0}
                      width={24}
                      height={24}
                      margin_left={column * 48 + 12}
                      margin_top={(row + 2) * 48 + 12}
                      align_x={-1}
                      align_y={-1}
                    />
                    <GlassCheckbox checked={variant === "checked"} label="" />
                  </Entity>
                ))}
              </GlassSkin>
            );
          })}
        </>
      );
    },
  });
}
