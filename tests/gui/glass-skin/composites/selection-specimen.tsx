import type { ReactNode } from "react";
import type { ThemeContract } from "../../skin-lab/theme.js";
import { Entity, Children } from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import {
  RadioGroup,
  SegmentedControl,
  Tabs,
  TreeView,
  OptionList,
  Dropdown,
  SearchableDropdown,
  MultiSelect,
  Autocomplete,
  TextLine,
} from "@ipp/react/gui-kit";
import { GlassGuiKit } from "@ipp/react/gui-glass";
import { resolveGlassPalette } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import type {
  GlassLight,
  GlassPaletteInput,
} from "../../../../packages/ipp-react/src/gui-glass/types.js";
import {
  defineSpecimen,
  type SpecimenContext,
} from "../../skin-lab/specimen.js";

export const SELECTION_EXTENT = [900, 640] as const;
const options = [
  { key: "blue", label: "Blue" },
  { key: "amber", label: "Amber" },
  { key: "off", label: "Unavailable", disabled: true },
];
const choices = options.map(({ key, ...option }) => ({
  value: key,
  ...option,
}));
function Cell({
  id,
  x,
  y,
  width = 270,
  height = 120,
  children,
}: {
  id: string;
  x: number;
  y: number;
  width?: number;
  height?: number;
  children: ReactNode;
}) {
  return (
    <Entity id={id}>
      <Layout
        kind={2}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Children>{children}</Children>
    </Entity>
  );
}
export function selectionSpecimen(
  contract: GlassKitContract & ThemeContract,
  palette: Partial<GlassPaletteInput> = {},
  light: Partial<GlassLight> = {},
) {
  const referencePalette = { primary: "#00D9FF", ...palette };
  const tokens = {
    controlHeight: 48.8,
    row: 44.8,
    icon: 32,
    cut: 7.2,
    partCut: 7.2,
  };
  const background = resolveGlassPalette(referencePalette).background;
  return defineSpecimen({
    extent: SELECTION_EXTENT,
    states: [{ name: "selection", cell: [0, 0, ...SELECTION_EXTENT] }],
    render: (lab: SpecimenContext) => (
      <GlassGuiKit
        contract={contract}
        tokens={tokens}
        font={lab.font}
        fontSize={10}
        palette={referencePalette}
        light={light}
        reducedMotion
      >
        <Entity id="selection-page">
          <Layout width={900} height={640} align_x={-1} align_y={-1} />
          <Style
            red={background[0]}
            green={background[1]}
            blue={background[2]}
            alpha={1}
          />
          <Box width={900} height={640} />
        </Entity>
        <Entity id="selection-content">
          <Layout kind={3} width={900} height={640} align_x={-1} align_y={-1} />
          <Children>
            <Cell id="radio-cell" x={20} y={20}>
              <RadioGroup
                id="glass-radio"
                label="RADIO"
                options={choices}
                defaultValue="blue"
                horizontal
              />
            </Cell>
            <Cell id="segment-cell" x={320} y={20}>
              <TextLine id="segments-title" text="SEGMENTS" />
              <SegmentedControl
                id="glass-segments"
                options={choices}
                defaultValue="blue"
              />
            </Cell>
            <Cell id="tabs-cell" x={620} y={20}>
              <Tabs
                id="glass-tabs"
                defaultValue="blue"
                tabs={choices.map((o) => ({
                  ...o,
                  content: (
                    <TextLine
                      id={`tab-${o.value}-content`}
                      text={`${o.label} content`}
                    />
                  ),
                }))}
                layout={{ height: 100 }}
              />
            </Cell>
            <Cell id="tree-cell" x={20} y={170} height={180}>
              <TreeView
                id="glass-tree"
                nodes={[
                  {
                    key: "root",
                    label: "Scene",
                    children: [
                      { key: "mesh", label: "Mesh" },
                      { key: "light", label: "Light" },
                    ],
                  },
                  { key: "off", label: "Unavailable", disabled: true },
                ]}
                defaultExpanded={["root"]}
                defaultValue="mesh"
                layout={{ height: 180 }}
              />
            </Cell>
            <Cell id="options-cell" x={320} y={170} height={180}>
              <OptionList
                id="glass-options"
                options={options}
                selected={["blue"]}
              />
            </Cell>
            <Cell id="dropdown-cell" x={620} y={170}>
              <TextLine id="dropdown-title" text="DROPDOWN" />
              <Dropdown
                id="glass-dropdown"
                options={options}
                defaultValue="blue"
              />
            </Cell>
            <Cell id="search-cell" x={20} y={390}>
              <TextLine id="search-title" text="SEARCHABLE" />
              <SearchableDropdown
                id="glass-search"
                options={options}
                defaultValue="blue"
              />
            </Cell>
            <Cell id="multi-cell" x={320} y={390}>
              <TextLine id="multi-title" text="MULTI SELECT" />
              <MultiSelect
                id="glass-multi"
                options={options}
                defaultValue={["blue"]}
              />
            </Cell>
            <Cell id="autocomplete-cell" x={620} y={390}>
              <TextLine id="autocomplete-title" text="AUTOCOMPLETE" />
              <Autocomplete
                id="glass-autocomplete"
                suggestions={options}
                defaultInputValue="Blue"
                ref={lab.control("autocomplete")}
              />
            </Cell>
          </Children>
        </Entity>
      </GlassGuiKit>
    ),
  });
}
