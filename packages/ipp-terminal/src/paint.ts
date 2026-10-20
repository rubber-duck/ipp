import type { Client, Command, EntityRef } from "@ipp/client";
import type { RenderColor, RenderFrame } from "@gespenst/core/headless";
import {
  alias,
  apply,
  component,
  handle,
  TerminalBatchError,
} from "./commands.js";
import type { TerminalContract, TerminalFont } from "./types.js";

type Color = readonly [number, number, number, number];
interface Glyph {
  glyph_id: number;
  position: readonly [number, number];
  color: Color;
}

function linear(value: number): number {
  const v = value / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}
function color(value: RenderColor): Color {
  return [linear(value.r), linear(value.g), linear(value.b), value.a ?? 1];
}
function tint(value: Color) {
  return { red: value[0], green: value[1], blue: value[2], alpha: value[3] };
}

/** Damage becomes ordinary Canvas entities and packed glyph rows. */
export class TerminalPaint {
  private root!: bigint;
  private background!: bigint;
  private cursor!: bigint;
  private readonly rows = new Map<number, bigint>();
  private closed = false;

  constructor(
    private readonly client: Client,
    private readonly contract: TerminalContract,
    private readonly target: bigint,
    private readonly fontSource: string,
    private readonly font: TerminalFont,
    private readonly fontSize: number,
    readonly cellWidth: number,
    readonly cellHeight: number,
    private readonly transparentBackground: boolean,
  ) {}

  async initialize(): Promise<void> {
    const commands: Command[] = [];
    for (let id = 0; id < 3; id++) {
      commands.push(
        {
          kind: "create",
          alias: id,
          metadata: {
            symbolicId: null,
            classes: [
              id === 0
                ? "terminal-content"
                : id === 1
                  ? "terminal-background"
                  : "terminal-cursor",
            ],
          },
        },
        {
          kind: "placeEntity",
          entity: alias(id),
          placement: {
            parent: id === 0 ? handle(this.target) : alias(0),
            before: null,
          },
        },
      );
    }
    commands.push(
      component(this.client, "CanvasStyle", alias(0)),
      component(this.client, "CanvasStyle", alias(1), {
        opacity: this.transparentBackground ? 0 : 1,
      }),
      component(this.client, "CanvasBox", alias(1)),
      component(this.client, "CanvasBox", alias(2)),
    );
    const result = await apply(this.client, commands);
    this.root = result.aliases.find((a) => a.alias === 0)!.id;
    this.background = result.aliases.find((a) => a.alias === 1)!.id;
    this.cursor = result.aliases.find((a) => a.alias === 2)!.id;
  }

  async update(frame: RenderFrame): Promise<void> {
    if (this.closed) return;
    const commands: Command[] = [];
    const updates: { y: number; alias: number }[] = [];
    for (const [y, id] of this.rows)
      if (y >= frame.rows) {
        commands.push({ kind: "deleteSubtree", root: handle(id) });
        this.rows.delete(y);
      }
    let nextAlias = 0;
    const create = (parent: EntityRef, classes: string[] = []) => {
      const id = nextAlias++;
      commands.push(
        { kind: "create", alias: id, metadata: { symbolicId: null, classes } },
        {
          kind: "placeEntity",
          entity: alias(id),
          placement: { parent, before: null },
        },
      );
      return alias(id);
    };
    const box = (
      parent: EntityRef,
      x: number,
      y: number,
      width: number,
      height: number,
      value: Color,
    ) => {
      if (width <= 0 || height <= 0) return;
      const entity = create(parent);
      commands.push(
        component(this.client, "CanvasStyle", entity, { x, y, ...tint(value) }),
        component(this.client, "CanvasBox", entity, { width, height }),
      );
    };
    const layout = this.client.components.CanvasGlyphRun!.fields.glyphs!.rows!;
    const defaultBackground = color(frame.colors.background);
    for (const row of frame.changedRows) {
      const previous = this.rows.get(row.y);
      if (previous !== undefined)
        commands.push({ kind: "deleteSubtree", root: handle(previous) });
      const entity = create(handle(this.root), ["terminal-row"]);
      updates.push({ y: row.y, alias: nextAlias - 1 });
      commands.push(
        component(this.client, "CanvasStyle", entity, {
          y: row.y * this.cellHeight,
        }),
      );
      const glyphs: Glyph[] = [];
      let background: { x: number; end: number; color: Color } | undefined;
      const flush = () => {
        if (background)
          box(
            entity,
            background.x * this.cellWidth,
            0,
            (background.end - background.x) * this.cellWidth,
            this.cellHeight,
            background.color,
          );
        background = undefined;
      };
      for (const cell of row.cells) {
        if (cell.width === "spacer-tail" || cell.width === "spacer-head")
          continue;
        let fg = color(cell.foreground ?? frame.colors.foreground);
        let bg = color(cell.background ?? frame.colors.background);
        if (cell.style.inverse) [fg, bg] = [bg, fg];
        if (
          row.selection &&
          cell.x >= row.selection.start &&
          cell.x < row.selection.end
        ) {
          bg = color(
            frame.colors.selectionBackground ?? frame.colors.foreground,
          );
          fg = color(
            frame.colors.selectionForeground ?? frame.colors.background,
          );
        }
        if (cell.style.faint) fg = [fg[0], fg[1], fg[2], fg[3] * 0.5];
        const width = cell.width === "wide" ? 2 : 1;
        if (bg.some((v, i) => v !== defaultBackground[i])) {
          if (
            background &&
            background.end === cell.x &&
            background.color.every((v, i) => v === bg[i])
          )
            background.end += width;
          else {
            flush();
            background = { x: cell.x, end: cell.x + width, color: bg };
          }
        } else flush();
        if (!cell.style.invisible) {
          for (const codepoint of cell.text) {
            if (codepoint === " " || codepoint === "\0") continue;
            glyphs.push({
              glyph_id: this.font.glyphs[codepoint] ?? 0,
              position: [
                cell.x * this.cellWidth,
                (this.font.ascent / this.font.unitsPerEm) * this.fontSize,
              ],
              color: fg,
            });
          }
          if (cell.style.underline)
            box(
              entity,
              cell.x * this.cellWidth,
              this.cellHeight - 2,
              width * this.cellWidth,
              1,
              fg,
            );
          if (cell.style.strikethrough)
            box(
              entity,
              cell.x * this.cellWidth,
              this.cellHeight * 0.55,
              width * this.cellWidth,
              1,
              fg,
            );
          if (cell.style.overline)
            box(
              entity,
              cell.x * this.cellWidth,
              0,
              width * this.cellWidth,
              1,
              fg,
            );
        }
      }
      flush();
      const glyphEntity = create(entity);
      commands.push(
        component(this.client, "CanvasGlyphRun", glyphEntity, {
          source: this.fontSource,
          font_size: this.fontSize,
          glyphs: this.contract.encodeRowsTable(layout, {
            nextSlot: glyphs.length,
            rows: new Map(glyphs.map((value, index) => [index, value])),
          }),
        }),
      );
    }
    const cursor = frame.cursor;
    const cursorWidth = cursor.style === "bar" ? 2 : this.cellWidth;
    const cursorHeight = cursor.style === "underline" ? 2 : this.cellHeight;
    commands.push(
      component(
        this.client,
        "CanvasStyle",
        handle(this.background),
        {
          ...tint(defaultBackground),
          opacity: this.transparentBackground ? 0 : 1,
        },
        true,
      ),
      component(
        this.client,
        "CanvasBox",
        handle(this.background),
        {
          width: frame.cols * this.cellWidth,
          height: frame.rows * this.cellHeight,
        },
        true,
      ),
      component(
        this.client,
        "CanvasStyle",
        handle(this.cursor),
        {
          x: (cursor.x ?? 0) * this.cellWidth,
          y:
            (cursor.y ?? 0) * this.cellHeight +
            (cursor.style === "underline" ? this.cellHeight - 2 : 0),
          ...tint(color(frame.colors.cursor ?? frame.colors.foreground)),
          opacity:
            cursor.visible && cursor.x !== null && cursor.y !== null
              ? cursor.style === "block"
                ? 0.5
                : 1
              : 0,
        },
        true,
      ),
      component(
        this.client,
        "CanvasBox",
        handle(this.cursor),
        { width: cursorWidth, height: cursorHeight },
        true,
      ),
      {
        kind: "placeEntity",
        entity: handle(this.cursor),
        placement: { parent: handle(this.root), before: null },
      },
    );
    const result = await apply(this.client, commands);
    for (const update of updates)
      this.rows.set(
        update.y,
        result.aliases.find((a) => a.alias === update.alias)!.id,
      );
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (this.root !== undefined) {
      const current = await this.client.inspectPage({
        collection: "entities",
        target: this.root,
        limit: 1,
      });
      if (current.entities.some((entity) => entity.id === this.root))
        await apply(this.client, [
          { kind: "deleteSubtree", root: handle(this.root) },
        ]).catch((error: unknown) => {
          // Another owner may remove the ancestor between inspection and deletion.
          if (
            !(
              error instanceof TerminalBatchError &&
              error.reason === "InvalidEntity"
            )
          )
            throw error;
        });
    }
  }
}
