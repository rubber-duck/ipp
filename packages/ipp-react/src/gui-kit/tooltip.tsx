/**
 * A tooltip: one or two lines of small text on the small floating surface,
 * a hint overlay of the control it explains. Declare it inside that
 * control's `Children`, closed: the runtime opens it after a delay on the
 * Host clock while the control is hovered or holds visible focus, closes it
 * after a short grace once that ends, at once when the control is pressed
 * and on Escape, and keeps one open per canvas. It takes no pointer input
 * and no focus. It sits above its control by default, a quarter inset away,
 * and flips to the other side, or shifts, to stay inside the canvas.
 *
 * Lines are dense rows of small text in a half-inset margin, so a one-line
 * tooltip is the small control height; the surface fits its longest line.
 */
import { useGuiKit } from "./kit.js";
import { Row } from "./layout.js";
import { Floating } from "./overlay.js";
import { TextLine, lineWidth } from "./text.js";

export interface TooltipProps {
  /** Symbolic id of the tooltip; its lines extend it. */
  readonly id: string;
  /** One or two lines of text. */
  readonly text: string | readonly string[];
  /** The control's side it opens on; above it by default. */
  readonly side?: "top" | "bottom" | "left" | "right";
}

export function Tooltip({ id, text, side = "top" }: TooltipProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const lines = typeof text === "string" ? [text] : text;
  const size = kit.typeSize("small");
  const margin = kit.unit(t.inset / 2);
  const gap = kit.unit(t.inset / 4);
  const offset: readonly [number, number] =
    side === "top"
      ? [0, -gap]
      : side === "bottom"
        ? [0, gap]
        : side === "left"
          ? [-gap, 0]
          : [gap, 0];
  return (
    <Floating
      id={id}
      side={side}
      align="centre"
      mode="hint"
      open={false}
      small
      offset={offset}
      layout={{
        width:
          Math.max(...lines.map((line) => lineWidth(line, size))) + 2 * margin,
        padding_left: margin,
        padding_right: margin,
        padding_top: kit.unit(t.inset / 4),
        padding_bottom: kit.unit(t.inset / 4),
      }}
    >
      {lines.map((line, index) => (
        <Row key={index} id={`${id}/line/${index}`} height={t.denseRow}>
          <TextLine id={`${id}/line/${index}/text`} text={line} size="small" />
        </Row>
      ))}
    </Floating>
  );
}
