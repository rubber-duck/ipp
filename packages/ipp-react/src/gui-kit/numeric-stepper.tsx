/**
 * A numeric stepper: the runtime's numeric text input, with its decrement and
 * increment parts, between a caption with the range's ends above it and a
 * persistent unit label beside it, and an error line under it. The runtime
 * owns the number and its entry: the text being edited stays an edit until
 * Enter or blur commits it clamped to the range, arrows and the step parts
 * step it, each part disabled at its bound, and a held part repeats on the
 * Host clock. The stepper reports each committed change through `onChange`.
 *
 * An entry that does not parse leaves the number unchanged and the runtime
 * reports it; the stepper then shows `invalidMessage` in an error alert
 * under the field and keeps it until the next commit, submission or step,
 * until the edit is discarded, as by Escape, which shows the formatted number
 * again, or until the field loses focus, which ends the edit. Each of these
 * carries the tick of the frame it happened in, so a report delivered after a
 * later commit or discard of the same frame does not bring the alert back.
 *
 * The stepper is a fixed-height column filling its container's width: an
 * optional caption row, the field row, the control height, and the error
 * line's room, a gap and the control height, reserved whether or not the
 * alert shows, so that neither the field nor what lies below it ever moves.
 */
import { useRef, useState } from "react";
import { Children, Entity } from "../components.js";
import type {
  GuiFocusChangeEvent,
  GuiFocusChangeListener,
  GuiScalarEvent,
} from "../gui/callbacks.js";
import { Behavior, Font, Layout } from "../gui/components.js";
import type { GuiControlRef } from "../gui/control-ref.js";
import { TextInput } from "../gui/controls.js";
import { InlineAlert } from "./inline-alert.js";
import { useGuiKit, type GuiKitTokens } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  Row,
  type GuiKitLayout,
} from "./layout.js";
import { textTones } from "./slider-rail.js";
import { formatValue, useScalarValue, withUnits } from "./scalar-value.js";
import { TextLine, lineWidth } from "./text.js";

export interface NumericStepperProps {
  /** Symbolic id of the stepper's root entity; its parts extend it. */
  readonly id: string;
  /** The caption naming the number, such as EXPOSURE, in the accent. */
  readonly label?: string;
  /** Inclusive bounds commits and steps clamp to; unbounded when omitted. */
  readonly min?: number;
  readonly max?: number;
  /** Step of the arrows and step parts; 1 by default, zero disables stepping. */
  readonly step?: number;
  /** Step of the arrows with Shift; zero, the default, is the step. */
  readonly fineStep?: number;
  /** Decimals the number shows with; none by default. */
  readonly precision?: number;
  /** The persistent unit label beside the field, such as EV. */
  readonly units?: string;
  /** The number, controlled; pair it with `onChange`. */
  readonly value?: number;
  /** The initial number while `value` is omitted; zero within the range. */
  readonly defaultValue?: number;
  /** The runtime committed a new number. */
  readonly onChange?: (value: number) => void;
  /** Keep the number and look but refuse input; every text turns neutral. */
  readonly disabled?: boolean;
  /** The range's ends after the caption; shown by default when both are given. */
  readonly bounds?: boolean;
  /** Show the decrement and increment parts; true by default. */
  readonly stepParts?: boolean;
  /** The error shown for an entry that does not parse. */
  readonly invalidMessage?: (text: string) => string;
  readonly layout?: GuiKitLayout;
  /** The TextInput's control handle. */
  readonly ref?: GuiControlRef;
  readonly onFocusChange?: GuiFocusChangeListener;
}

const notANumber = () => "Not a number";

/** The stepper's height at the tokens' em: caption, field and error line. */
export function numericStepperHeight(
  tokens: GuiKitTokens,
  caption: boolean,
): number {
  return (
    (caption ? tokens.denseRow : 0) +
    2 * tokens.controlHeight +
    tokens.inset / 2
  );
}

export function NumericStepper({
  id,
  label,
  min,
  max,
  step = 1,
  fineStep = 0,
  precision = 0,
  units,
  value,
  defaultValue,
  onChange,
  disabled = false,
  bounds = true,
  stepParts = true,
  invalidMessage = notANumber,
  layout,
  ref,
  onFocusChange,
}: NumericStepperProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const tones = textTones(disabled);
  const initial =
    defaultValue ?? Math.min(Math.max(0, min ?? 0), max ?? Number.MAX_VALUE);
  const state = useScalarValue(
    value === undefined ? undefined : [value],
    [initial],
    onChange && ((values) => onChange(values[0]!)),
    ref,
  );
  const [error, setError] = useState<string | undefined>(undefined);
  // The tick of the last commit, submission, discard or blur: a rejection
  // reported from that frame or earlier was superseded by it.
  const settled = useRef(-1n);
  const settle = (tick: bigint) => {
    if (tick > settled.current) settled.current = tick;
    setError(undefined);
  };

  const ends =
    bounds && min !== undefined && max !== undefined
      ? `${formatValue(min, precision, min < 0)} – ${withUnits(
          formatValue(max, precision, min < 0),
          units,
        )}`
      : undefined;
  const caption = label !== undefined || ends !== undefined;
  const gap = kit.unit(t.inset / 2);

  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_COLUMN}
        height={kit.unit(numericStepperHeight(t, caption))}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        {caption && (
          <Row id={`${id}/caption`} height={t.denseRow}>
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
            {ends !== undefined && (
              <TextLine
                id={`${id}/ends`}
                text={ends}
                tone={tones.secondary}
                size="small"
              />
            )}
          </Row>
        )}
        <Row id={`${id}/row`} height={t.controlHeight}>
          <Entity id={`${id}/field`}>
            <Layout
              kind={LAYOUT_LEAF}
              height={kit.unit(t.controlHeight)}
              flex={1}
            />
            <Behavior
              enabled={!disabled}
              {...(label === undefined ? {} : { semantic_label: label })}
            />
            <TextInput
              numeric
              value={state.control.value}
              {...(min === undefined ? {} : { min })}
              {...(max === undefined ? {} : { max })}
              step={step}
              fine_step={fineStep}
              precision={precision}
              step_parts={stepParts}
              ref={state.control.ref}
              onScalarCommit={(event: GuiScalarEvent) => {
                state.control.onScalarCommit(event);
                settle(event.tick);
              }}
              onSubmit={(event) => settle(event.tick)}
              onDiscard={(event) => settle(event.tick)}
              onReject={(event) => {
                if (event.tick > settled.current)
                  setError(invalidMessage(event.value));
              }}
              onFocusChange={(event: GuiFocusChangeEvent) => {
                if (!event.focused) settle(event.tick);
                onFocusChange?.(event);
              }}
            />
          </Entity>
          {units !== undefined && (
            <TextLine
              id={`${id}/units`}
              text={units}
              tone={tones.readout}
              layout={{
                width: lineWidth(units, kit.typeSize("body")),
                margin_left: gap,
              }}
            />
          )}
        </Row>
        {error !== undefined && (
          <InlineAlert
            id={`${id}/error`}
            severity="error"
            text={error}
            layout={{ margin_top: gap }}
          />
        )}
      </Children>
    </Entity>
  );
}
