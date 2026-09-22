/** Shared retained decoration and verified glyphs from Shure Tech Mono Nerd Font. */
import {
  Stack,
  Text,
  type GuiNodeProps,
  type GuiThemeLaneStyle,
} from "@ipp/react/gui";
import type { ClientAssetSource } from "@ipp/client";

export const GUI_ICONS = {
  cube: "\uf1b2", // nf-fa-cube
  signal: "\uf012", // nf-fa-signal
  pulse: "\ueb31", // nf-cod-pulse
  aurora: "\uf2dc", // nf-fa-snowflake_o
  ember: "\uf06d", // nf-fa-fire
  neon: "\uf0e7", // nf-fa-flash
} as const;

export function Shape({
  x = 0,
  y = 0,
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
      // Counterbalanced margins place decorations without shrinking the stack.
      margin={[y, -x, -y, x]}
      backgroundColor={color}
      theme={theme}
    />
  );
}

export function Icon({
  font,
  glyph,
  size,
  color,
}: {
  font: ClientAssetSource;
  glyph: string;
  size: number;
  color: readonly [number, number, number, number];
}) {
  return (
    <Text
      text={glyph}
      asset={font}
      width={size}
      height={size}
      fontSize={size}
      color={color}
      enabled={false}
    />
  );
}
