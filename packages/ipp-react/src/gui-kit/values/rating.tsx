import { useRef, useState } from "react";
import { useEntityId } from "../../scope/identity.js";
import { useField } from "../../scope/field.js";
import { Column, Row, type ContainerProps } from "../layout.js";
import { Grip } from "../containers/grip.js";
import { TextLine } from "../text.js";
import { Button } from "../button.js";
import { useGuiKit } from "../kit.js";
import { useApplicationValue } from "../selection/application-value.js";

export interface RatingProps extends Omit<ContainerProps, "children"> {
  readonly label: string;
  readonly count?: number;
  readonly precision?: number;
  readonly value?: number;
  readonly defaultValue?: number;
  readonly onChange?: (value: number) => void;
  readonly disabled?: boolean;
  readonly readOnly?: boolean;
  readonly allowClear?: boolean;
}

export function Rating({
  id: providedId,
  label,
  count = 5,
  precision = 1,
  value,
  defaultValue = 0,
  onChange,
  disabled = false,
  readOnly = false,
  allowClear = true,
  ...layout
}: RatingProps) {
  if (
    !Number.isInteger(count) ||
    count < 1 ||
    count > 100 ||
    !Number.isFinite(precision) ||
    precision <= 0 ||
    precision > 1 ||
    !Number.isInteger(1 / precision)
  )
    throw new RangeError(
      "Expected 1–100 rating marks and an evenly dividing precision",
    );
  const id = useEntityId(providedId),
    kit = useGuiKit(),
    state = useApplicationValue(value, defaultValue, onChange),
    [preview, setPreview] = useState<number | null>(null);
  const pointer = useRef<bigint | null>(null),
    area = `${id}/rating`,
    bounds = useField(area, "CanvasBounds", "local_width");
  const width =
    bounds.status === "ready" && typeof bounds.value === "number"
      ? bounds.value
      : kit.unit(count * kit.tokens.controlHeight);
  const snap = (n: number) =>
    Math.min(count, Math.max(0, Math.round(n / precision) * precision));
  if (!Number.isFinite(state.current))
    throw new RangeError("Expected a finite rating");
  const current = snap(state.current),
    shown = disabled || readOnly ? current : (preview ?? current);
  const reset = () => {
    pointer.current = null;
    setPreview(null);
  };
  const commit = (n: number) =>
    state.change(() => Math.max(allowClear ? 0 : precision, snap(n)));
  return (
    <Column {...layout} id={id}>
      <TextLine text={`${label}: ${current} / ${count}`} />
      <Grip
        id={area}
        label={`${label}: ${current} / ${count}`}
        layout={{
          width: kit.unit(count * kit.tokens.controlHeight),
          height: kit.unit(kit.tokens.controlHeight),
        }}
        disabled={disabled || readOnly}
        onBlur={reset}
        onInteractionChange={(event) => {
          if (!event.hovered && pointer.current === null) setPreview(null);
        }}
        onInput={(input) => {
          if (disabled || readOnly) return;
          if (input.kind === "pointer") {
            const val = snap(
              Math.ceil(
                ((input.point[0] / Math.max(1, width)) * count) / precision,
              ) * precision,
            );
            if (
              input.phase === "down" &&
              input.button === "primary" &&
              pointer.current === null
            ) {
              pointer.current = input.pointer;
              setPreview(val);
            }
            if (
              input.phase === "move" &&
              (pointer.current === null || pointer.current === input.pointer)
            )
              setPreview(
                input.point[0] < 0 || input.point[0] > width ? null : val,
              );
            if (input.phase === "up" && pointer.current === input.pointer) {
              commit(val);
              reset();
            }
            if (input.phase === "cancel") reset();
          } else if (input.kind === "key" && input.pressed) {
            if (input.key === "Home") commit(allowClear ? 0 : precision);
            if (input.key === "End") commit(count);
            if (input.key === "ArrowLeft" || input.key === "ArrowDown")
              commit(current - precision);
            if (input.key === "ArrowRight" || input.key === "ArrowUp")
              commit(current + precision);
          }
        }}
      >
        <Row gap={0}>
          {Array.from({ length: count }, (_, i) => (
            <TextLine
              key={i}
              id={`${id}/mark/${i}`}
              flex={1}
              text={shown >= i + 1 ? "★" : shown > i ? "◐" : "☆"}
              tone={shown > i ? "accent" : "neutral"}
            />
          ))}
        </Row>
      </Grip>
      {allowClear && !readOnly && (
        <Button
          label="Clear"
          disabled={disabled || !current}
          onPress={() => commit(0)}
        />
      )}
    </Column>
  );
}
