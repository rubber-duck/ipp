import { useTextMeasurements } from "../text-measurement.js";
import {
  KitLayout,
  useGuiKitLayout,
  type GuiKitLayoutProps,
  LAYOUT_LEAF,
  LAYOUT_STACK,
} from "../layout.js";

import { useEntityId } from "../../scope/identity.js";
/**
 * A slider's scale: tick marks and their labels along its rail, below a
 * horizontal rail or right of a vertical one, each centred on where the
 * thumb's centre is at its value (`slider-rail.tsx`). Ticks start a quarter
 * em past the thumb, so a thumb never covers them, in the quiet line, as the
 * dial's tick ring is drawn; labels are small neutral text, secondary to the
 * readout. A mark at the origin, such as a bipolar slider's zero, reaches
 * from the rail's edge in neutral, so it reads as the reference.
 *
 * The scale is drawn over its slider's box and past it: declare it before a
 * slider of the kit's depth in a stack holding both from its start edge, so
 * the slider paints over the origin mark. Its box is the slider's along the
 * rail, filling the stack, or `length` long when vertical, and reaches across
 * to its labels' far edge, so the stack's measurement includes them. The
 * labelled slider composes one.
 */
import { PresentationGate, Children, Entity } from "../../components.js";
import { Style, Layout } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useGuiKit, type GuiKitScope } from "../kit.js";

import {
  MarkText,
  RailMark,
  SLIDER_LENGTH,
  fractionOf,
  railGeometry,
} from "./slider-rail.js";
import { decimalsOf, formatValue, withUnits } from "./scalar-value.js";

/** What a scale marks, besides the range it spans. */
export interface SliderScaleMarks {
  /** The values to mark; by default `count` values spread evenly. */
  readonly values?: readonly number[];
  /** Marks spread evenly over the range, its ends included; 5 by default. */
  readonly count?: number;
  /** A label's text; by default the values' own decimals, signed below zero. */
  readonly format?: (value: number) => string;
  /** Units after every label; none by default, the readout carrying them. */
  readonly units?: string;
  /** A value whose mark is the reference, such as zero. */
  readonly origin?: number;
}

export interface SliderScaleProps extends SliderScaleMarks, GuiKitLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  readonly min: number;
  readonly max: number;
  /** Along a vertical rail rather than below a horizontal one. */
  readonly vertical?: boolean;
  /** A vertical rail's length at the tokens' em; the unsized slider's by default. */
  readonly length?: number;
}

/** Length of a tick across the rail. */
const TICK_LENGTH = 12;

/** The marked values in rail order. */
export function scaleValues(
  min: number,
  max: number,
  { values, count = 5 }: SliderScaleMarks,
): readonly number[] {
  if (values) return [...values].sort((left, right) => left - right);
  if (count < 2) return [min];
  return Array.from(
    { length: count },
    (_, index) => min + ((max - min) * index) / (count - 1),
  );
}

/** The labels of the marked values. */
export function scaleLabels(
  min: number,
  values: readonly number[],
  { format, units }: SliderScaleMarks,
): readonly string[] {
  const decimals = Math.max(0, ...values.map(decimalsOf));
  return values.map((value) =>
    withUnits(
      format ? format(value) : formatValue(value, decimals, min < 0),
      units,
    ),
  );
}

/**
 * The scale's lengths across the rail from the slider's start edge: where
 * the ticks and the origin's mark start and end, where labels start and the
 * whole extent.
 */
export function scaleGeometry(
  kit: GuiKitScope,
  vertical: boolean,
  widest: number,
) {
  const { depth, thumb, bar, gap, row } = railGeometry(kit);
  const centre = depth / 2;
  const tick = centre + thumb / 2 + bar / 2;
  const end = tick + kit.unit(TICK_LENGTH);
  const text = vertical ? end + gap : end;
  return {
    tick,
    origin: centre + bar / 2,
    end,
    text,
    extent: text + (vertical ? widest : row),
  };
}

export function SliderScale(props: SliderScaleProps) {
  const {
    id: providedId,
    layer = 0,
    min,
    max,
    vertical = false,
    length = SLIDER_LENGTH,
    layout,
    ...marks
  } = useGuiKitLayout(props);
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const values = scaleValues(min, max, marks);
  const labels = scaleLabels(min, values, marks);
  const measured = useTextMeasurements(
    labels.map((label) => [label, kit.typeSize("small")] as const),
  );
  const g = scaleGeometry(
    kit,
    vertical,
    Math.max(
      0,
      ...labels.map((label) =>
        measured.lineWidth(label, kit.typeSize("small")),
      ),
    ),
  );
  const line = kit.unit(kit.tokens.lineWidth);
  return (
    <>
      {measured.nodes}
      <Entity id={id}>
        <PresentationGate open />
        <Style layer={layer} />
        <KitLayout
          kind={LAYOUT_STACK}
          {...(vertical
            ? { width: g.extent, height: kit.unit(length) }
            : { height: g.extent })}
          {...layout}
        />
        <Children>
          {values.map((value, index) => {
            const mark = `${id}/mark/${index}`;
            const origin =
              marks.origin !== undefined &&
              Math.fround(value) === Math.fround(marks.origin);
            const start = origin ? g.origin : g.tick;
            return (
              <RailMark
                key={mark}
                id={mark}
                fraction={fractionOf(value, min, max)}
                vertical={vertical}
              >
                <Entity id={`${mark}/tick`}>
                  <PresentationGate open />
                  <Layout
                    kind={LAYOUT_LEAF}
                    {...(vertical
                      ? {
                          width: g.end - start,
                          height: line,
                          margin_left: start,
                          align_y: 0,
                        }
                      : {
                          width: line,
                          height: g.end - start,
                          margin_top: start,
                          align_x: 0,
                        })}
                  />
                  <Skin theme={kit.theme(origin ? "division" : "quiet")} />
                </Entity>
                <MarkText
                  id={`${mark}/label`}
                  text={labels[index]!}
                  offset={g.text}
                  vertical={vertical}
                  tone="neutral"
                />
              </RailMark>
            );
          })}
        </Children>
      </Entity>
    </>
  );
}
