import { useMemo } from "react";
import { Children, Entity } from "../components.js";
import { Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGlassSkin } from "./skin.js";

/** Inert glass background on the caller's laid-out Entity; it adds no hit target. */
export function GlassPanel() {
  return <Skin theme={useGlassSkin().theme("panel")} />;
}

export interface GlassProgressProps {
  readonly id: string;
  readonly value: number;
  readonly width?: number;
  readonly height?: number;
}

/** Determinate read-only progress, sized in the enclosing World's logical units. */
export function GlassProgress({
  id,
  value,
  width = 240,
  height = 12,
}: GlassProgressProps) {
  const glass = useGlassSkin();
  if (
    !Number.isFinite(value) ||
    !Number.isFinite(width) ||
    !Number.isFinite(height) ||
    width <= 0 ||
    height <= 0
  )
    throw new RangeError(
      "GlassProgress needs finite progress and positive dimensions",
    );
  const track = useMemo(
    () =>
      glass.contract.GuiTheme.encodeParts({
        nextSlot: 1,
        rows: new Map([
          [
            0,
            {
              part: glass.contract.guiPaintPartIndex({ part: "background" }),
              color: glass.palette.surface,
              paint_data: [0, 0, 0, 0],
            },
          ],
        ]),
      }),
    [glass.contract, glass.palette],
  );
  const fraction = Math.max(0, Math.min(value, 1));
  return (
    <Entity id={id}>
      <Layout kind={3} width={width} height={height} />
      <Skin theme={glass.theme("progress")} parts={track} />
      <Children>
        {fraction > 0 && (
          <Entity id={`${id}/fill`}>
            <Layout
              kind={0}
              width={width * fraction}
              height={height}
              align_x={-1}
              align_y={-1}
            />
            <Skin theme={glass.theme("progress")} />
          </Entity>
        )}
      </Children>
    </Entity>
  );
}
