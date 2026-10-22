import { Entity } from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import {
  Button,
  GuiKit,
  TextLine,
  type GuiKitContract,
} from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";
import { defineSpecimen } from "../../skin-lab/specimen.js";

// The 6x reference remains inside the native Host's 2048px capture surface.
export const EDGE_EXTENT = [320, 320] as const;
/** Fractional placements exercise different pixel phases at each capture density. */
export const EDGE_SAMPLES = [
  { id: "circle", x: 32.25, y: 150.25, width: 31.5, height: 31.5 },
  { id: "pill", x: 150.25, y: 150.25, width: 132.5, height: 31.5 },
  { id: "small", x: 32.25, y: 236.25, width: 18.5, height: 18.5 },
  { id: "large", x: 150.25, y: 236.25, width: 52.5, height: 52.5 },
] as const;

export function edgeQualitySpecimen(
  contract: GuiKitContract,
  backdrop?: number,
) {
  const skin = glassSkin(contract, {
    palette: { primary: "#D9B6FF", secondary: "#9360D5" },
    reducedMotion: true,
  });
  return defineSpecimen({
    extent: EDGE_EXTENT,
    states: [{ name: "edges", cell: [0, 0, ...EDGE_EXTENT] }],
    render: (lab) => (
      <>
        <Entity id="edge-backdrop">
          <Layout
            width={EDGE_EXTENT[0]}
            height={EDGE_EXTENT[1]}
            align_x={-1}
            align_y={-1}
          />
          <Style
            red={backdrop ?? 0.006}
            green={backdrop ?? 0.01}
            blue={backdrop ?? 0.016}
          />
          <Box width={EDGE_EXTENT[0]} height={EDGE_EXTENT[1]} />
        </Entity>
        <GuiKit
          id="edge-skin"
          contract={contract}
          skin={skin}
          font={lab.font}
          fontSize={14}
          reducedMotion
        >
          <TextLine
            text="GLASS / SHARED MATERIAL"
            tone="accent"
            size="display"
            layout={{ margin_left: 24, margin_top: 18, align_y: -1 }}
          />
          {[
            { x: 24, label: "1" },
            { x: 86, label: "2", selected: true },
            { x: 148, label: "3", disabled: true },
            { x: 228, label: "Continue", width: 80 },
          ].map((item, i) => (
            <Button
              key={i}
              id={`edge-action-${i}`}
              label={item.label}
              selected={item.selected ?? false}
              disabled={item.disabled ?? false}
              width={item.width ?? 36}
              height={36}
              ref={lab.control(`action-${i}`)}
              layout={{ margin_left: item.x, margin_top: 64 }}
            />
          ))}
          <TextLine
            text="CURVES / SUBPIXEL PLACEMENT"
            tone="neutral"
            size="small"
            layout={{ margin_left: 24, margin_top: 120, align_y: -1 }}
          />
          {EDGE_SAMPLES.map(({ id, x, y, width, height }) => (
            <Button
              key={id}
              id={`edge-${id}`}
              label=""
              width={width}
              height={height}
              layout={{ margin_left: x, margin_top: y }}
            />
          ))}
        </GuiKit>
      </>
    ),
  });
}
