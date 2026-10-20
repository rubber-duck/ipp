import {
  KitLayout,
  useGuiKitLayout,
  type GuiKitLayoutProps,
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  Row,
} from "../layout.js";

import type { ValueChangeHandler } from "./value-synchronization.js";
import { useEntityId } from "../../scope/identity.js";
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
 * The caption follows the form width; the entry fits a stable number of digits
 * and its step buttons. An explicit layout width or flex stretches the entry
 * with the column. An alert
 * expands the form below the field without moving the field itself. Set
 * reserveErrorSpace when a stable validation slot is explicitly wanted.
 */
import { useRef, useState } from "react";
import { PresentationGate, Children, Entity } from "../../components.js";
import type {
  GuiFocusChangeEvent,
  GuiFocusChangeListener,
  GuiScalarEvent,
} from "../../gui/callbacks.js";
import { Style, Behavior, Font, Layout } from "../../gui/components.js";
import type { GuiControlRef } from "../../gui/control-ref.js";
import { TextInput } from "../../gui/controls.js";
import { InlineAlert } from "../feedback/inline-alert.js";
import { Skin } from "../../gui/theme.js";
import { useGuiKit } from "../kit.js";

import { textTones } from "./slider-rail.js";
import { formatValue, useScalarValue, withUnits } from "./scalar-value.js";
import { TextLine, lineWidth } from "../text.js";

export interface NumericStepperProps extends GuiKitLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the stepper's root entity; its parts extend it. */
  readonly id?: string;
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
  readonly onChange?: ValueChangeHandler<number>;
  /** Keep the number and look but refuse input; every text turns neutral. */
  readonly disabled?: boolean;
  /** The range's ends after the caption; shown by default when both are given. */
  readonly bounds?: boolean;
  /** Keep validation room while valid; omitted forms fit only visible content. */
  readonly reserveErrorSpace?: boolean;
  /** Show the decrement and increment parts; true by default. */
  readonly stepParts?: boolean;
  /** The error shown for an entry that does not parse. */
  readonly invalidMessage?: (text: string) => string;

  /** The TextInput's control handle. */
  readonly ref?: GuiControlRef;
  readonly onFocusChange?: GuiFocusChangeListener;
}

const notANumber = () => "Not a number";

export function NumericStepper(props: NumericStepperProps) {
  const {
    id: providedId,
    layer = 0,
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
    reserveErrorSpace = false,
    stepParts = true,
    invalidMessage = notANumber,
    layout,
    ref,
    onFocusChange,
  } = useGuiKitLayout(props);
  const id = useEntityId(providedId);
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
  const gap = layout?.gap ?? kit.spacing.label;
  const validationRoom = reserveErrorSpace || error !== undefined;
  const contentHeight =
    (caption ? kit.unit(t.denseRow) + gap : 0) +
    kit.unit(t.controlHeight) +
    (validationRoom ? gap + kit.unit(t.controlHeight) : 0);
  const digits = Math.max(
    6,
    precision + 3,
    ...[min, max].map((bound) =>
      bound === undefined ? 0 : formatValue(bound, precision, false).length,
    ),
  );
  const entryWidth =
    lineWidth("0".repeat(digits), kit.fontSize) +
    kit.unit(2 * t.inset + (stepParts ? 2 * t.controlHeight : 0));
  const rowWidth =
    layout?.width !== undefined || (layout?.flex ?? 0) > 0
      ? undefined
      : entryWidth +
        (units === undefined
          ? 0
          : gap + lineWidth(units, kit.typeSize("body")));

  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <KitLayout
        kind={LAYOUT_COLUMN}
        height={contentHeight}
        gap={gap}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        {caption && (
          <Row gap={0} id={`${id}/caption`} height={kit.unit(t.denseRow)}>
            {label === undefined ? (
              <Entity id={`${id}/space`}>
                <PresentationGate open />
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
        <Row
          gap={0}
          id={`${id}/row`}
          width={rowWidth}
          height={kit.unit(t.controlHeight)}
        >
          <Entity id={`${id}/field`}>
            <PresentationGate open />
            <Layout
              kind={LAYOUT_LEAF}
              height={kit.unit(t.controlHeight)}
              flex={1}
            />
            <Behavior
              enabled={!disabled}
              {...(label === undefined ? {} : { semantic_label: label })}
            />
            {kit.controlTheme("textInput") && (
              <Skin theme={kit.controlTheme("textInput")!} />
            )}
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
          <InlineAlert id={`${id}/error`} severity="error" text={error} />
        )}
      </Children>
    </Entity>
  );
}
