/**
 * Measured completion of a task as a ring: a quiet track and a lit arc from
 * twelve o'clock clockwise through the reported fraction, never ahead of it,
 * with the floored percentage in the centre and the task below. While the
 * task runs, a short arc just ahead of the lit one fades in and out on the
 * Host clock, steady under the kit's reduced motion, as the progress bar's
 * leading section does; the lit arc shares the ring with it, so at zero it
 * starts at twelve o'clock and it always fits. Complete fills the ring and
 * carries the check mark in the centre, with 100% beside the task; failed
 * keeps the reached arc in the error colour, with the error icon in the
 * centre and Failed beside the task.
 * Without a value the total is unknown: the spinner's lit quarter turns on the
 * Host clock, held still under reduced motion, and no percentage shows; the
 * task should then describe the work. Idle, before a task starts or between
 * tasks, the ring is the quiet track alone, with no arc and no percentage,
 * above the task's name. The ring is read-only: never focusable, with no
 * handle and no hit target.
 *
 * The ring is 128 units across, or 64 at the small size. The component is a
 * fixed-height column as wide as the wider of its ring and caption, which it
 * centres on each other.
 */
import { Children, Entity } from "../../components.js";
import { Style, Font, Layout } from "../../gui/components.js";
import { Arc, TURNING_SWEEP } from "../arc.js";
import { LEAD } from "../row-motion.js";
import { useGuiKit, type GuiKitTone, type GuiKitTypeSize } from "../kit.js";
import { LAYOUT_COLUMN, Row, type GuiKitLayout } from "../layout.js";
import {
  CheckMark,
  Icon,
  TextLine,
  glyphSizeForWidth,
  lineWidth,
  percentage,
  textWidth,
} from "../text.js";
import type { KitThemeName } from "../themes.js";

export type CircularProgressStatus = "complete" | "failed";

export type CircularProgressSize = "large" | "small";

export interface CircularProgressProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  /** The task, such as Uploading. */
  readonly label: string;
  /** Reported completion in 0..1; omit it while the total is unknown. */
  readonly value?: number;
  /** The task's outcome once it has one. */
  readonly status?: CircularProgressStatus;
  /**
   * Nothing is running: the quiet track alone, without an arc or a
   * percentage. A value or an outcome given with it is not shown.
   */
  readonly idle?: boolean;
  /** `large` by default. */
  readonly size?: CircularProgressSize;
  readonly layout?: GuiKitLayout;
}

/**
 * Ring diameter and thickness, the centre symbol's side and the percentage's
 * type size, at the tokens' `em`.
 */
const SIZES: Readonly<
  Record<
    CircularProgressSize,
    {
      diameter: number;
      thickness: number;
      symbol: number;
      readout: GuiKitTypeSize;
    }
  >
> = {
  large: { diameter: 128, thickness: 12, symbol: 48, readout: "display" },
  small: { diameter: 64, thickness: 8, symbol: 24, readout: "body" },
};

/**
 * The error icon's width against the symbol's side: a filled glyph carries
 * more ink than the stroked check, so it is drawn smaller to weigh the same.
 */
const FILLED = 2 / 3;

/** Space between the ring and its caption. */
const CAPTION_GAP = 8;

const OUTCOMES: Readonly<
  Record<
    CircularProgressStatus,
    { arc: KitThemeName; tone: GuiKitTone; word: string }
  >
> = {
  complete: { arc: "arcAccent", tone: "accent", word: "100%" },
  failed: { arc: "arcError", tone: "error", word: "Failed" },
};

export function CircularProgress({
  id,
  layer = 0,
  label,
  value,
  status,
  idle = false,
  size = "large",
  layout,
}: CircularProgressProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const { diameter, thickness, symbol, readout } = SIZES[size];
  const outcome = !idle && status ? OUTCOMES[status] : undefined;
  const fraction = idle
    ? undefined
    : status === "complete"
      ? 1
      : value === undefined
        ? undefined
        : Math.min(Math.max(value, 0), 1);
  const unknown = !idle && fraction === undefined && !status;
  // While running, the leading arc takes the leading section's length of the
  // ring's middle circle, and the lit arc the reported share of the rest.
  const running = fraction !== undefined && !outcome;
  const lead = running ? LEAD / (Math.PI * (diameter - thickness)) : 0;
  const gap = t.inset / 2;
  const body = kit.typeSize("body");
  const caption = outcome
    ? textWidth(label, body) + kit.unit(gap) + lineWidth(outcome.word, body)
    : lineWidth(label, body);

  const centre =
    outcome && status === "complete" ? (
      <CheckMark id={`${id}/symbol`} size={symbol} layout={{ align_x: 0 }} />
    ) : outcome && status === "failed" ? (
      <Icon
        id={`${id}/symbol`}
        icon="error"
        tone="error"
        size={glyphSizeForWidth(symbol * FILLED)}
        layout={{ align_x: 0 }}
      />
    ) : (
      fraction !== undefined && (
        // Floor, so the readout reaches 100% only when the task reports it.
        <TextLine
          id={`${id}/readout`}
          text={percentage(fraction)}
          size={readout}
          layout={{ align_x: 0 }}
        />
      )
    );

  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        width={Math.max(kit.unit(diameter), caption)}
        height={kit.unit(diameter + CAPTION_GAP + t.denseRow)}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Arc
          id={`${id}/ring`}
          theme="arcTrack"
          size={diameter}
          thickness={thickness}
        >
          {unknown ? (
            <Arc
              id={`${id}/value`}
              theme="arcAccent"
              size={diameter}
              thickness={thickness}
              sweep={TURNING_SWEEP}
              turning
            />
          ) : (
            fraction !== undefined &&
            fraction > 0 && (
              <Arc
                id={`${id}/value`}
                theme={outcome?.arc ?? "arcAccent"}
                size={diameter}
                thickness={thickness}
                sweep={fraction * (1 - lead)}
              />
            )
          )}
          {running && fraction !== undefined && (
            <Arc
              id={`${id}/lead`}
              theme="arcAccent"
              size={diameter}
              thickness={thickness}
              start={fraction * (1 - lead)}
              sweep={lead}
              cue
            />
          )}
          {centre}
        </Arc>
        <Row
          id={`${id}/caption`}
          height={t.denseRow}
          layout={{
            width: caption,
            align_x: 0,
            margin_top: kit.unit(CAPTION_GAP),
          }}
        >
          <TextLine id={`${id}/label`} text={label} />
          {outcome && (
            <TextLine
              id={`${id}/outcome`}
              text={outcome.word}
              tone={outcome.tone}
              layout={{ margin_left: kit.unit(gap) }}
            />
          )}
        </Row>
      </Children>
    </Entity>
  );
}
