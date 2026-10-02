/**
 * The rail the kit's slider composites share: the runtime's slider, at the
 * runtime's unsized depth across its rail, and marks placed along it at the
 * runtime's value-to-position mapping
 * (`crates/ipp-core/src/world/systems/gui/local/slider.rs`). The thumb, a
 * square three quarters of the control's smaller side, travels by its centre
 * from half a thumb in from the minimum's end, the left or the bottom, to
 * half a thumb in from the other end. A mark is a box the thumb's size along
 * a stack as long as the rail, aligned by the value's fraction of the range,
 * so its centre is the thumb's centre at that value whatever the rail's
 * length; what it holds, a tick or a label, centres on it.
 *
 * The slider declares no theme: it paints the runtime's default look of its
 * presentation, the slider's or the dial's.
 */
import type { ReactNode } from "react";
import { Children, Entity } from "../components.js";
import type { GuiFocusChangeListener } from "../gui/callbacks.js";
import { Behavior, Layout } from "../gui/components.js";
import type { GuiControlRef } from "../gui/control-ref.js";
import { Slider } from "../gui/controls.js";
import { useGuiKit, type GuiKitScope, type GuiKitTone } from "./kit.js";
import { LAYOUT_STACK, Row, type GuiKitLayout } from "./layout.js";
import type { ScalarValue } from "./scalar-value.js";
import { TextLine, lineWidth } from "./text.js";

/**
 * The runtime's unsized slider depth across its rail, in ems: its thumb,
 * three quarters of it, is then the language's 16-unit part at body size.
 */
const DEPTH_EMS = 4 / 3;

/** The runtime's thumb edge as a fraction of the control's smaller side. */
const THUMB_EDGE = 0.75;

/** The runtime's unsized slider length along its rail, at the tokens' em. */
export const SLIDER_LENGTH = 128;

/** What every slider composite takes besides its values. */
export interface SliderCompositeProps {
  /** Symbolic id of the composite's root entity; its parts extend it. */
  readonly id: string;
  /** The caption naming the value, such as GAIN, in the accent. */
  readonly label?: string;
  readonly min: number;
  readonly max: number;
  /** Step of drags, arrows and the wheel; zero, the default, is continuous. */
  readonly step?: number;
  /** Step of arrows and the wheel with Shift; zero, the default, is the step. */
  readonly fineStep?: number;
  /** The text of a value; by default fixed to the step's decimals. */
  readonly format?: (value: number) => string;
  /** Units after every formatted value, such as `%` or `m`. */
  readonly units?: string;
  /** Keep the value and look but refuse input; every text turns neutral. */
  readonly disabled?: boolean;
  readonly layout?: GuiKitLayout;
  /** The Slider's control handle. */
  readonly ref?: GuiControlRef;
  /** Focus moved to or from the slider, or between a range's thumbs. */
  readonly onFocusChange?: GuiFocusChangeListener;
}

/** The rail geometry in the World's units. */
export function railGeometry(kit: GuiKitScope) {
  const t = kit.tokens;
  const depth = kit.unit(t.em * DEPTH_EMS);
  return {
    depth,
    thumb: THUMB_EDGE * depth,
    bar: kit.unit(t.bar),
    gap: kit.unit(t.inset / 2),
    row: kit.unit(t.denseRow),
  };
}

/** A value's fraction of the range, as the runtime places its thumb. */
export function fractionOf(value: number, min: number, max: number): number {
  if (!(max > min)) return 0;
  return Math.min(Math.max((value - min) / (max - min), 0), 1);
}

/**
 * Margins that centre a child `size` long on a box `extent` long: negative
 * when the child is the longer, which then reaches past the box evenly on
 * both sides without changing the box's measurement.
 */
export function centredOn(size: number, extent: number): number {
  return (extent - size) / 2;
}

/**
 * A box the thumb's size along a stack as long as the rail, filling it
 * across, whose centre is the thumb's centre at `fraction` of the range.
 */
export function RailMark({
  id,
  fraction,
  vertical,
  children,
}: {
  readonly id: string;
  readonly fraction: number;
  readonly vertical: boolean;
  readonly children?: ReactNode;
}) {
  const kit = useGuiKit();
  const { thumb } = railGeometry(kit);
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_STACK}
        {...(vertical
          ? { height: thumb, align_y: 1 - 2 * fraction }
          : { width: thumb, align_x: 2 * fraction - 1 })}
      />
      <Children>{children}</Children>
    </Entity>
  );
}

/**
 * One line of text on a mark: across the rail at `offset` from the mark's
 * start, a dense row high and its text wide. Along the rail it centres on the
 * thumb's centre, or with `hang` it hangs from there toward the minimum (-1)
 * or the maximum (1), a quarter inset clear of the centre when horizontal,
 * where the row is only as long as its text. Its far margin across the rail
 * takes back its own extent, so the mark never narrows it.
 */
export function MarkText({
  id,
  text,
  offset,
  vertical,
  tone,
  size = "small",
  hang = 0,
}: {
  readonly id: string;
  readonly text: string;
  readonly offset: number;
  readonly vertical: boolean;
  readonly tone: GuiKitTone;
  readonly size?: "small" | "body";
  readonly hang?: -1 | 0 | 1;
}) {
  const kit = useGuiKit();
  const { thumb, row } = railGeometry(kit);
  const width = lineWidth(text, kit.typeSize(size));
  // The row's extent along the rail and its start from the mark's start,
  // where the minimum lies at the start horizontally and the end vertically.
  const length = vertical ? row : width;
  const clear = vertical ? 0 : kit.unit(kit.tokens.inset / 4);
  const toward = vertical ? -hang : hang;
  const start =
    toward === 0
      ? centredOn(length, thumb)
      : toward > 0
        ? thumb / 2 + clear
        : thumb / 2 - clear - length;
  const end = thumb - length - start;
  return (
    <Row
      id={id}
      layout={
        vertical
          ? {
              width,
              height: row,
              margin_left: offset,
              margin_right: -width,
              margin_top: start,
              margin_bottom: end,
            }
          : {
              width,
              height: row,
              margin_top: offset,
              margin_bottom: -row,
              margin_left: start,
              margin_right: end,
            }
      }
    >
      <TextLine id={`${id}/text`} text={text} tone={tone} size={size} />
    </Row>
  );
}

/**
 * The runtime's slider as a stack, so it fills its container along the rail
 * unless sized, holding `children` over it; it declares its values once, as
 * `value` gives them.
 */
export function SliderControl({
  id,
  props,
  value,
  axis,
  range = false,
  origin,
  layout,
  children,
}: {
  readonly id: string;
  readonly props: SliderCompositeProps;
  readonly value: ScalarValue;
  /** Horizontal 0, vertical 1 or a dial 2. */
  readonly axis: 0 | 1 | 2;
  readonly range?: boolean;
  readonly origin?: number | undefined;
  readonly layout: GuiKitLayout;
  readonly children?: ReactNode;
}) {
  const { label, min, max, step = 0, fineStep = 0, disabled = false } = props;
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_STACK} {...layout} />
      <Behavior
        enabled={!disabled}
        {...(label === undefined ? {} : { semantic_label: label })}
      />
      <Slider
        min={min}
        max={max}
        step={step}
        fine_step={fineStep}
        axis={axis}
        {...(range ? { range: true } : {})}
        {...(origin === undefined ? {} : { origin })}
        {...value.control}
        {...(props.onFocusChange ? { onFocusChange: props.onFocusChange } : {})}
      />
      {children !== undefined && <Children>{children}</Children>}
    </Entity>
  );
}

/** Tones of a composite's texts: neutral throughout while disabled. */
export function textTones(disabled: boolean | undefined) {
  return {
    label: (disabled ? "neutral" : "accent") as GuiKitTone,
    readout: (disabled ? "neutral" : "text") as GuiKitTone,
    secondary: "neutral" as GuiKitTone,
  };
}

/**
 * A caption of the range's minimum or maximum at one end of a horizontal
 * rail, centred on the rail: a row the slider's depth high and its text
 * wide.
 */
export function EndCaption({
  id,
  text,
  layout,
}: {
  readonly id: string;
  readonly text: string;
  readonly layout?: GuiKitLayout;
}) {
  const kit = useGuiKit();
  const { depth } = railGeometry(kit);
  return (
    <Row
      id={id}
      layout={{
        width: lineWidth(text, kit.typeSize("small")),
        height: depth,
        ...layout,
      }}
    >
      <TextLine id={`${id}/text`} text={text} tone="neutral" size="small" />
    </Row>
  );
}

/**
 * One line of text centred in a dense row filling its column across: a
 * vertical composite's caption, end captions and readout, centred on the
 * rail.
 */
export function CentredLine({
  id,
  text,
  tone,
  size = "body",
}: {
  readonly id: string;
  readonly text: string;
  readonly tone: GuiKitTone;
  readonly size?: "small" | "body";
}) {
  const kit = useGuiKit();
  return (
    <Row
      id={id}
      height={kit.tokens.denseRow}
      layout={{ width: lineWidth(text, kit.typeSize(size)), align_x: 0 }}
    >
      <TextLine id={`${id}/text`} text={text} tone={tone} size={size} />
    </Row>
  );
}
