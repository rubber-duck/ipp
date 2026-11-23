import { Entity } from "../components.js";
import { Layout, Style, Text } from "../gui/components.js";
import { OverlayStack, type GuiKitLayoutProps } from "../gui-kit/layout.js";
import { useGuiKit } from "../gui-kit/kit.js";
import { useEntityId } from "../scope/identity.js";
import { gold, ink } from "./palette.js";

/** A short display title with a solid ink outline and a lower shadow. */
export function CartoonTitle({
  id: providedId,
  text,
  fontSize,
  ...layout
}: GuiKitLayoutProps & { id?: string; text: string; fontSize?: number }) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const size = fontSize ?? kit.typeSize("display");
  const stroke = size / 24;
  // Normalized offsets leave room for the outline on every side. The native
  // text leaves share one font asset and measure the stack from the glyphs.
  const layers = [
    ["shadow", 1, 3],
    ["left", 0, 1],
    ["right", 2, 1],
    ["top", 1, 0],
    ["bottom", 1, 2],
    ["face", 1, 1],
  ] as const;
  return (
    <OverlayStack id={id} fitWidth fitHeight {...layout}>
      {layers.map(([name, x, y]) => {
        const [red, green, blue] = name === "face" ? gold : ink;
        return (
          <Entity key={name} id={`${id}/${name}`}>
            <Layout
              align_x={-1}
              align_y={-1}
              margin_left={x * stroke}
              margin_top={y * stroke}
            />
            <Style red={red} green={green} blue={blue} />
            <Text text={text} source={kit.font} font_size={size} />
          </Entity>
        );
      })}
    </OverlayStack>
  );
}
