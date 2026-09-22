/** Viewport-bounded positioned text for repeatable rendering measurements. */
import type { SurfaceItemProps } from "@ipp/react";
import type { TerminalAssets } from "./scene.js";

export interface TerminalWorkload {
  rows: number;
  columns: number;
  sequence: number;
  /** `unseen` shows a window of `unseenGlyphs` that slides by half a screen
   * per sequence step, so each step introduces glyphs not shown before. */
  mode: "idle" | "typing" | "scroll" | "full" | "unseen";
  cursor: boolean;
  glyphs: readonly number[];
  unseenGlyphs?: readonly number[] | undefined;
}

/** Rows have stable identities; typing changes only the final visible row. */
export function terminalWorkloadItems(
  assets: TerminalAssets,
  workload: TerminalWorkload,
): SurfaceItemProps[] {
  const { rows, columns, sequence, mode, cursor, glyphs } = workload;
  const unseen = workload.unseenGlyphs ?? [];
  const slide = Math.floor((rows * columns) / 2);
  if (
    !Number.isInteger(rows) ||
    rows < 1 ||
    rows > 80 ||
    !Number.isInteger(columns) ||
    columns < 1 ||
    columns > 160 ||
    !Number.isSafeInteger(sequence) ||
    sequence < 0 ||
    glyphs.length === 0 ||
    (mode === "unseen" && sequence * slide + rows * columns > unseen.length)
  )
    throw new Error("Invalid terminal workload dimensions or sequence");
  const line = 2.1 / rows;
  const advance = 3.4 / columns;
  const fontSize = Math.min(line * 0.85, advance / 0.6);
  const items: SurfaceItemProps[] = [];
  for (let row = 0; row < rows; row++) {
    items.push({
      key: `row-${row}`,
      content: {
        kind: "glyphRun",
        glyphs: Array.from({ length: columns }, (_, column) => {
          if (mode === "unseen")
            return {
              glyphId: unseen[sequence * slide + row * columns + column]!,
              position: [column * advance, 0],
            };
          const edit =
            mode === "full" ||
            mode === "scroll" ||
            (mode === "typing" && row === rows - 1 && column === columns - 1);
          const sourceRow = row + (mode === "scroll" ? sequence : 0);
          const index =
            sourceRow * columns +
            Math.floor(sourceRow / 3) +
            column +
            (edit && mode !== "scroll" ? sequence : 0);
          return {
            glyphId: glyphs[index % glyphs.length]!,
            position: [column * advance, 0],
          };
        }),
      },
      asset: assets.font,
      position: [0.2, 0.15 + (row + 0.8) * line],
      fontSize,
      color: [0.65, 0.92, 0.8, 1],
    });
  }
  items.push({
    key: "cursor",
    content: { kind: "drawing" },
    asset: assets.panel,
    position: [3.6, 2.28],
    scale: [0.06, 0.08],
    color: [0.3, 1, 0.5, 1],
    opacity: cursor ? 1 : 0,
  });
  return items;
}
