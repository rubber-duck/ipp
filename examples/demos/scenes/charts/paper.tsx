import { createElement } from "react";
import {
  Children,
  CustomMaterial,
  Entity,
  MeshInstance,
  PlotFrame2d,
  PlotFrame3d,
  Transform,
  usePlotSkin,
} from "@ipp/react";
import { Box, Paint, Style, Text } from "@ipp/react/gui";
import { CHART_CANVAS_EXTENT, type ChartSpec } from "./catalog.js";

export function ChartPaper({ spec, font }: { spec: ChartSpec; font: string }) {
  const skin = usePlotSkin();
  const title = skin?.paperPaint && skin.legend?.textColor;
  return (
    <>
      <Entity bindTo={`chart-background-${spec.id}`}>
        <Style red={0.015} green={0.025} blue={0.04} alpha={1} />
        <Box width={CHART_CANVAS_EXTENT[0]} height={CHART_CANVAS_EXTENT[1]} />
        {skin?.paperPaint && createElement(Paint, skin.paperPaint)}
      </Entity>
      <Entity bindTo={`chart-title-${spec.id}`}>
        <Style
          x={Number(spec.frame.width) - 24 - spec.title.length * 9}
          y={2}
          red={title?.[0] ?? 0}
          green={title?.[1] ?? 0.8}
          blue={title?.[2] ?? 1}
          alpha={1}
        />
        <Text text={spec.title.toUpperCase()} source={font} font_size={14} />
      </Entity>
    </>
  );
}

/** The paper surface supplies the grid only in the plane it occupies. */
export function ChartFrame({ spec }: { spec: ChartSpec }) {
  const skin = usePlotSkin();
  if (spec.component.endsWith("2d")) {
    return <PlotFrame2d {...(skin?.paperPaint && { grid_alpha: 0 })} />;
  }
  const material = skin?.baseMaterial;
  const width = Number(spec.frame.width);
  const depth = Number(spec.frame.depth);
  return (
    <>
      <PlotFrame3d floor_grid={!material} />
      {material && (
        <Children>
          <Entity id={`chart-base-${spec.id}`}>
            <Transform x={width / 2} y={-0.115} z={depth / 2} />
            <MeshInstance
              source={`ipp://mesh/cube?width=${width + 0.36}&height=0.14&length=${depth + 0.36}`}
            />
            {createElement(CustomMaterial, material)}
          </Entity>
        </Children>
      )}
    </>
  );
}
