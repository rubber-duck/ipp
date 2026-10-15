import { Entity } from "@ipp/react";
import { Behavior, Font, Layout, Style, Box } from "@ipp/react/gui";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import {
  GlassButton,
  GlassTextInput,
} from "../../../../packages/ipp-react/src/gui-glass/actions.js";
import type { GlassContract } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { defineSpecimen, srgb, type PinStep } from "../../skin-lab/specimen.js";
const states = ["idle", "hover", "pressed", "focus", "disabled"] as const;
export default (contract: GlassContract) =>
  defineSpecimen({
    extent: [637.5, 100],
    states: ["button", "input"].flatMap((family, row) =>
      states.map((state, index) => {
        const x = index * 127.5,
          y = row * 50,
          width = family === "button" ? 106 : 117;
        const at = [x + 5 + width - 12, y + 20] as const;
        const pin: PinStep[] =
          state === "hover"
            ? [{ kind: "hover", at }]
            : state === "pressed"
              ? [{ kind: "press", at }]
              : state === "focus"
                ? [
                    {
                      kind: "action",
                      control: `${family}-${state}`,
                      action: { kind: "focus" },
                    },
                  ]
                : [];
        if (family === "input" && state === "focus")
          pin.splice(
            0,
            pin.length,
            { kind: "click", at },
            { kind: "selectText", start: 11, end: 11 },
            { kind: "hover", at: [630, 95] },
          );
        return {
          name: `${family}-${state}`,
          cell: [
            x,
            y,
            family === "button" ? 117.5 : 127,
            family === "button" ? 41 : 40,
          ] as const,
          pin,
        };
      }),
    ),
    render: (lab) => (
      <GlassSkin
        contract={contract}
        font={lab.font}
        fontSize={10}
        reducedMotion
      >
        <Entity id="background">
          <Layout
            kind={0}
            width={637.5}
            height={100}
            align_x={-1}
            align_y={-1}
          />
          <Style
            red={srgb("#00101a")[0]}
            green={srgb("#00101a")[1]}
            blue={srgb("#00101a")[2]}
          />
          <Box />
        </Entity>
        {["button", "input"].flatMap((family, row) =>
          states.map((state, index) => (
            <Entity key={`${family}-${state}`} id={`${family}-${state}`}>
              <Layout
                kind={0}
                width={family === "button" ? 106.5 : 117}
                height={family === "button" ? 28.5 : 29}
                margin_left={index * 127.5 + 5}
                margin_top={row * 50 + (family === "button" ? 5.5 : 5)}
                align_x={-1}
                align_y={-1}
              />
              <Font source={lab.font} font_size={10} />
              <Behavior enabled={state !== "disabled"} />
              {family === "button" ? (
                <GlassButton
                  label="Action"
                  ref={lab.control(`${family}-${state}`)}
                />
              ) : (
                <GlassTextInput
                  text="Sample text"
                  ref={lab.control(`${family}-${state}`)}
                />
              )}
            </Entity>
          )),
        )}
      </GlassSkin>
    ),
  });
