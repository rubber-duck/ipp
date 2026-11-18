import { useEntityId } from "../../scope/identity.js";
import type { ContainerProps } from "../layout.js";
import { useApplicationValue } from "../selection/application-value.js";
import { Segments, type SegmentValues } from "./segments.js";

function seconds(text: string) {
  const match = /^(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(text);
  if (!match) return null;
  const hour = Number(match[1]),
    minute = Number(match[2]),
    second = Number(match[3] ?? 0);
  return hour < 24 && minute < 60 && second < 60
    ? hour * 3600 + minute * 60 + second
    : null;
}

function format(at: number, precise: boolean) {
  const parts = [Math.floor(at / 3600), Math.floor(at / 60) % 60, at % 60].map(
    (v) => String(v).padStart(2, "0"),
  );
  return parts.slice(0, precise ? 3 : 2).join(":");
}

export interface TimeInputProps extends Omit<ContainerProps, "children"> {
  readonly label?: string;
  readonly locale?: string;
  readonly hour12?: boolean;
  readonly showSeconds?: boolean;
  readonly min?: string;
  readonly max?: string;
  readonly stepSeconds?: number;
  readonly required?: boolean;
  readonly value?: string | null;
  readonly defaultValue?: string | null;
  readonly onChange?: (time: string | null) => void;
}

/** Values are local wall-clock strings independent of timezone or daylight-saving rules. */
export function TimeInput({
  id: providedId,
  label = "Time",
  locale = "en",
  hour12,
  showSeconds = false,
  min = "00:00",
  max = "23:59:59",
  stepSeconds = showSeconds ? 1 : 60,
  required = false,
  value,
  defaultValue = null,
  onChange,
  ...layout
}: TimeInputProps) {
  const id = useEntityId(providedId);
  const state = useApplicationValue(value, defaultValue, onChange);
  const lower = seconds(min),
    upper = seconds(max);
  if (
    lower === null ||
    upper === null ||
    lower > upper ||
    !Number.isSafeInteger(stepSeconds) ||
    stepSeconds < 1 ||
    (state.current !== null && seconds(state.current) === null)
  )
    throw new RangeError("Invalid time or step");
  const display = new Intl.DateTimeFormat(locale, {
    timeZone: "UTC",
    hour: "numeric",
    minute: "2-digit",
    ...(showSeconds ? { second: "2-digit" } : {}),
    ...(hour12 === undefined ? {} : { hour12 }),
  });
  const twelve = display.resolvedOptions().hour12 === true;
  const periods = [0, 12].map(
    (h) =>
      display
        .formatToParts(Date.UTC(2000, 0, 1, h))
        .find((p) => p.type === "dayPeriod")?.value ?? (h ? "PM" : "AM"),
  );
  const segments = display
    .formatToParts(0)
    .filter((p) => ["hour", "minute", "second", "dayPeriod"].includes(p.type))
    .map((p) => ({
      key: p.type,
      label: p.type === "dayPeriod" ? "Period" : p.type,
    }));
  function split(text: string | null): SegmentValues {
    if (!text) return Object.fromEntries(segments.map((s) => [s.key, ""]));
    const at = seconds(text)!;
    const h = Math.floor(at / 3600);
    return {
      hour: String(twelve ? h % 12 || 12 : h).padStart(2, "0"),
      minute: String(Math.floor(at / 60) % 60).padStart(2, "0"),
      second: String(at % 60).padStart(2, "0"),
      dayPeriod: periods[Number(h >= 12)]!,
    };
  }
  function parse(p: SegmentValues) {
    if (
      !/^\d{1,2}$/.test(p.hour ?? "") ||
      !/^\d{1,2}$/.test(p.minute ?? "") ||
      (showSeconds && !/^\d{1,2}$/.test(p.second ?? ""))
    )
      return null;
    let h = Number(p.hour);
    const m = Number(p.minute),
      s = showSeconds ? Number(p.second) : 0;
    if (twelve) {
      const period = periods.findIndex(
        (v) =>
          v.toLocaleLowerCase(locale) ===
          (p.dayPeriod ?? "").toLocaleLowerCase(locale),
      );
      if (period < 0 || h < 1 || h > 12) return null;
      h = (h % 12) + period * 12;
    }
    return h > 23 || m > 59 || s > 59
      ? null
      : format(h * 3600 + m * 60 + s, showSeconds);
  }
  const stepTime = (text: string, part: string, direction: number) => {
    const at = seconds(text)!;
    const unit =
      part === "hour"
        ? 3600
        : part === "minute"
          ? 60
          : part === "dayPeriod"
            ? 43200
            : 1;
    const requested = at + direction * Math.max(unit, stepSeconds);
    const last =
      lower + Math.floor((upper - lower) / stepSeconds) * stepSeconds;
    if (showSeconds) {
      const round = direction > 0 ? Math.ceil : Math.floor;
      return format(
        Math.max(
          lower,
          Math.min(
            last,
            lower + round((requested - lower) / stepSeconds) * stepSeconds,
          ),
        ),
        true,
      );
    }
    // A minute-only display can commit only step-aligned whole minutes.
    // Start at the requested minute and try towards the bound, then back to the current value.
    const firstMinute = Math.ceil(lower / 60) * 60;
    const lastMinute = Math.floor(upper / 60) * 60;
    const candidate = Math.max(
      firstMinute,
      Math.min(
        lastMinute,
        (direction > 0 ? Math.ceil : Math.floor)(requested / 60) * 60,
      ),
    );
    for (const delta of [direction * 60, -direction * 60]) {
      for (
        let minute = candidate;
        minute >= firstMinute &&
        minute <= lastMinute &&
        (minute - at) * direction > 0;
        minute += delta
      ) {
        if ((minute - lower) % stepSeconds === 0) return format(minute, false);
      }
    }
    return text;
  };
  return (
    <Segments
      {...layout}
      id={id}
      label={label}
      value={state.current}
      required={required}
      segments={segments}
      split={split}
      parse={parse}
      paste={(text) => (seconds(text) === null ? null : text)}
      eligible={(text) => {
        const at = seconds(text);
        return (
          at !== null &&
          at >= lower &&
          at <= upper &&
          (at - lower) % stepSeconds === 0
        );
      }}
      step={stepTime}
      onCommit={(next) => state.change(() => next)}
    />
  );
}
