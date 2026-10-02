/**
 * An ordered interval on one rail: the runtime's range slider, whose two
 * thumbs are each a focus stop with its own keys and stop at each other, with
 * the readout of each value following its thumb and captions of the range's
 * ends. The fill runs between the thumbs; both values change in one write and
 * are reported together, once per change.
 *
 * Readouts are content text in every state: focus shows as the focused
 * thumb's glow alone, and a readout that lit with it would follow focus a
 * client round trip late. Each readout hangs from its thumb's centre away
 * from the other thumb, the lower one toward the minimum and the upper one
 * toward the maximum, so the two never overlap, even with the thumbs
 * together, at any rail length: a horizontal rail fills its container, and
 * the client cannot know its laid-out length to tell when centred readouts
 * would collide.
 *
 * Horizontal, the composite is a fixed-height column filling its
 * container's width: an optional caption, then the rail between the
 * minimum's and maximum's captions, centred on it, with the readouts under
 * the thumbs. Vertical, the minimum is at the bottom: a fixed-height
 * column filling its container's width with the rail centred in it, the
 * caption above, the maximum's caption above the rail and the minimum's
 * below it, and the readouts right of the thumbs, the lower one below its
 * thumb's centre and the upper one above, reaching past the column's centre.
 */
import { Children, Entity } from "../components.js";
import { Font, Layout } from "../gui/components.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_COLUMN, LAYOUT_ROW, LAYOUT_STACK, Row } from "./layout.js";
import {
  CentredLine,
  EndCaption,
  MarkText,
  RailMark,
  SLIDER_LENGTH,
  SliderControl,
  fractionOf,
  railGeometry,
  textTones,
  type SliderCompositeProps,
} from "./slider-rail.js";
import { defaultFormat, useScalarValue, withUnits } from "./scalar-value.js";
import { TextLine, lineWidth } from "./text.js";

/** A range's lower and upper values. */
export type RangeValue = readonly [number, number];

export interface RangeSliderProps extends SliderCompositeProps {
  /** The interval, controlled; pair it with `onChange`. */
  readonly value?: RangeValue;
  /** The initial interval while `value` is omitted; the whole range by default. */
  readonly defaultValue?: RangeValue;
  /** The runtime committed a new interval: one or both values changed. */
  readonly onChange?: (value: RangeValue) => void;
  /** Along a vertical rail, minimum at the bottom. */
  readonly vertical?: boolean;
  /** A vertical rail's length at the tokens' em; the unsized slider's by default. */
  readonly length?: number;
  /** Captions of the minimum and maximum at the rail's ends; shown by default. */
  readonly bounds?: boolean;
}

export function RangeSlider(props: RangeSliderProps) {
  const {
    id,
    label,
    min,
    max,
    step = 0,
    value,
    defaultValue,
    onChange,
    vertical = false,
    length = SLIDER_LENGTH,
    bounds = true,
    units,
    disabled,
    layout,
    ref,
  } = props;
  const kit = useGuiKit();
  const t = kit.tokens;
  const g = railGeometry(kit);
  const format = props.format ?? defaultFormat(min, max, step);
  const state = useScalarValue(
    value,
    defaultValue ?? [min, max],
    onChange && ((values) => onChange([values[0]!, values[1]!])),
    ref,
  );
  const tones = textTones(disabled);
  const text = (number: number) => withUnits(format(number), units);
  const readouts = state.current.map(text);
  // Readouts sit half an inset past the thumb, right of a vertical one; a
  // horizontal one's dense row starts at the slider's edge.
  const offset = vertical ? (g.depth + g.thumb) / 2 + g.gap : g.depth;
  const readoutMarks = state.current.map((number, index) => (
    <RailMark
      key={index}
      id={`${id}/thumb/${index}`}
      fraction={fractionOf(number, min, max)}
      vertical={vertical}
    >
      <MarkText
        id={`${id}/readout/${index}`}
        text={readouts[index]!}
        offset={offset}
        vertical={vertical}
        tone={tones.readout}
        hang={index === 0 ? -1 : 1}
      />
    </RailMark>
  ));

  if (vertical) {
    const rail = kit.unit(length);
    const widest = Math.max(
      ...[min, max, ...state.current].map((number) =>
        lineWidth(text(number), kit.typeSize("small")),
      ),
    );
    const box = offset + widest;
    const rows = (label === undefined ? 0 : 1) + (bounds ? 2 : 0);
    return (
      <Entity id={id}>
        <Layout kind={LAYOUT_COLUMN} height={rows * g.row + rail} {...layout} />
        <Font source={kit.font} font_size={kit.fontSize} />
        <Children>
          {label !== undefined && (
            <CentredLine id={`${id}/label`} text={label} tone={tones.label} />
          )}
          {bounds && (
            <CentredLine
              id={`${id}/max`}
              text={text(max)}
              tone={tones.secondary}
              size="small"
            />
          )}
          <Entity id={`${id}/track`}>
            <Layout kind={LAYOUT_STACK} height={rail} />
            <Children>
              {/* The rail centred: the readouts' reach past it takes no room. */}
              <Entity id={`${id}/rail`}>
                <Layout
                  kind={LAYOUT_STACK}
                  width={box}
                  height={rail}
                  align_x={0}
                  margin_right={g.depth - box}
                />
                <Children>
                  {readoutMarks}
                  <SliderControl
                    id={`${id}/slider`}
                    props={props}
                    value={state}
                    axis={1}
                    range
                    layout={{ width: g.depth, height: rail }}
                  />
                </Children>
              </Entity>
            </Children>
          </Entity>
          {bounds && (
            <CentredLine
              id={`${id}/min`}
              text={text(min)}
              tone={tones.secondary}
              size="small"
            />
          )}
        </Children>
      </Entity>
    );
  }

  const box = g.depth + g.row;
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_COLUMN}
        height={(label === undefined ? 0 : g.row) + box}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        {label !== undefined && (
          <Row id={`${id}/header`} height={t.denseRow}>
            <TextLine id={`${id}/label`} text={label} tone={tones.label} />
          </Row>
        )}
        <Entity id={`${id}/rail-row`}>
          <Layout kind={LAYOUT_ROW} height={box} />
          <Children>
            {bounds && (
              <EndCaption
                id={`${id}/min`}
                text={text(min)}
                layout={{ margin_right: g.gap }}
              />
            )}
            <Entity id={`${id}/rail`}>
              <Layout kind={LAYOUT_STACK} height={box} flex={1} />
              <Children>
                {readoutMarks}
                <SliderControl
                  id={`${id}/slider`}
                  props={props}
                  value={state}
                  axis={0}
                  range
                  layout={{ height: g.depth }}
                />
              </Children>
            </Entity>
            {bounds && (
              <EndCaption
                id={`${id}/max`}
                text={text(max)}
                layout={{ margin_left: g.gap }}
              />
            )}
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}
