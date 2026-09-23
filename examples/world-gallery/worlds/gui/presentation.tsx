/** Shared retained decoration and verified glyphs from Shure Tech Mono Nerd Font. */
import {
  Align,
  Padding,
  Stack,
  Text,
  type GuiNodeProps,
  type GuiThemeLaneStyle,
} from "@ipp/react/gui";
import type { ClientAssetSource } from "@ipp/client";

type Color = readonly [number, number, number, number];

export const GUI_ICONS = {
  dashboard: "", // nf-cod-dashboard
  signal: "", // nf-fa-signal
  pulse: "", // nf-cod-pulse
  aurora: "", // nf-fa-snowflake_o
  ember: "", // nf-fa-fire
  neon: "", // nf-fa-flash
} as const;

/**
 * A non-interactive decoration box. `x` and `y` offset it from the start of
 * its parent Stack by adding to the caller's leading margins, so the Stack
 * extent, layout bounds and painted position agree.
 */
export function Shape({
  x = 0,
  y = 0,
  margin = [0, 0, 0, 0],
  material,
  ...props
}: GuiNodeProps & {
  x?: number;
  y?: number;
  material?: GuiThemeLaneStyle;
}) {
  const { color = [0, 0, 0, 0], ...appearance } = material ?? {};
  // Plain strips need only a fill, without redundant named material properties.
  const theme = Object.keys(appearance).length
    ? { parts: { background: { base: appearance } } }
    : undefined;
  return (
    <Stack
      {...props}
      enabled={false}
      margin={[margin[0] + y, margin[1], margin[2], margin[3] + x]}
      backgroundColor={color}
      theme={theme}
    />
  );
}

/**
 * A glyph measured at its intrinsic line box: its advance wide and one line
 * tall. The Nerd Font centres each icon's ink in that box, so an enclosing
 * Align centres the ink itself. Choose `fontSize` by ink width: every icon
 * advance is 0.54 em, and the 1.127 em line must fit the cell height, or
 * Align clamps the box and pins it to the top.
 */
export function Icon({
  font,
  glyph,
  fontSize,
  color,
}: {
  font: ClientAssetSource;
  glyph: string;
  fontSize: number;
  color: Color;
}) {
  return (
    <Text
      text={glyph}
      asset={font}
      fontSize={fontSize}
      color={color}
      enabled={false}
    />
  );
}

/**
 * A glyph in a fixed, non-interactive cell. The Padding cell carries no align
 * lanes, so its parent Stack, Row or Column places it at the start; the Align
 * filling the cell positions the glyph's intrinsic box, centred by default.
 */
export function IconCell({
  width,
  height,
  alignX = 0,
  ...icon
}: {
  width: number;
  height: number;
  alignX?: number;
  font: ClientAssetSource;
  glyph: string;
  fontSize: number;
  color: Color;
}) {
  return (
    <Padding width={width} height={height} enabled={false}>
      <Align alignX={alignX} alignY={0} enabled={false}>
        <Icon {...icon} />
      </Align>
    </Padding>
  );
}
