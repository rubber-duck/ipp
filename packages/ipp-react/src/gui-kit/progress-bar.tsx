/**
 * Measured completion of a task: its label and readout above a read-only
 * frame holding a fill. The frame and fill are the switch's rail and block
 * stretched (small height, docked-height fill, the clearance between them).
 * The fill is the reported fraction, or the reported parts in order, each in
 * its tone, adjacent and never beyond the frame; the readout is their total,
 * floored, so it never shows 100% early. The run of parts keeps the part cut
 * on its outer ends only and meets in square joints.
 *
 * While the task runs, a short leading section just ahead of the fill, at the
 * start of the frame before any progress, fades in and out on the Host clock
 * so the bar never looks stopped; under the kit's reduced motion it stays,
 * steady. The fill shares the frame with it, so it always fits and the fill
 * never runs ahead of the report.
 *
 * Without a value the task's duration is unknown: a segment moves to and fro
 * across the frame on the Host clock, or rests centred in it under reduced
 * motion, and no percentage shows. Complete,
 * cancelled and failed are labelled states without the leading section:
 * complete fills the frame with its parts in proportion and carries the check
 * mark, cancelled keeps the reached fill unlit, failed keeps it in the error
 * colour beside the error icon. The bar is never focusable; a Cancel button,
 * where the task can be cancelled, is the application's.
 *
 * The bar is a fixed-height column that fills its container's width. Its
 * motion is ordinary animation, so its World selects the animation System,
 * and its canvas changes every frame while the task runs.
 */
import { useMemo, type ReactNode } from "react";
import { Animation } from "../animation.js";
import { AnimationAsset, assetRef } from "../assets.js";
import { Children, Entity } from "../components.js";
import { Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit, type GuiKitTone } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  LAYOUT_PADDING,
  LAYOUT_ROW,
  LAYOUT_STACK,
  Row,
  type GuiKitLayout,
} from "./layout.js";
import { LEAD, PULSE_REST, Pulse, useOwnRow } from "./row-motion.js";
import { CHECK_MARK, CheckMark, Icon, TextLine, percentage } from "./text.js";
import type { KitThemeName } from "./themes.js";

export type ProgressBarStatus = "complete" | "cancelled" | "failed";

/** Palette roles a part of the fill is drawn in. */
export type ProgressBarTone = "accent" | "amber" | "error" | "neutral";

/** One part of the reported completion. */
export interface ProgressBarSegment {
  /** Its share of the whole track, in 0..1. */
  readonly value: number;
  /** Accent by default. */
  readonly tone?: ProgressBarTone;
}

export interface ProgressBarProps {
  readonly id: string;
  /** The task, such as Uploading. */
  readonly label: string;
  /**
   * Reported completion in 0..1, the one-part form of `segments`; omit both
   * while the duration is unknown.
   */
  readonly value?: number;
  /** Reported completion in parts, in track order; replaces `value`. */
  readonly segments?: readonly ProgressBarSegment[];
  /** The task's outcome once it has one. */
  readonly status?: ProgressBarStatus;
  readonly layout?: GuiKitLayout;
}

/** Space between the label row and the frame. */
const LABEL_GAP = 4;

/** Length of the moving segment. */
const SEGMENT = 96;

/** Seconds for the segment to cross the frame one way. */
const CROSSING = 1;

const FILLS: Readonly<Record<ProgressBarTone, KitThemeName>> = {
  accent: "valueAccent",
  amber: "valueAmber",
  error: "valueError",
  neutral: "valueNeutral",
};

const OUTCOMES: Readonly<
  Record<
    ProgressBarStatus,
    { tone: GuiKitTone; word: string; fill?: ProgressBarTone }
  >
> = {
  complete: { tone: "accent", word: "Complete" },
  cancelled: { tone: "neutral", word: "Cancelled", fill: "neutral" },
  failed: { tone: "error", word: "Failed", fill: "error" },
};

/**
 * The reported parts in order, each clamped to what is left of the track,
 * and their total; nothing while the duration is unknown.
 */
function reportedParts(
  value: number | undefined,
  segments: readonly ProgressBarSegment[] | undefined,
) {
  const reported = segments ?? (value === undefined ? undefined : [{ value }]);
  if (!reported) return undefined;
  let total = 0;
  const parts = reported.map(({ value, tone = "accent" }) => {
    const share = Math.min(Math.max(value, 0), 1 - total);
    total += share;
    return { share, tone };
  });
  return { parts, total };
}

export function ProgressBar({
  id,
  label,
  value,
  segments,
  status,
  layout,
}: ProgressBarProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const outcome = status && OUTCOMES[status];
  const reported = reportedParts(value, segments);
  const moving = !reported && !status;
  const running = !!reported && !status;
  // Complete fills the frame with the parts in proportion; an outcome
  // recolours the reached fill.
  const parts =
    status === "complete"
      ? reported && reported.total > 0
        ? reported.parts.map(({ share, tone }) => ({
            share: share / reported.total,
            tone,
          }))
        : [{ share: 1, tone: "accent" as const }]
      : (reported?.parts.map(({ share, tone }) => ({
          share,
          tone: outcome?.fill ?? tone,
        })) ?? []);
  const total = status === "complete" ? 1 : reported?.total;
  // Floor, so the readout reaches 100% only when the task reports it.
  const readout = outcome
    ? outcome.word
    : total === undefined
      ? undefined
      : percentage(total);
  const clearance = (t.smallHeight - t.dockedHeight) / 2;

  // The run of visible pieces, then the leading section while running; the
  // run takes the part cut on its outer ends only.
  const pieces: {
    id: string;
    theme: KitThemeName;
    share?: number;
    lead?: boolean;
  }[] = parts.flatMap(({ share, tone }, index) =>
    share > 0 ? [{ id: `${id}/fill/${index}`, theme: FILLS[tone], share }] : [],
  );
  if (running)
    pieces.push({ id: `${id}/lead`, theme: "valueAccent", lead: true });

  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_COLUMN}
        height={kit.unit(t.denseRow + LABEL_GAP + t.smallHeight)}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Row id={`${id}/labels`} height={t.denseRow}>
          <TextLine id={`${id}/label`} text={label} layout={{ flex: 1 }} />
          {status === "failed" && (
            <Icon
              id={`${id}/outcome-icon`}
              icon="error"
              tone="error"
              size={t.textBody}
              layout={{ margin_right: kit.unit(t.inset / 4) }}
            />
          )}
          {readout !== undefined && (
            <TextLine
              id={`${id}/readout`}
              text={readout}
              tone={outcome?.tone ?? "text"}
            />
          )}
        </Row>
        <Entity id={`${id}/frame`}>
          <Layout
            kind={LAYOUT_PADDING}
            height={kit.unit(t.smallHeight)}
            padding_top={kit.unit(clearance)}
            padding_right={kit.unit(clearance)}
            padding_bottom={kit.unit(clearance)}
            padding_left={kit.unit(clearance)}
            margin_top={kit.unit(LABEL_GAP)}
          />
          <Skin theme={kit.theme("frame")} />
          <Children>
            <Entity id={`${id}/track`}>
              {/* Its own stack, so alignment in it is inside the clearance. */}
              <Layout kind={LAYOUT_STACK} height={kit.unit(t.dockedHeight)} />
              <Children>
                {moving ? (
                  <Entity id={`${id}/segment`}>
                    <Layout
                      kind={LAYOUT_LEAF}
                      width={kit.unit(SEGMENT)}
                      height={kit.unit(t.dockedHeight)}
                      align_x={kit.reducedMotion ? 0 : -1}
                    />
                    <Skin theme={kit.theme("valueAccent")} />
                  </Entity>
                ) : (
                  <Entity id={`${id}/fills`}>
                    <Layout
                      kind={LAYOUT_ROW}
                      height={kit.unit(t.dockedHeight)}
                    />
                    <Children>
                      {pieces.map((piece, index) => (
                        <Piece
                          key={piece.id}
                          id={piece.id}
                          theme={piece.theme}
                          share={piece.share}
                          first={index === 0}
                          last={index === pieces.length - 1}
                          lead={piece.lead}
                        />
                      ))}
                      {total !== undefined && total < 1 && (
                        <Entity id={`${id}/rest`}>
                          <Layout kind={LAYOUT_LEAF} flex={1 - total} />
                        </Entity>
                      )}
                    </Children>
                  </Entity>
                )}
                {status === "complete" && (
                  <CheckMark
                    id={`${id}/check`}
                    tone="surface"
                    layout={{
                      align_x: 1,
                      // As far from the fill's end as from its edges.
                      margin_right: kit.unit((t.dockedHeight - CHECK_MARK) / 2),
                    }}
                  />
                )}
              </Children>
            </Entity>
          </Children>
        </Entity>
      </Children>
      {moving && !kit.reducedMotion && <Sweep id={id} />}
    </Entity>
  );
}

/**
 * One piece of the fill: a part, its share of the track left beside the
 * leading section, or the leading section itself, the language's bar wide,
 * at the pulse's rest and fading.
 */
function Piece({
  id,
  theme,
  share,
  first,
  last,
  lead = false,
}: {
  readonly id: string;
  readonly theme: KitThemeName;
  readonly share?: number | undefined;
  readonly first: boolean;
  readonly last: boolean;
  readonly lead?: boolean | undefined;
}): ReactNode {
  const kit = useGuiKit();
  const t = kit.tokens;
  const cut = kit.unit(t.partCut);
  const parts = useOwnRow({
    corner_cut: [first ? cut : 0, 0, last ? cut : 0, 0],
    ...(lead ? { opacity: PULSE_REST } : {}),
  });
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        {...(lead ? { width: kit.unit(LEAD) } : { flex: share ?? 0 })}
        height={kit.unit(t.dockedHeight)}
      />
      <Skin theme={kit.theme(theme)} parts={parts} />
      {lead && <Pulse target={id} />}
    </Entity>
  );
}

/**
 * The segment's crossing to and fro, looping on the Host clock. Animation
 * adds the clip's change since its first key to the authored field, so the
 * segment rests at the start (`align_x` -1) and the clip carries it to the
 * end and back.
 */
function Sweep({ id }: { readonly id: string }) {
  const kit = useGuiKit();
  const layout = kit.contract.components.GuiLayout;
  const clip = useMemo(
    () => ({
      duration: 2 * CROSSING,
      tracks: [
        {
          property: {
            component: layout.id,
            offsets: [layout.fields.align_x.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32" as const, value: -1 },
              interpolation: { kind: "linear" as const },
            },
            {
              time: CROSSING,
              value: { kind: "f32" as const, value: 1 },
              interpolation: { kind: "linear" as const },
            },
            { time: 2 * CROSSING, value: { kind: "f32" as const, value: -1 } },
          ],
        },
      ],
    }),
    [layout],
  );
  return (
    <>
      <AnimationAsset id={`${id}/sweep`} clip={clip} />
      <Animation
        source={assetRef(`${id}/sweep`)}
        target={`${id}/segment`}
        looping
        autoPlay
      />
    </>
  );
}
