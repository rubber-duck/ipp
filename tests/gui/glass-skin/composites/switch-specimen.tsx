/** Peer switch tracks at normal and enlarged body sizes, with native state. */
import { GuiKit, Switch } from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import { Fill, Label, SHEET_PAGE } from "../../skin-lab/kit.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";

export function switchSpecimen(contract: GlassKitContract) {
  return defineSpecimen({
    extent: [640, 272],
    states: [{ name: "switches", cell: [0, 0, 640, 272] }],
    render: (lab) => (
      <>
        <Fill id="switch-page" rect={[0, 0, 640, 272]} color={SHEET_PAGE} />
        {[
          cybertronSkin(contract),
          glassSkin(contract, { appearance: "liquid", reducedMotion: true }),
        ].map((skin, column) => (
          <GuiKit key={column} skin={skin} font={lab.font} reducedMotion>
            <Label
              id={`switch-title-${column}`}
              at={[32 + column * 320, 20]}
              font={lab.font}
              size={20}
              text={column ? "Glass" : "Cybertron"}
            />
            {[1, 2].map((scale, row) => (
              <GuiKit
                key={scale}
                skin={skin}
                font={lab.font}
                fontSize={16 * scale}
                reducedMotion
              >
                {[false, true].map((checked, state) => {
                  const id = `switch-${column}-${scale}-${state}`;
                  return (
                    <Switch
                      key={id}
                      id={id}
                      defaultChecked={checked}
                      ref={lab.control(id)}
                      layout={{
                        margin_left: 32 + column * 320 + state * 144,
                        margin_top: 80 + row * 96,
                        align_x: -1,
                        align_y: -1,
                      }}
                    />
                  );
                })}
              </GuiKit>
            ))}
          </GuiKit>
        ))}
      </>
    ),
  });
}
