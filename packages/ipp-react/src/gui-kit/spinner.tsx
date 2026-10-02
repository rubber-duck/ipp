/**
 * Activity of unknown duration beside readable status text: a ring the size
 * of an icon, a quiet track with a lit quarter that turns once a second on the
 * Host clock. It never implies a percentage, is never focusable and takes no
 * input. Under the kit's reduced motion the same symbol stands still, its arc
 * from twelve to three o'clock, beside the same text.
 *
 * The spinner turns while it is declared: remove it as soon as the operation
 * finishes, fails or is cancelled, or once it is hidden, and its animation
 * ends with it, so nothing keeps changing its canvas every frame. Its width
 * hugs its text.
 */
import { Children, Entity } from "../components.js";
import { Font, Layout } from "../gui/components.js";
import { Arc, TURNING_SWEEP } from "./arc.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_ROW, Strut, type GuiKitLayout } from "./layout.js";
import { TextLine, lineWidth } from "./text.js";

export interface SpinnerProps {
  readonly id: string;
  /** The status text, such as Preparing…. */
  readonly label: string;
  readonly layout?: GuiKitLayout;
}

/** Ring thickness: an eighth of the icon-sized ring, as the small progress ring. */
const THICKNESS = 3;

export function Spinner({ id, label, layout }: SpinnerProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const gap = t.inset / 2;
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_ROW}
        width={kit.unit(t.icon + gap) + lineWidth(label, kit.typeSize("body"))}
        height={kit.unit(t.icon)}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Strut id={`${id}/strut`} height={kit.unit(t.icon)} />
        <Arc
          id={`${id}/track`}
          theme="arcTrack"
          size={t.icon}
          thickness={THICKNESS}
        >
          <Arc
            id={`${id}/arc`}
            theme="arcAccent"
            size={t.icon}
            thickness={THICKNESS}
            sweep={TURNING_SWEEP}
            turning
          />
        </Arc>
        <TextLine
          id={`${id}/label`}
          text={label}
          layout={{ margin_left: kit.unit(gap) }}
        />
      </Children>
    </Entity>
  );
}
