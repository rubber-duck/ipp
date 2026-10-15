/** Joined edges at the default and enlarged type sizes, through native states. */
import { GuiKit, SegmentedControl, Tabs } from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import {
  Fill,
  Label,
  placed,
  SHEET_PAGE,
  SHEET_CAPTION,
} from "../../skin-lab/kit.js";
import { defineSpecimen, type PinStep } from "../../skin-lab/specimen.js";

const OPTIONS = [
  { value: "slow", label: "SLOW" },
  { value: "normal", label: "NORMAL" },
  { value: "fast", label: "FAST" },
];
const ROWS = ["selected", "hover", "pressed", "focus", "last", "only"] as const;
export const CONTOUR_EXTENT = [960, 760] as const;

export function contourSpecimen(contract: GlassKitContract) {
  return defineSpecimen({
    extent: CONTOUR_EXTENT,
    states: [
      ...["cybertron", "glass"].flatMap((skin, column) =>
        ROWS.map((name, row) => {
          const x = 28 + column * 480;
          const y = 64 + row * 96;
          const at = [x + (name === "last" ? 375 : 60), y + 30] as const;
          const pin: PinStep[] =
            name === "hover"
              ? [{ kind: "hover", at }]
              : name === "pressed"
                ? [{ kind: "press", at }]
                : name === "focus" || name === "only" || name === "last"
                  ? [
                      { kind: "click", at },
                      { kind: "key", key: name === "last" ? "end" : "home" },
                    ]
                  : [];
          return {
            name: `${skin}-${name}`,
            cell: [x - 10, y - 26, 458, 92] as const,
            pin,
          };
        }),
      ),
      { name: "tabs", cell: [0, 648, 960, 112], pin: [] },
    ],
    render: (lab) => (
      <>
        <Fill
          id="contour-page"
          rect={[0, 0, ...CONTOUR_EXTENT]}
          color={SHEET_PAGE}
        />
        {[
          cybertronSkin(contract),
          glassSkin(contract, { reducedMotion: true }),
        ].map((skin, column) => (
          <GuiKit
            key={column}
            id={`contour-kit-${column}`}
            skin={skin}
            font={lab.font}
            fontSize={24}
            reducedMotion
          >
            {ROWS.map((name, row) => (
              <SegmentedControl
                key={name}
                id={`contour-${column}-${name}`}
                options={name === "only" ? OPTIONS.slice(0, 1) : OPTIONS}
                defaultValue={
                  name === "hover" || name === "pressed"
                    ? "normal"
                    : name === "last"
                      ? "fast"
                      : "slow"
                }
                layout={placed([28 + column * 480, 64 + row * 96], 424)}
              />
            ))}
            <GuiKit fontSize={16}>
              <Tabs
                id={`contour-tabs-${column}`}
                tabs={OPTIONS}
                defaultValue="normal"
                layout={{
                  ...placed([28 + column * 480, 680], 424),
                  height: 44,
                }}
              />
            </GuiKit>
          </GuiKit>
        ))}
        {["CYBERTRON", "GLASS"].flatMap((skin, column) =>
          ROWS.map((name, row) => (
            <Label
              key={`${skin}-${name}`}
              id={`contour-label-${column}-${name}`}
              at={[28 + column * 480, 40 + row * 96]}
              text={`${skin} / ${name}`}
              font={lab.font}
              size={12}
              color={SHEET_CAPTION}
            />
          )),
        )}
      </>
    ),
  });
}
