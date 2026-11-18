import { useEntityId } from "../../scope/identity.js";
import { Column } from "../layout.js";
import { Popover } from "../overlays/popover.js";
import type { OverlayOpenProps } from "../overlays/overlay.js";
import { useApplicationValue } from "../selection/application-value.js";
import { Calendar, type CalendarProps } from "./calendar.js";
import { DateInput } from "./date-input.js";
import type { CivilRange } from "./civil-date.js";

export type DatePickerProps = CalendarProps &
  OverlayOpenProps & { readonly label?: string };
export function DatePicker(props: DatePickerProps) {
  const {
    id: providedId,
    label = "Date",
    open,
    defaultOpen = false,
    onOpenChange,
    mode,
    value,
    defaultValue,
    onChange,
    min,
    max,
    isDateDisabled,
    locale,
    month,
    defaultMonth,
    onMonthChange,
    weekStartsOn,
    ...layout
  } = props;
  const id = useEntityId(providedId);
  const state = useApplicationValue<string | CivilRange | null>(
    value,
    defaultValue ?? null,
    (next) => {
      if (props.mode === "range") {
        if (typeof next !== "string") props.onChange?.(next);
      } else if (typeof next === "string" || next === null)
        props.onChange?.(next);
    },
  );
  const visibility = useApplicationValue(open, defaultOpen, onOpenChange);
  const constraints = {
    min,
    max,
    isDateDisabled,
    ...(locale === undefined ? {} : { locale }),
  };
  const calendar = {
    ...constraints,
    ...(month ? { month } : {}),
    ...(defaultMonth ? { defaultMonth } : {}),
    ...(onMonthChange ? { onMonthChange } : {}),
    ...(weekStartsOn === undefined ? {} : { weekStartsOn }),
  };
  const range = typeof state.current === "object" ? state.current : null;
  function commit(next: string | CivilRange | null) {
    state.change(() => next);
    if (next === null || typeof next === "string" || next.end)
      visibility.change(() => false);
  }
  return (
    <Column {...layout} id={id}>
      {mode === "range" ? (
        <>
          <DateInput
            {...constraints}
            id={`${id}/start`}
            label={`${label} start`}
            value={range?.start ?? null}
            onChange={(d) =>
              commit(
                d
                  ? {
                      start: d,
                      end: range?.end && range.end >= d ? range.end : null,
                    }
                  : null,
              )
            }
          />
          <DateInput
            {...constraints}
            id={`${id}/end`}
            label={`${label} end`}
            min={
              range?.start &&
              (!max || range.start <= max) &&
              (!min || range.start > min)
                ? range.start
                : min
            }
            value={range?.end ?? null}
            onChange={(d) =>
              commit(
                range
                  ? { ...range, end: d }
                  : d
                    ? { start: d, end: null }
                    : null,
              )
            }
          />
        </>
      ) : (
        <DateInput
          {...constraints}
          id={`${id}/date`}
          label={label}
          value={typeof state.current === "string" ? state.current : null}
          onChange={commit}
        />
      )}
      <Popover
        id={`${id}/choose`}
        title={label}
        label="Choose date"
        contentWidth={360}
        open={visibility.current}
        onOpenChange={(show) => visibility.change(() => show)}
      >
        {mode === "range" ? (
          <Calendar
            {...calendar}
            id={`${id}/calendar`}
            mode="range"
            value={range}
            onChange={commit}
          />
        ) : (
          <Calendar
            {...calendar}
            id={`${id}/calendar`}
            value={typeof state.current === "string" ? state.current : null}
            onChange={commit}
          />
        )}
      </Popover>
    </Column>
  );
}
