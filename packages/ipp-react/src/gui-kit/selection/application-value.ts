import { useLayoutEffect, useRef, useState } from "react";

/** Application-owned composite values. Consecutive effects see the preceding
 * edit in uncontrolled mode; controlled edits always use the committed prop. */
export function useApplicationValue<T>(
  value: T | undefined,
  initial: T,
  onChange?: ((next: T) => void) | undefined,
) {
  const [own, setOwn] = useState(initial);
  const current = value === undefined ? own : value;
  const latest = useRef(current);
  useLayoutEffect(() => {
    latest.current = current;
  }, [current]);
  function change(edit: (previous: T) => T) {
    const before = latest.current;
    const next = edit(before);
    if (Object.is(before, next)) return;
    if (value === undefined) {
      latest.current = next;
      setOwn(() => next);
    }
    onChange?.(next);
  }
  return { current, change };
}
