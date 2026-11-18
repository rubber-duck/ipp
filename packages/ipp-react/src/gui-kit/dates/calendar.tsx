import { Children, Entity, PresentationGate } from "../../components.js";
import { Behavior, Group, Layout } from "../../gui/components.js";
import { useEntityId } from "../../scope/identity.js";
import { Button } from "../button.js";
import { Column, Row, type ContainerProps } from "../layout.js";
import { TextLine } from "../text.js";
import { useApplicationValue } from "../selection/application-value.js";
import {
  adjacentMonth,
  civilDate,
  civilInstant,
  dateEligible,
  monthLength,
  parseCivilDate,
  validateDateBounds,
  type CivilDate,
  type CivilRange,
  type DateBounds,
} from "./civil-date.js";

export type CalendarSelection =
  | {
      readonly mode?: "single";
      readonly value?: CivilDate | null;
      readonly defaultValue?: CivilDate | null;
      readonly onChange?: (value: CivilDate | null) => void;
    }
  | {
      readonly mode: "range";
      readonly value?: CivilRange | null;
      readonly defaultValue?: CivilRange | null;
      readonly onChange?: (value: CivilRange | null) => void;
    };
export type CalendarProps = CalendarSelection &
  DateBounds &
  Omit<ContainerProps, "children"> & {
    /** Any date within the displayed month. Default to the selection, minimum or 2000-01. */
    readonly month?: CivilDate;
    readonly defaultMonth?: CivilDate;
    readonly onMonthChange?: (firstOfMonth: CivilDate) => void;
    readonly locale?: string;
    readonly weekStartsOn?: number;
  };

export function Calendar(props: CalendarProps) {
  const {
    id: providedId,
    month,
    defaultMonth,
    onMonthChange,
    weekStartsOn: authoredWeekStart,
    locale = "en",
    min,
    max,
    isDateDisabled,
    mode,
    value,
    defaultValue,
    onChange,
    ...layout
  } = props;
  const id = useEntityId(providedId);
  const regional = new Intl.Locale(locale) as Intl.Locale & {
    getWeekInfo?: () => { firstDay: number };
    weekInfo?: { firstDay: number };
  };
  const weekStartsOn =
    authoredWeekStart ??
    ((regional.getWeekInfo?.() ?? regional.weekInfo)?.firstDay ?? 1) % 7;
  validateDateBounds({ min, max });
  if (!Number.isInteger(weekStartsOn) || weekStartsOn < 0 || weekStartsOn > 6)
    throw new RangeError("weekStartsOn must be 0–6");
  const selection = useApplicationValue<CivilDate | CivilRange | null>(
    value,
    defaultValue ?? null,
    (next) => {
      if (props.mode === "range") {
        if (typeof next !== "string") props.onChange?.(next);
      } else if (next === null || typeof next === "string")
        props.onChange?.(next);
    },
  );
  const selected = selection.current;
  const start = typeof selected === "string" ? selected : selected?.start;
  const months = useApplicationValue(
    month,
    defaultMonth ?? start ?? min ?? max ?? "2000-01-01",
    onMonthChange,
  );
  const shown = parseCivilDate(months.current);
  if (!shown) throw new RangeError("Invalid calendar month");
  // A changed bound can remove the displayed month while the picker stays mounted.
  const requested = civilDate(shown.year, shown.month, 1);
  const first =
    min && requested.slice(0, 7) < min.slice(0, 7)
      ? min.slice(0, 7) + "-01"
      : max && requested.slice(0, 7) > max.slice(0, 7)
        ? max.slice(0, 7) + "-01"
        : requested;
  const view = parseCivilDate(first)!;
  const length = monthLength(view.year, view.month);
  const offset = (civilInstant(first).getUTCDay() - weekStartsOn + 7) % 7;
  function monthButton(
    offset: -12 | -1 | 1 | 12,
    key: string,
    caption: string,
    label: string,
  ) {
    const to = adjacentMonth(first, offset);
    const enabled =
      to !== null &&
      (!min || to.slice(0, 7) >= min.slice(0, 7)) &&
      (!max || to.slice(0, 7) <= max.slice(0, 7));
    return (
      <Button
        id={`${id}/${key}`}
        label={caption}
        semanticLabel={label}
        disabled={!enabled}
        onPress={() => {
          if (to && enabled) months.change(() => to);
        }}
      />
    );
  }
  const monthFormat = new Intl.DateTimeFormat(locale, {
    calendar: "gregory",
    timeZone: "UTC",
    month: "long",
    year: "numeric",
  });
  const weekdayFormat = new Intl.DateTimeFormat(locale, {
    calendar: "gregory",
    timeZone: "UTC",
    weekday: "short",
  });
  const weekNames = Array.from({ length: 7 }, (_, index) =>
    weekdayFormat.format(
      civilInstant(civilDate(2023, 1, 1 + ((weekStartsOn + index) % 7))),
    ),
  );
  function pick(day: CivilDate) {
    if (!dateEligible(day, { min, max, isDateDisabled })) return;
    selection.change((old) => {
      if (mode !== "range") return day;
      if (!old || typeof old === "string" || old.end)
        return { start: day, end: null };
      return day < old.start
        ? { start: day, end: old.start }
        : { start: old.start, end: day };
    });
  }
  return (
    <Column {...layout} id={id}>
      <Row>
        {monthButton(-12, "previous-year", "«", "Previous year")}
        {monthButton(-1, "previous", "‹", "Previous month")}
        <TextLine
          flex={1}
          id={`${id}/month`}
          text={monthFormat.format(civilInstant(first))}
        />
        {monthButton(1, "next", "›", "Next month")}
        {monthButton(12, "next-year", "»", "Next year")}
      </Row>
      <Row gap={0}>
        {weekNames.map((name, index) => (
          <TextLine key={index} flex={1} text={name} />
        ))}
      </Row>
      <Entity id={`${id}/days`}>
        <PresentationGate open />
        <Layout kind={2} fit_height />
        <Behavior semantic_label={monthFormat.format(civilInstant(first))} />
        <Group axis={0} selection={0} />
        <Children>
          {Array.from(
            { length: Math.ceil((offset + length) / 7) },
            (_, week) => (
              <Row key={week} gap={0}>
                {Array.from({ length: 7 }, (_, column) => {
                  const n = week * 7 + column - offset + 1;
                  if (n < 1 || n > length)
                    return <Column flex={1} key={`space-${column}`} />;
                  const date = civilDate(view.year, view.month, n);
                  const active =
                    typeof selected === "string"
                      ? selected === date
                      : !!selected &&
                        date >= selected.start &&
                        date <= (selected.end ?? selected.start);
                  return (
                    <Button
                      id={`${id}/${date}`}
                      key={date}
                      label={String(n)}
                      semanticLabel={date}
                      flex={1}
                      selected={active}
                      disabled={
                        !dateEligible(date, { min, max, isDateDisabled })
                      }
                      onPress={() => pick(date)}
                    />
                  );
                })}
              </Row>
            ),
          )}
        </Children>
      </Entity>
    </Column>
  );
}
