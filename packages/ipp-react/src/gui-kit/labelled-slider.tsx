/**
 * A slider with its caption, a formatted readout of its value with units,
 * optional captions of its minimum and maximum and an optional scale. The
 * slider is the runtime's, at its unsized depth across the rail; it fills
 * from `origin` when one is given, so a bipolar slider fills only between
 * zero and its thumb. The readout follows each committed value.
 *
 * Horizontal, the composite is a fixed-height column filling its
 * container's width: the caption and, at the row's end, the readout above
 * the rail, as a progress bar labels its track, then the rail between the
 * minimum's and maximum's captions, with the scale below it.
 *
 * Vertical, the minimum is at the bottom and the text stays upright: a
 * fixed-height column filling its container's width with the rail centred
 * in it, its caption above, the maximum's caption above the rail and the
 * minimum's below it, the readout under them and the scale right of the
 * rail, reaching past the column's centre by its ticks and labels.
 *
 * End captions are shown by default unless a scale labels the ends.
 */
import { Children, Entity } from "../components.js";
import { Style, Font, Layout } from "../gui/components.js";
import { useGuiKit } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  LAYOUT_ROW,
  LAYOUT_STACK,
  Row,
  type GuiKitLayout,
} from "./layout.js";
import {
  CentredLine,
  EndCaption,
  SLIDER_LENGTH,
  SliderControl,
  railGeometry,
  textTones,
  type SliderCompositeProps,
} from "./slider-rail.js";
import {
  SliderScale,
  scaleGeometry,
  scaleLabels,
  scaleValues,
  type SliderScaleMarks,
} from "./slider-scale.js";
import { defaultFormat, useScalarValue, withUnits } from "./scalar-value.js";
import { TextLine } from "./text.js";

export interface LabelledSliderProps extends SliderCompositeProps {
  /** The value, controlled; pair it with `onChange`. */
  readonly value?: number;
  /** The initial value while `value` is omitted; the origin, or the minimum. */
  readonly defaultValue?: number;
  /** The runtime committed a new value. */
  readonly onChange?: (value: number) => void;
  /** The value the fill runs from, such as zero on a bipolar slider. */
  readonly origin?: number;
  /** Along a vertical rail, minimum at the bottom. */
  readonly vertical?: boolean;
  /** A vertical rail's length at the tokens' em; the unsized slider's by default. */
  readonly length?: number;
  /** Captions of the minimum and maximum; by default unless a scale shows them. */
  readonly bounds?: boolean;
  /**
   * A scale of ticks and labels along the rail; its origin is the slider's
   * by default.
   */
  readonly scale?: SliderScaleMarks;
}

export function LabelledSlider(props: LabelledSliderProps) {
  const {
    id,
    layer = 0,
    label,
    min,
    max,
    step = 0,
    value,
    defaultValue,
    onChange,
    origin,
    vertical = false,
    length = SLIDER_LENGTH,
    scale,
    bounds = scale === undefined,
    units,
    disabled,
    layout,
    ref,
  } = props;
  const kit = useGuiKit();
  const t = kit.tokens;
  const g = railGeometry(kit);
  const format = props.format ?? defaultFormat(min, max, step);
  const initial =
    defaultValue ??
    (origin !== undefined && origin >= min && origin <= max ? origin : min);
  const state = useScalarValue(
    value === undefined ? undefined : [value],
    [initial],
    onChange && ((values) => onChange(values[0]!)),
    ref,
  );
  const tones = textTones(disabled);
  const text = (number: number) => withUnits(format(number), units);
  const readout = text(state.current[0]!);
  const marks = scale && {
    ...(origin === undefined ? {} : { origin }),
    ...scale,
  };
  const scaleExtent =
    marks &&
    scaleGeometry(
      kit,
      vertical,
      scaleLabels(min, scaleValues(min, max, marks), marks),
    ).extent;

  const slider = (layoutFields: GuiKitLayout) => (
    <SliderControl
      id={`${id}/slider`}
      props={props}
      value={state}
      axis={vertical ? 1 : 0}
      origin={origin}
      layout={layoutFields}
    />
  );
  const scaleOf = marks && (
    <SliderScale
      id={`${id}/scale`}
      min={min}
      max={max}
      vertical={vertical}
      length={length}
      {...marks}
    />
  );

  if (vertical) {
    const rail = kit.unit(length);
    const box = Math.max(g.depth, scaleExtent ?? 0);
    const rows = (label === undefined ? 0 : 1) + (bounds ? 2 : 0) + 1;
    return (
      <Entity id={id}>
        <Style layer={layer} />
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
              {/* The rail centred: the scale's reach past it takes no room. */}
              <Entity id={`${id}/rail`}>
                <Layout
                  kind={LAYOUT_STACK}
                  width={box}
                  height={rail}
                  align_x={0}
                  margin_right={g.depth - box}
                />
                <Children>
                  {scaleOf}
                  {slider({ width: g.depth, height: rail })}
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
          <CentredLine
            id={`${id}/readout`}
            text={readout}
            tone={tones.readout}
          />
        </Children>
      </Entity>
    );
  }

  const box = Math.max(g.depth, scaleExtent ?? 0);
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout kind={LAYOUT_COLUMN} height={g.row + box} {...layout} />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Row id={`${id}/header`} height={t.denseRow}>
          {label === undefined ? (
            <Entity id={`${id}/space`}>
              <Layout kind={LAYOUT_LEAF} flex={1} />
            </Entity>
          ) : (
            <TextLine
              id={`${id}/label`}
              text={label}
              tone={tones.label}
              layout={{ flex: 1 }}
            />
          )}
          <TextLine id={`${id}/readout`} text={readout} tone={tones.readout} />
        </Row>
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
                {scaleOf}
                {slider({ height: g.depth })}
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
