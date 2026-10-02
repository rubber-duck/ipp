/**
 * A rotary knob: the runtime's slider presented as a dial in its cut
 * housing, which is the control itself, with the caption above it and, inside
 * the housing, the captions of the minimum and maximum under the ends of the
 * sweep and the readout of the value under the dial. The dial turns by
 * relative vertical drags and never jumps to the pointer; it fills from
 * `origin` when one is given, so a bipolar knob lights only the arc between
 * zero and its value. The housing is `size` wide, by default the language's
 * dial, the unsized dial's side, and a dense row taller, so the runtime draws
 * the dial in its top square and leaves the row beneath for the readout; a
 * larger size gives longer end captions room. Focus glows on the housing's
 * border.
 *
 * The knob is a fixed-size column as wide as the wider of its housing and
 * caption, centred on each other. `children` go in a column under the
 * housing as tall as a numeric stepper without its caption, field and error
 * line, for the paired input that enters a precise value: a
 * `NumericStepper` without step parts or range caption, sharing the knob's
 * value, so typing a number turns the knob and turning it updates the
 * field:
 *
 * ```tsx
 * const [gain, setGain] = useState(65);
 * <Knob id="gain" label="GAIN" min={0} max={100} step={1} units="%"
 *   value={gain} onChange={setGain} layout={{ width: 176 }}>
 *   <NumericStepper id="gain/input" min={0} max={100} units="%"
 *     stepParts={false} bounds={false} value={gain} onChange={setGain} />
 * </Knob>
 * ```
 *
 * Give such a knob a `layout` width that fits the input's error message; the
 * housing stays centred above it.
 */
import type { ReactNode } from "react";
import { Children, Entity } from "../components.js";
import { Font, Layout } from "../gui/components.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_COLUMN, Row } from "./layout.js";
import { numericStepperHeight } from "./numeric-stepper.js";
import {
  SliderControl,
  railGeometry,
  textTones,
  type SliderCompositeProps,
} from "./slider-rail.js";
import { defaultFormat, useScalarValue, withUnits } from "./scalar-value.js";
import { TextLine, lineWidth } from "./text.js";

export interface KnobProps extends SliderCompositeProps {
  /** The value, controlled; pair it with `onChange`. */
  readonly value?: number;
  /** The initial value while `value` is omitted; the origin, or the minimum. */
  readonly defaultValue?: number;
  /** The runtime committed a new value. */
  readonly onChange?: (value: number) => void;
  /** The value the arc runs from, such as zero on a bipolar knob. */
  readonly origin?: number;
  /** Captions of the minimum and maximum at the sweep's ends; shown by default. */
  readonly bounds?: boolean;
  /** The housing's width and the dial's side at the tokens' em; the language's dial by default. */
  readonly size?: number;
  /** Content under the housing, such as a paired numeric stepper. */
  readonly children?: ReactNode;
}

/**
 * The runtime's inset of the dial's tick ring from the dial square's edge, in
 * ems of the control's font.
 */
const TICK_INSET_EMS = 0.5;

export function Knob(props: KnobProps) {
  const {
    id,
    label,
    min,
    max,
    step = 0,
    value,
    defaultValue,
    onChange,
    origin,
    bounds = true,
    size,
    units,
    disabled,
    layout,
    ref,
    children,
  } = props;
  const kit = useGuiKit();
  const t = kit.tokens;
  const { row, gap } = railGeometry(kit);
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
  const small = kit.typeSize("small");
  const body = kit.typeSize("body");

  const side = kit.unit(size ?? t.dial);
  const caption = label === undefined ? 0 : lineWidth(label, body);
  const input = kit.unit(numericStepperHeight(t, false));
  const slot = children === undefined ? 0 : gap + input;
  // An end caption centres a small size below where the tick ring's outer
  // edge ends, at half past seven and half past four, but keeps half an
  // inset inside the housing, clear of its line and cut.
  const reach = (side / 2 - TICK_INSET_EMS * kit.fontSize) / Math.SQRT2;
  const end = (name: "min" | "max", number: number) => {
    const words = text(number);
    const width = lineWidth(words, small);
    const room = side / 2 - gap - width / 2;
    const x = side / 2 + Math.min(reach, room) * (name === "min" ? -1 : 1);
    return (
      <Row
        id={`${id}/${name}`}
        layout={{
          width,
          height: row,
          margin_left: x - width / 2,
          margin_top: side / 2 + reach + small - row / 2,
        }}
      >
        <TextLine
          id={`${id}/${name}/text`}
          text={words}
          tone={tones.secondary}
          size="small"
        />
      </Row>
    );
  };
  const readout = text(state.current[0]!);

  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_COLUMN}
        width={Math.max(side, caption)}
        height={(label === undefined ? 0 : row) + side + row + slot}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        {label !== undefined && (
          <Row
            id={`${id}/caption`}
            height={t.denseRow}
            layout={{ width: caption, align_x: 0 }}
          >
            <TextLine id={`${id}/label`} text={label} tone={tones.label} />
          </Row>
        )}
        <SliderControl
          id={`${id}/dial`}
          props={props}
          value={state}
          axis={2}
          origin={origin}
          layout={{ width: side, height: side + row, align_x: 0 }}
        >
          {bounds && end("min", min)}
          {bounds && end("max", max)}
          <Row
            id={`${id}/readout`}
            layout={{
              width: lineWidth(readout, body),
              height: row,
              margin_top: side,
              align_x: 0,
            }}
          >
            <TextLine
              id={`${id}/readout/text`}
              text={readout}
              tone={tones.readout}
            />
          </Row>
        </SliderControl>
        {children !== undefined && (
          <Entity id={`${id}/slot`}>
            <Layout kind={LAYOUT_COLUMN} height={input} margin_top={gap} />
            <Children>{children}</Children>
          </Entity>
        )}
      </Children>
    </Entity>
  );
}
