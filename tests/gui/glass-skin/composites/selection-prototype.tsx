/** Two real native composites at the explicit source sheet's pixel scale. */
import { Entity, Children } from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import { SegmentedControl, Menu, Floating } from "@ipp/react/gui-kit";
import { GlassGuiKit, glassColor } from "@ipp/react/gui-glass";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import {
  defineSpecimen,
  type SpecimenContext,
} from "../../skin-lab/specimen.js";
import type { GlassLight } from "../../../../packages/ipp-react/src/gui-glass/types.js";
import type { ThemeContract } from "../../skin-lab/theme.js";

export const PROTOTYPE_RECTS = {
  segment: [20, 20, 377, 61],
  row: [22, 174, 404, 56],
} as const;
export function selectionPrototype(
  contract: GlassKitContract & ThemeContract,
  light: Partial<GlassLight> = {},
) {
  const tokens = {
    controlHeight: 48.8,
    row: 44.8,
    cut: 7.2,
    partCut: 7.2,
    icon: 32,
  };
  const p = glassColor("#001421");
  return defineSpecimen({
    extent: [450, 230],
    states: [
      { name: "selected-segment", cell: [0, 0, 220, 55] },
      {
        name: "hovered-menu",
        cell: [0, 65, 225, 155],
        pin: [{ kind: "hover", at: [90, 102] }],
      },
    ],
    render: (lab: SpecimenContext) => (
      <GlassGuiKit
        contract={contract}
        tokens={tokens}
        font={lab.font}
        fontSize={10}
        palette={{ primary: "#00D9FF" }}
        light={light}
        reducedMotion
      >
        <Entity id="prototype-page">
          <Layout width={450} height={230} align_x={-1} align_y={-1} />
          <Style red={p[0]} green={p[1]} blue={p[2]} alpha={1} />
          <Box width={450} height={230} />
        </Entity>
        <Entity id="prototype-segment-anchor">
          <Layout
            kind={3}
            width={188.5}
            height={30.5}
            margin_left={10}
            margin_top={10}
            align_x={-1}
            align_y={-1}
          />
          <Children>
            <SegmentedControl
              id="prototype-segments"
              defaultValue="activity"
              options={[
                { value: "overview", label: "Overview" },
                { value: "activity", label: "Activity" },
                { value: "history", label: "History" },
              ]}
            />
          </Children>
        </Entity>
        <Entity id="prototype-menu-anchor">
          <Layout
            width={212}
            height={0}
            margin_left={10}
            margin_top={60}
            align_x={-1}
            align_y={-1}
          />
          <Children>
            <Floating
              id="prototype-menu-frame"
              mode="manual"
              open
              layout={{ width: 212 }}
            >
              <Menu
                id="prototype-menu"
                layout={{ width: 212 }}
                items={[
                  {
                    key: "open",
                    label: "Open",
                    icon: "\u{F018F}",
                  },
                  {
                    key: "duplicate",
                    label: "Duplicate",
                    icon: "\u{F018F}",
                  },
                  { key: "rename", label: "Rename" },
                  { key: "delete", label: "Delete", disabled: true },
                ]}
              />
            </Floating>
          </Children>
        </Entity>
      </GlassGuiKit>
    ),
  });
}
