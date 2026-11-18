import { useEntityId } from "../../scope/identity.js";
import { Button } from "../button.js";
import { Toolbar, type ToolbarProps } from "../layout.js";
import { TextLine } from "../text.js";
import { useApplicationValue } from "./application-value.js";

export interface PaginationProps extends Omit<ToolbarProps, "children"> {
  readonly totalItems: number;
  readonly pageSize: number;
  /** One-based. With no items the control has no current page. */
  readonly page?: number;
  readonly defaultPage?: number;
  readonly onPageChange?: (page: number) => void;
  readonly siblings?: number;
}

export function paginationWindow(
  page: number,
  count: number,
  siblings = 1,
): number[] {
  if (
    !Number.isSafeInteger(count) ||
    count < 0 ||
    !Number.isSafeInteger(page) ||
    !Number.isSafeInteger(siblings) ||
    siblings < 0 ||
    siblings > 10
  )
    throw new RangeError(
      "Expected an integer page, a nonnegative count and 0–10 neighboring pages",
    );
  if (count === 0) return [];
  const current = Math.min(count, Math.max(1, page));
  const pages = new Set([1, count]);
  for (
    let at = Math.max(1, current - siblings);
    at <= Math.min(count, current + siblings);
    at++
  )
    pages.add(at);
  return [...pages].sort((a, b) => a - b);
}

export function Pagination({
  id: providedId,
  totalItems,
  pageSize,
  page,
  defaultPage = 1,
  onPageChange,
  siblings = 1,
  label = "Pages",
  ...props
}: PaginationProps) {
  const id = useEntityId(providedId);
  if (
    !Number.isSafeInteger(totalItems) ||
    totalItems < 0 ||
    !Number.isSafeInteger(pageSize) ||
    pageSize < 1
  )
    throw new RangeError(
      "Expected a nonnegative item count and positive integer page size",
    );
  const count = Math.ceil(totalItems / pageSize);
  const state = useApplicationValue(page, defaultPage, onPageChange);
  const current = Math.min(count, Math.max(1, state.current));
  const pages = paginationWindow(state.current, count, siblings);
  const activate = (next: number) => {
    if (next >= 1 && next <= count && next !== current)
      state.change(() => next);
  };
  return (
    <Toolbar {...props} id={id} label={label}>
      <Button
        id={`${id}/previous`}
        label="Previous"
        disabled={current <= 1}
        onPress={() => activate(current - 1)}
      />
      {pages.flatMap((at, index) => {
        const before = pages[index - 1];
        const button = (
          <Button
            key={at}
            id={`${id}/page/${at}`}
            label={String(at)}
            semanticLabel={`Page ${at}`}
            selected={at === current}
            onPress={() => activate(at)}
          />
        );
        return before !== undefined && before + 1 < at
          ? [<TextLine key={`gap/${at}`} text="…" />, button]
          : [button];
      })}
      <Button
        id={`${id}/next`}
        label="Next"
        disabled={current >= count}
        onPress={() => activate(current + 1)}
      />
    </Toolbar>
  );
}
