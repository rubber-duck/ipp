import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { GuiClientInput } from "@ipp/client";
import { Children, Entity, PresentationGate } from "../components.js";
import { Behavior, InputTarget, Layout } from "../gui/components.js";
import type { GuiControlHandle } from "../gui/control-ref.js";
import { useTextMeasurements } from "./text-measurement.js";
import { useField } from "../scope/field.js";
import { useTargetInput } from "../gui/target-input.js";
import { useEntityId } from "../scope/identity.js";
import { TextLine } from "./text.js";
import {
  Row,
  LAYOUT_ROW,
  useLayoutFields,
  type GuiKitLayoutProps,
} from "./layout.js";
import { useGuiKit } from "./kit.js";
import {
  importTokenDocument,
  normalizeTokenDocument,
  replaceTokenSelection,
  serializeTokenDocument,
  tokenUnits,
  type TokenClipboardData,
  type TokenDocument,
  type TokenPart,
  type TokenSelection,
} from "./token-document.js";

export interface TokenInputProps extends GuiKitLayoutProps {
  readonly id?: string;
  readonly label: string;
  readonly value?: TokenDocument;
  readonly defaultValue?: TokenDocument;
  readonly disabled?: boolean;
  readonly readOnly?: boolean;
  readonly onChange?: (document: TokenDocument) => void;
  readonly onSelectionChange?: (selection: TokenSelection) => void;
  readonly clipboard?: {
    write(data: TokenClipboardData): Promise<void>;
    read?(): Promise<TokenClipboardData>;
    acceptsToken?(token: Extract<TokenPart, { kind: "token" }>): boolean;
  };
  readonly onClipboardError?: (error: unknown) => void;
}

const empty: TokenDocument = [];
export function TokenInput({
  id: provided,
  label,
  value,
  defaultValue = empty,
  disabled = false,
  readOnly = false,
  onChange,
  onSelectionChange,
  clipboard,
  onClipboardError,
  ...layout
}: TokenInputProps) {
  const id = useEntityId(provided);
  const kit = useGuiKit();
  const [own, setOwn] = useState(() => normalizeTokenDocument(defaultValue));
  const canonical = useMemo(
    () => normalizeTokenDocument(value ?? own),
    [value, own],
  );
  const identity = JSON.stringify(canonical);
  const units = useMemo(() => tokenUnits(canonical), [canonical]);
  const [selection, setSelection] = useState<TokenSelection>(() => [
    units.length,
    units.length,
  ]);
  const latest = useRef({ value: canonical, selection, epoch: 0 });
  const proposed = useRef<{
    identity: string;
    value: TokenDocument;
    selection: TokenSelection;
  } | null>(null);
  const previous = useRef(identity);
  const [, scheduleObservation] = useState(0);
  const [editLifetime, setEditLifetime] = useState(0);
  const [focused, setFocused] = useState(false);
  const retiring = useRef(false);
  const activePointer = useRef<{ id: bigint; anchor: number } | null>(null);
  function select(next: TokenSelection) {
    if (
      latest.current.selection[0] === next[0] &&
      latest.current.selection[1] === next[1]
    )
      return;
    latest.current.selection = next;
    latest.current.epoch++;
    setSelection(next);
    onSelectionChange?.(next);
  }
  // Every proposal receives a render even when its controlled owner rejects it.
  // Several inputs in one turn may build on that turn's unacknowledged proposal.
  useLayoutEffect(() => {
    const pending = proposed.current;
    proposed.current = null;
    const replaced = previous.current !== identity;
    previous.current = identity;
    latest.current.value = canonical;
    if (pending?.identity === identity) {
      select(pending.selection);
      return;
    }
    if (!replaced || value === undefined) return;
    retiring.current = true;
    activePointer.current = null;
    latest.current.epoch++;
    const [anchor, caret] = latest.current.selection;
    select([Math.min(anchor, units.length), Math.min(caret, units.length)]);
    setEditLifetime((n) => n + 1);
  });
  function apply(
    inserted: TokenDocument,
    range = proposed.current?.selection ?? latest.current.selection,
  ) {
    if (disabled || readOnly || retiring.current) return;
    const prior = proposed.current?.value ?? latest.current.value;
    let result: ReturnType<typeof replaceTokenSelection>;
    try {
      result = replaceTokenSelection(prior, range, inserted);
    } catch (error) {
      onClipboardError?.(error);
      return;
    }
    const candidate = JSON.stringify(result.value);
    if (candidate === JSON.stringify(prior)) {
      select(result.selection);
      return;
    }
    latest.current.epoch++;
    if (value === undefined) {
      latest.current.value = result.value;
      setOwn(result.value);
      select(result.selection);
    } else {
      proposed.current = { identity: candidate, ...result };
      scheduleObservation((n) => n + 1);
    }
    onChange?.(result.value);
  }
  useLayoutEffect(
    () => () => {
      latest.current.epoch++;
    },
    [disabled, readOnly, clipboard],
  );
  const instanceId = `${id}/input/${editLifetime}`;
  const fields = useLayoutFields(layout, {
    width: kit.unit(240),
    height: kit.unit(kit.tokens.controlHeight),
  });
  const size = kit.typeSize("body");
  const display = units.map((unit) =>
    unit.kind === "text" ? unit.text : `[${unit.label}]`,
  );
  const measured = useTextMeasurements(
    [...new Set(display)].map((text) => [text, size] as const),
  );
  const widths = new Map(
    [...new Set(display)].map((text) => [text, measured.width(text, size)]),
  );
  const positions = [0];
  for (const text of display)
    positions.push(positions[positions.length - 1]! + widths.get(text)!);
  const viewport = useField(instanceId, "CanvasBounds", "local_width");
  const inset = Number(fields.padding_left ?? 0);
  const room =
    viewport.status === "ready" && typeof viewport.value === "number"
      ? Math.max(0, viewport.value - inset - Number(fields.padding_right ?? 0))
      : Infinity;
  const shiftX = focused
    ? Math.max(0, (positions[selection[1]] ?? 0) - room + 1)
    : 0;
  function onInput(input: GuiClientInput) {
    if (disabled || retiring.current) return;
    if (input.kind === "pointer") {
      if (input.phase === "cancel") {
        if (activePointer.current?.id === input.pointer)
          activePointer.current = null;
        return;
      }
      if (!measured.ready) return;
      if (input.phase === "down" && input.button !== "primary") return;
      if (input.phase !== "down" && activePointer.current?.id !== input.pointer)
        return;
      const local = input.point[0] - inset + shiftX;
      // Pick the nearer edge of each actual glyph or whole token cell.
      let index = 0;
      while (
        index < units.length &&
        local >= (positions[index]! + positions[index + 1]!) * 0.5
      )
        index++;
      if (input.phase === "down") {
        activePointer.current = { id: input.pointer, anchor: index };
        setFocused(true);
        select([index, index]);
      } else {
        select([activePointer.current!.anchor, index]);
        if (input.phase === "up") activePointer.current = null;
      }
      return;
    }
    if (input.kind === "wheel") return;
    setFocused(true);
    if (input.kind === "text") {
      const inserted = importTokenDocument(
        input,
        input.paste ? clipboard?.acceptsToken : undefined,
      );
      if (inserted.length) apply(inserted);
      return;
    }
    if (input.kind !== "key" || !input.pressed) return;
    const [anchor, caret] = latest.current.selection;
    const currentUnits = tokenUnits(latest.current.value);
    const key = input.key.toLowerCase();
    const shift = !!(input.modifiers & 1),
      shortcut = !!(input.modifiers & 10);
    if (shortcut && key === "a") {
      select([0, currentUnits.length]);
      return;
    }
    if (shortcut && (key === "c" || key === "x" || key === "v")) {
      if (!clipboard) return;
      const epoch = latest.current.epoch;
      const checked = (action: () => void) => {
        if (latest.current.epoch === epoch) action();
      };
      if (key === "v" && clipboard.read && !readOnly) {
        void clipboard
          .read()
          .then((data) =>
            checked(() => {
              const inserted = importTokenDocument(
                data,
                clipboard.acceptsToken,
              );
              if (inserted.length) apply(inserted);
            }),
          )
          .catch(onClipboardError ?? (() => {}));
      } else if (key !== "v" && anchor !== caret) {
        const copied = serializeTokenDocument(
          currentUnits.slice(Math.min(anchor, caret), Math.max(anchor, caret)),
        );
        void clipboard
          .write(copied)
          .then(() =>
            checked(() => {
              if (key === "x") apply([]);
            }),
          )
          .catch(onClipboardError ?? (() => {}));
      }
      return;
    }
    const last = currentUnits.length;
    const move =
      key === "home"
        ? 0
        : key === "end"
          ? last
          : key === "arrowleft"
            ? shift || anchor === caret
              ? Math.max(0, caret - 1)
              : Math.min(anchor, caret)
            : key === "arrowright"
              ? shift || anchor === caret
                ? Math.min(last, caret + 1)
                : Math.max(anchor, caret)
              : undefined;
    if (move !== undefined) {
      select([shift ? anchor : move, move]);
      return;
    }
    if (key === "backspace" || key === "delete")
      apply(
        [],
        anchor !== caret
          ? [anchor, caret]
          : key === "backspace"
            ? [Math.max(0, caret - 1), caret]
            : [caret, Math.min(last, caret + 1)],
      );
  }
  const target = useTargetInput(
    onInput,
    () => {
      activePointer.current = null;
      latest.current.epoch++;
      setFocused(false);
    },
    () => setFocused(true),
  );
  const attach = useCallback(
    (handle: GuiControlHandle | null) => {
      if (handle) retiring.current = false;
      target(handle);
    },
    [target],
  );
  const [lo, hi] = [Math.min(...selection), Math.max(...selection)];
  // Native font measurement supplies both paint cell widths and hit boundaries.
  // A caret leaf has zero advance, so showing it cannot shift the hit positions.
  const cells = [];
  for (let i = 0; i <= units.length; i++) {
    if (focused && i === selection[1])
      cells.push(<TextLine key="caret" text="│" width={0} tone="accent" />);
    const part = units[i];
    if (!part) continue;
    cells.push(
      <TextLine
        key={part.kind === "token" ? `token-${part.id}` : `text-${i}`}
        id={`${instanceId}/cell/${i}`}
        text={display[i]!}
        {...(measured.ready
          ? { width: widths.get(display[i]!)! }
          : { fitWidth: true })}
        tone={
          focused && i >= lo && i < hi
            ? "accent"
            : part.kind === "token"
              ? "neutral"
              : "text"
        }
      />,
    );
  }
  return (
    <>
      {measured.nodes}
      <Entity key={editLifetime} id={instanceId}>
        <PresentationGate open />
        <Layout kind={LAYOUT_ROW} {...fields} clip />
        <Behavior enabled={!disabled} focusable semantic_label={label} />
        <InputTarget ref={attach} />
        <Children>
          <Row
            id={`${instanceId}/content`}
            layout={{ margin_left: -shiftX }}
            gap={0}
            fitWidth
          >
            {cells}
          </Row>
        </Children>
      </Entity>
    </>
  );
}
