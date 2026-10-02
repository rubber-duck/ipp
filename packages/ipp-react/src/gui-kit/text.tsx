/**
 * Text leaves of the kit: one line in a tone of the palette at a size of the
 * type scale, one icon glyph, and the check mark. Each is an ordinary entity
 * vertically centred in a row by default; a text's layout box is its line box.
 */
import { Entity } from "../components.js";
import { Font, Layout, Style, Text } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { GUI_KIT_ICONS, type GuiKitIcon } from "./icons.js";
import { useGuiKit, type GuiKitTone, type GuiKitTypeSize } from "./kit.js";
import { LAYOUT_LEAF, type GuiKitLayout } from "./layout.js";

/**
 * The shared font's advance in em: every glyph of the monospaced Shure Tech
 * Mono Nerd Font, icons included. Layout boxes size containers only from
 * explicit lengths, so a kit part that hugs its text measures it with this.
 */
const FONT_ADVANCE = 0.54;

/** The width of one line of `text` at font size `size`, in the same units. */
export function textWidth(text: string, size: number): number {
  return [...text].length * FONT_ADVANCE * size;
}

/** The font size whose glyph ink, which fills the advance, is `width` wide. */
export function glyphSizeForWidth(width: number): number {
  return width / FONT_ADVANCE;
}

export interface TextLineProps {
  readonly id: string;
  readonly text: string;
  /** Palette role: accent for titles and labels, text for content, neutral for secondary. */
  readonly tone?: GuiKitTone;
  readonly size?: GuiKitTypeSize;
  readonly layout?: GuiKitLayout;
}

/** One line of text in a tone of the palette at a size of the type scale. */
export function TextLine({
  id,
  text,
  tone = "text",
  size = "body",
  layout,
}: TextLineProps) {
  const kit = useGuiKit();
  const color = kit.color(tone);
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_LEAF} align_y={0} {...layout} />
      <Style red={color[0]} green={color[1]} blue={color[2]} alpha={color[3]} />
      <Text text={text} source={kit.font} font_size={kit.typeSize(size)} />
    </Entity>
  );
}

export interface IconProps {
  readonly id: string;
  readonly icon: GuiKitIcon;
  readonly tone?: GuiKitTone;
  /** Font size at the tokens' `em`; the icon size by default. */
  readonly size?: number;
  readonly layout?: GuiKitLayout;
}

/** One icon glyph of the shared font in a tone of the palette. */
export function Icon({ id, icon, tone = "accent", size, layout }: IconProps) {
  const kit = useGuiKit();
  const color = kit.color(tone);
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_LEAF} align_y={0} {...layout} />
      <Style red={color[0]} green={color[1]} blue={color[2]} alpha={color[3]} />
      <Text
        text={GUI_KIT_ICONS[icon]}
        source={kit.font}
        font_size={kit.unit(size ?? kit.tokens.icon)}
      />
    </Entity>
  );
}

/**
 * The width a container gives one line of `text` at font size `size` when
 * the line ends at the container's own edge: the line's width and a hundredth
 * of the size, so that rounding in layout never wraps its last glyph.
 */
export function lineWidth(text: string, size: number): number {
  return textWidth(text, size) + size / 100;
}

/**
 * Side of the check mark at which its stroke is designed: the checkbox's icon
 * rectangle, half its box.
 */
export const CHECK_MARK = 16;

export interface CheckMarkProps {
  readonly id: string;
  /** Lit on the page, or the surface colour on a lit fill. */
  readonly tone?: "accent" | "surface";
  /** Side at the tokens' `em`; the checkbox's icon size by default. */
  readonly size?: number;
  readonly layout?: GuiKitLayout;
}

/**
 * The language's one check mark, the checkbox's stroke mark, in a square of
 * `size`; its own font size draws the stroke in proportion.
 */
export function CheckMark({
  id,
  tone = "accent",
  size = CHECK_MARK,
  layout,
}: CheckMarkProps) {
  const kit = useGuiKit();
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        width={kit.unit(size)}
        height={kit.unit(size)}
        align_y={0}
        {...layout}
      />
      <Font source={kit.font} font_size={(kit.fontSize * size) / CHECK_MARK} />
      <Skin theme={kit.theme(tone === "accent" ? "checkLit" : "check")} />
    </Entity>
  );
}

/**
 * A completion in 0..1 as a whole percentage, floored so that it reaches
 * 100% only when the task does, but never below the report through rounding:
 * 0.57 reads 57%, though `0.57 * 100` is 56.99999999999999.
 */
export function percentage(fraction: number): string {
  return `${Math.floor(fraction * 100 + 1e-9)}%`;
}
