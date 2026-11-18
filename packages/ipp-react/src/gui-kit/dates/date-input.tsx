import { useEntityId } from "../../scope/identity.js";
import type { ContainerProps } from "../layout.js";
import { useApplicationValue } from "../selection/application-value.js";
import {
  civilDate,
  parseCivilDate,
  dateEligible,
  validateDateBounds,
  type CivilDate,
  type DateBounds,
} from "./civil-date.js";
import { Segments, type SegmentValues } from "./segments.js";

export interface DateInputProps
  extends Omit<ContainerProps, "children">,
    DateBounds {
  readonly label?: string;
  readonly locale?: string;
  readonly required?: boolean;
  readonly value?: CivilDate | null;
  readonly defaultValue?: CivilDate | null;
  readonly onChange?: (date: CivilDate | null) => void;
}

export function DateInput({
  id: providedId,
  label = "Date",
  locale = "en",
  required = false,
  value,
  defaultValue = null,
  onChange,
  min,
  max,
  isDateDisabled,
  ...layout
}: DateInputProps) {
  const id = useEntityId(providedId);
  validateDateBounds({ min, max });
  const state = useApplicationValue(value, defaultValue, onChange);
  if (state.current !== null && !parseCivilDate(state.current))
    throw new RangeError("Invalid committed civil date");
  const segments = new Intl.DateTimeFormat(locale, {
    calendar: "gregory",
    timeZone: "UTC",
    year: "numeric",
    month: "numeric",
    day: "numeric",
  })
    .formatToParts(0)
    .filter((p) => p.type === "year" || p.type === "month" || p.type === "day")
    .map((p) => ({ key: p.type, label: p.type }));
  function parse(parts: SegmentValues) {
    if (
      !/^\d{4}$/.test(parts.year ?? "") ||
      !/^\d{1,2}$/.test(parts.month ?? "") ||
      !/^\d{1,2}$/.test(parts.day ?? "")
    )
      return null;
    try {
      return civilDate(
        Number(parts.year),
        Number(parts.month),
        Number(parts.day),
      );
    } catch {
      return null;
    }
  }
  return (
    <Segments
      {...layout}
      id={id}
      label={label}
      value={state.current}
      segments={segments}
      required={required}
      split={(date) => {
        const [year = "", month = "", day = ""] = (date ?? "").split("-");
        return { year, month, day };
      }}
      parse={parse}
      eligible={(date) => dateEligible(date, { min, max, isDateDisabled })}
      paste={(text) => (parseCivilDate(text) ? text : null)}
      onCommit={(next) => state.change(() => next)}
    />
  );
}
