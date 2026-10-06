/**
 * A range slider's two thumbs as focus parts: `buttons` ends in `range`,
 * whose thumbs are the Tab stops before the `sliders` panel's first control
 * `c`. A lifecycle watch of the range's two value fields records its values
 * as React's value callbacks read them.
 */
import { check } from "../../../harness/page/checks.js";
import {
  canvasOf,
  json,
  openCompositePage,
  type CompositeSetup,
} from "./composites.js";
import { PANEL, buttonsPanel, slidersPanel } from "./panels.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [buttonsPanel([0, 0]), slidersPanel([PANEL, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const records = await page.watchValues("range", "GuiSlider", [
    "value",
    "upper",
  ]);
  const values = () =>
    records.map(
      ({ values: [lower, upper] }) => [Number(lower), Number(upper)] as const,
    );
  return {
    ...page.steps,
    /** Frames until the range holds `values`. */
    async expectRange(expected: readonly [number, number]) {
      let last: unknown;
      await page.until(
        async () => {
          const { fields } = await page.read("range");
          last = [fields.value, fields.upper];
          return fields.value === expected[0] && fields.upper === expected[1];
        },
        () => `range holds ${json(last)}, expected ${json(expected)}`,
      );
      return expected;
    },
    /**
     * A client writes the range's fields, `value` alone, `upper` alone or
     * both in one component write; returns why the World refused it, or null.
     */
    async writeRange(fields: { value?: number; upper?: number }) {
      const { client } = page.panel("buttons");
      const descriptor = client.components.GuiSlider!;
      const writes = Object.entries(fields).map(([name, value]) => ({
        offset: descriptor.fields[name]!.offset,
        value: { kind: "f32" as const, value: value! },
      }));
      check(writes.length > 0, "Nothing to write");
      const target = {
        kind: "handle" as const,
        id: page.entry("range").entity,
      };
      const outcome = await client.batch([
        writes.length === 1
          ? {
              kind: "setField",
              entity: target,
              component: descriptor.id,
              field: writes[0]!,
            }
          : {
              kind: "insertComponent",
              entity: target,
              component: descriptor.id,
              fields: writes,
              adopt: true,
            },
      ]);
      await page.host.presentation.frame(page.view);
      return outcome.ok ? null : outcome.error.reason;
    },
    /** The range's value records since `from`, each with both values. */
    rangeRecords(from: number) {
      return values().slice(from);
    },
    /** How many value records of the range arrived so far. */
    rangeRecordCount() {
      return records.length;
    },
  };
}
