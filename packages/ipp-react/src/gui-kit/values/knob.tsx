import { KitLayout, useGuiKitLayout, LAYOUT_COLUMN, Row } from "../layout.js";

import type { ValueChangeHandler } from "./value-synchronization.js";
import { useEntityId } from "../../scope/identity.js";
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
 * The column fits its content, as wide as the wider of its housing and
 * caption, centred on each other. `children` flow under the housing and
 * can expand for validation feedback. A paired precise input can be a
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
import { PresentationGate, Children, Entity } from "../../components.js";
import { Style, Font, Layout } from "../../gui/components.js";
import { useGuiKit } from "../kit.js";

import {
  SliderControl,
  railGeometry,
  textTones,
  type SliderCompositeProps,
} from "./slider-rail.js";
import { defaultFormat, useScalarValue, withUnits } from "./scalar-value.js";
import { useOwnRow } from "../row-motion.js";
import { TextLine, lineWidth } from "../text.js";

export interface KnobProps extends SliderCompositeProps {
  /** The value, controlled; pair it with `onChange`. */
  readonly value?: number;
  /** The initial value while `value` is omitted; the origin, or the minimum. */
  readonly defaultValue?: number;
  /** The runtime committed a new value; return async acceptance before convergence. */
  readonly onChange?: ValueChangeHandler<number>;
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

export function Knob(inputProps: KnobProps) {
  const props = useGuiKitLayout(inputProps);
  const {
    id: providedId,
    layer = 0,
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
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const t = kit.tokens;
  const readoutInside = t.dialReadoutInside ?? false;
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
  const housingScale = t.dialHousingScale ?? 1;
  const housingInset = (side * (housingScale - 1)) / 2;
  const housing = useOwnRow({ outset: [housingInset, housingInset] });
  const caption = label === undefined ? 0 : lineWidth(label, body);
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
        gap={0}
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
  const readoutType = readoutInside ? "small" : "body";
  const readoutSize = kit.typeSize(readoutType);

  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <KitLayout
        kind={LAYOUT_COLUMN}
        width={Math.max(side, caption)}
        height={-1}
        fit_height
        gap={kit.spacing.label}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        {label !== undefined && (
          <Row
            gap={0}
            id={`${id}/caption`}
            height={kit.unit(t.denseRow)}
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
          {...(housingScale !== 1 ? { skinParts: housing } : {})}
          layout={{
            width: side,
            height: side + (readoutInside ? 0 : row),
            align_x: 0,
          }}
        >
          {bounds && end("min", min)}
          {bounds && end("max", max)}
          <Row
            gap={0}
            id={`${id}/readout`}
            layout={{
              width: lineWidth(readout, readoutSize),
              height: row,
              margin_top: readoutInside ? (side - row) / 2 : side,
              align_x: 0,
            }}
          >
            <TextLine
              id={`${id}/readout/text`}
              text={readout}
              size={readoutType}
              tone={tones.readout}
            />
          </Row>
        </SliderControl>
        {children !== undefined && (
          <Entity id={`${id}/slot`}>
            <PresentationGate open />
            <Layout
              kind={LAYOUT_COLUMN}
              height={-1}
              fit_height
              margin_top={0}
            />
            <Children>{children}</Children>
          </Entity>
        )}
      </Children>
    </Entity>
  );
}
