/** Gregorian civil dates use four-digit years and do not represent instants. */
export type CivilDate = string;
export interface CivilRange {
  readonly start: CivilDate;
  readonly end: CivilDate | null;
}
export interface DateBounds {
  readonly min?: CivilDate | undefined;
  readonly max?: CivilDate | undefined;
  readonly isDateDisabled?: ((date: CivilDate) => boolean) | undefined;
}

export function civilDate(year: number, month: number, day: number): CivilDate {
  if (
    ![year, month, day].every(Number.isInteger) ||
    year < 1 ||
    year > 9999 ||
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > monthLength(year, month)
  )
    throw new RangeError("Expected a valid Gregorian date in years 0001–9999");
  return [
    String(year).padStart(4, "0"),
    String(month).padStart(2, "0"),
    String(day).padStart(2, "0"),
  ].join("-");
}

export function monthLength(year: number, month: number) {
  if (month === 2)
    return year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 29 : 28;
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

export function parseCivilDate(date: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!match) return null;
  const year = Number(match[1]),
    month = Number(match[2]),
    day = Number(match[3]);
  return year > 0 &&
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= monthLength(year, month)
    ? { year, month, day }
    : null;
}

export function validateDateBounds({ min, max }: DateBounds) {
  if (
    (min !== undefined && !parseCivilDate(min)) ||
    (max !== undefined && !parseCivilDate(max)) ||
    (min !== undefined && max !== undefined && min > max)
  )
    throw new RangeError("Invalid civil date limits");
}

export function dateEligible(
  date: CivilDate,
  { min, max, isDateDisabled }: DateBounds,
) {
  return (
    !!parseCivilDate(date) &&
    (!min || date >= min) &&
    (!max || date <= max) &&
    !isDateDisabled?.(date)
  );
}

/** UTC is only the formatting carrier, including years below 100. */
export function civilInstant(date: CivilDate): Date {
  const parts = parseCivilDate(date);
  if (!parts) throw new RangeError("Invalid civil date");
  const instant = new Date(0);
  instant.setUTCFullYear(parts.year, parts.month - 1, parts.day);
  return instant;
}

export function adjacentMonth(
  date: CivilDate,
  offset: -12 | -1 | 1 | 12,
): CivilDate | null {
  const parts = parseCivilDate(date);
  if (!parts) throw new RangeError("Invalid civil date");
  const n = (parts.year - 1) * 12 + parts.month - 1 + offset;
  if (n < 0 || n >= 9999 * 12) return null;
  return civilDate(Math.floor(n / 12) + 1, (n % 12) + 1, 1);
}
