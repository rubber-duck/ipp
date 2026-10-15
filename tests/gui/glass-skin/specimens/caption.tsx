import { Entity } from "@ipp/react";
import { Layout, Style, Text } from "@ipp/react/gui";
import { useGlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";

/** Inert labels make exported flat references readable without the capture script. */
export function SpecimenCaption({
  id,
  text,
  x,
  y,
  width = 180,
}: {
  id: string;
  text: string;
  x: number;
  y: number;
  width?: number;
}) {
  const glass = useGlassSkin();
  const color = glass.palette.mutedText;
  return (
    <Entity id={`caption/${id}`}>
      <Layout
        kind={0}
        width={width}
        height={14}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Style red={color[0]} green={color[1]} blue={color[2]} />
      <Text
        source={glass.font}
        font_size={10}
        text={text.replaceAll("-", " ")}
      />
    </Entity>
  );
}
