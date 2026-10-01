/** Viewport-bounded positioned text for repeatable rendering measurements. */
import { createElement } from "react";
import type { RowsInput, RowsLayoutDescriptor } from "@ipp/client";
import { Entity } from "@ipp/react";
import { Drawing, GlyphRun, Style } from "@ipp/react/gui";
import type { ReactNode } from "react";
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

export interface TerminalGlyphRowsCodec {
  readonly layout: RowsLayoutDescriptor;
  encodeRowsTable<Row extends object>(
    layout: RowsLayoutDescriptor,
    rows: RowsInput<Row>,
  ): Uint8Array<ArrayBuffer>;
}

interface PositionedCanvasGlyph {
  glyph_id: number;
  position: readonly [number, number];
}

/** Rows have stable entity identities; typing changes only the final visible row. */
export function terminalWorkloadLayers(
  assets: TerminalAssets,
  workload: TerminalWorkload,
  codec: TerminalGlyphRowsCodec,
): ReactNode[] {
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
  const layers: ReactNode[] = [];
  for (let row = 0; row < rows; row++) {
    const positioned: PositionedCanvasGlyph[] = Array.from(
      { length: columns },
      (_, column) => {
        if (mode === "unseen")
          return {
            glyph_id: unseen[sequence * slide + row * columns + column]!,
            position: [column * advance, 0] as const,
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
          glyph_id: glyphs[index % glyphs.length]!,
          position: [column * advance, 0] as const,
        };
      },
    );
    const glyphBytes = codec.encodeRowsTable(codec.layout, {
      nextSlot: positioned.length,
      rows: new Map(positioned.map((glyph, index) => [index, glyph])),
    });
    layers.push(
      createElement(
        Entity,
        { key: `row-${row}`, id: `row-${row}` },
        createElement(Style, {
          x: 0.2,
          y: 0.15 + (row + 0.8) * line,
          red: 0.65,
          green: 0.92,
          blue: 0.8,
        }),
        createElement(GlyphRun, {
          source: assets.font.source,
          font_size: fontSize,
          glyphs: glyphBytes,
        }),
      ),
    );
  }
  layers.push(
    createElement(
      Entity,
      { key: "cursor", id: "cursor" },
      createElement(Style, {
        x: 3.6,
        y: 2.28,
        scale_x: 0.06,
        scale_y: 0.08,
        red: 0.3,
        green: 1,
        blue: 0.5,
        opacity: cursor ? 1 : 0,
      }),
      createElement(Drawing, { source: assets.panel.source }),
    ),
  );
  return layers;
}
