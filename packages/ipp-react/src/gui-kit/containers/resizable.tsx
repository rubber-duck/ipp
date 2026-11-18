import { Fragment, useRef, useState, type ReactNode } from "react";
import { useEntityId } from "../../scope/identity.js";
import { useField } from "../../scope/field.js";
import {
  useLayoutFields,
  Column,
  Row,
  type ContainerProps,
} from "../layout.js";
import { useGuiKit } from "../kit.js";
import { TextLine } from "../text.js";
import { useApplicationValue } from "../selection/application-value.js";
import { Grip } from "./grip.js";

export interface SplitPane {
  readonly key: string;
  readonly content: ReactNode;
  readonly initialSize?: number;
  readonly min?: number;
  readonly max?: number;
  readonly collapsible?: boolean;
}
export type PaneSizes = Readonly<Record<string, number>>;
export interface ResizableProps extends Omit<ContainerProps, "children"> {
  readonly panes: readonly SplitPane[];
  readonly orientation?: "horizontal" | "vertical";
  readonly sizes?: PaneSizes;
  readonly defaultSizes?: PaneSizes;
  readonly onSizesChange?: (sizes: PaneSizes) => void;
}

/** Water-fill the available extent among flexible panes. Infeasible minima shrink
 * proportionally. Excess beyond all maxima stays as unused space. */
export function fitPanes(
  panes: readonly SplitPane[],
  intent: PaneSizes,
  total: number,
): number[] {
  if (
    !Number.isFinite(total) ||
    total < 0 ||
    new Set(panes.map((p) => p.key)).size !== panes.length
  )
    throw new RangeError("Invalid pane extent or duplicate keys");
  const bounds = panes.map((p) => {
    const min = p.min ?? 0,
      max = p.max ?? Infinity,
      want =
        intent[p.key] ??
        p.initialSize ??
        (panes.length ? total / panes.length : 0);
    if (
      !Number.isFinite(min) ||
      min < 0 ||
      Number.isNaN(max) ||
      max < min ||
      !Number.isFinite(want) ||
      want < 0
    )
      throw new RangeError("Invalid pane constraints");
    return p.collapsible && want === 0
      ? { min: 0, max: 0, want: 0 }
      : { min, max, want };
  });
  const minimum = bounds.reduce((sum, b) => sum + b.min, 0);
  if (minimum > total) return bounds.map((b) => (b.min * total) / minimum);
  const result = bounds.map((b) => Math.min(b.max, Math.max(b.min, b.want)));
  let delta = total - result.reduce((a, b) => a + b, 0);
  for (let pass = 0; pass <= panes.length && Math.abs(delta) > 1e-6; pass++) {
    const grow = delta > 0;
    const eligible = bounds.map((b, i) =>
      grow ? b.max - result[i]! : result[i]! - b.min,
    );
    const active = eligible.filter((n) => n > 1e-9).length;
    if (!active) break;
    const share = Math.abs(delta) / active;
    eligible.forEach((room, i) => {
      if (room > 0) {
        const change = Math.min(share, room) * (grow ? 1 : -1);
        result[i] = result[i]! + change;
        delta -= change;
      }
    });
  }
  return result;
}

export function Resizable({
  id: providedId,
  panes,
  orientation = "horizontal",
  sizes,
  defaultSizes = {},
  onSizesChange,
  ...layout
}: ResizableProps) {
  const id = useEntityId(providedId),
    vertical = orientation === "vertical",
    kit = useGuiKit(),
    gap = kit.unit(12);
  const fields = useLayoutFields(layout);
  const observed = useField(
    id,
    "CanvasBounds",
    vertical ? "local_height" : "local_width",
  );
  const length =
    observed.status === "ready" && typeof observed.value === "number"
      ? observed.value
      : vertical
        ? (fields.height ?? 0)
        : (fields.width ?? 0);
  const state = useApplicationValue(sizes, defaultSizes, onSizesChange);
  const padding = vertical
    ? (fields.padding_top ?? 0) + (fields.padding_bottom ?? 0)
    : (fields.padding_left ?? 0) + (fields.padding_right ?? 0);
  const available = Math.max(
    0,
    length - padding - Math.max(0, panes.length - 1) * gap,
  );
  const actual = fitPanes(panes, state.current, available);
  const previous = useRef(new Map<string, number>());
  const Container = vertical ? Column : Row;
  function drag(index: number, delta: number) {
    const left = panes[index]!,
      right = panes[index + 1]!,
      a = actual[index]!,
      b = actual[index + 1]!;
    const move =
      Math.min(
        Math.max(0, (left.max ?? Infinity) - a),
        Math.max(0, b - (right.min ?? 0)),
        Math.max(0, delta),
      ) -
      Math.min(
        Math.max(0, (right.max ?? Infinity) - b),
        Math.max(0, a - (left.min ?? 0)),
        Math.max(0, -delta),
      );
    if (move === 0) return;
    state.change(() =>
      Object.fromEntries(
        panes.map((pane, i) => [
          pane.key,
          actual[i]! + (i === index ? move : i === index + 1 ? -move : 0),
        ]),
      ),
    );
  }
  return (
    <Container {...layout} id={id} gap={0} measureBounds>
      {panes.map((pane, index) => (
        <Fragment key={pane.key}>
          <Column
            id={`${id}/pane/${pane.key}`}
            measureBounds
            clip
            gap={0}
            {...(vertical
              ? { height: actual[index]!, fitHeight: false }
              : { width: actual[index]! })}
          >
            {pane.content}
          </Column>
          {index < panes.length - 1 && (
            <SplitHandle
              key={panes[index + 1]!.key}
              id={`${id}/handle/${pane.key}`}
              vertical={vertical}
              gap={gap}
              label={pane.key}
              resize={(d) => drag(index, d)}
              toggle={() => {
                if (!pane.collapsible) return;
                const current = actual[index]!;
                if (current > 0) previous.current.set(pane.key, current);
                state.change(() =>
                  Object.fromEntries(
                    panes.map((item, i) => [
                      item.key,
                      i === index
                        ? current > 0
                          ? 0
                          : (previous.current.get(pane.key) ??
                            pane.initialSize ??
                            available / panes.length)
                        : actual[i]!,
                    ]),
                  ),
                );
              }}
            />
          )}
        </Fragment>
      ))}
    </Container>
  );
}

function SplitHandle({
  id,
  vertical,
  gap,
  label,
  resize,
  toggle,
}: {
  id: string;
  vertical: boolean;
  gap: number;
  label: string;
  resize: (delta: number) => void;
  toggle: () => void;
}) {
  const held = useRef<{ pointer: bigint; offset: number } | null>(null);
  const [active, setActive] = useState(false);
  const reset = () => {
    held.current = null;
    setActive(false);
  };
  return (
    <Grip
      id={id}
      label={`Resize ${label}`}
      layout={vertical ? { height: gap } : { width: gap }}
      onBlur={reset}
      onInput={(event) => {
        if (event.kind === "pointer") {
          const at = event.point[vertical ? 1 : 0];
          if (event.phase === "down" && event.button === "primary") {
            held.current = { pointer: event.pointer, offset: at };
            setActive(true);
          } else if (held.current?.pointer === event.pointer) {
            if (event.phase === "move") resize(at - held.current.offset);
            if (event.phase === "up" || event.phase === "cancel") reset();
          }
        } else if (event.kind === "key" && event.pressed) {
          if (event.key === "Enter") {
            toggle();
            return;
          }
          if (event.key === (vertical ? "ArrowDown" : "ArrowRight")) resize(10);
          if (event.key === (vertical ? "ArrowUp" : "ArrowLeft")) resize(-10);
          if (event.key === "Home") resize(-Number.MAX_SAFE_INTEGER);
          if (event.key === "End") resize(Number.MAX_SAFE_INTEGER);
        }
      }}
    >
      <TextLine
        text={vertical ? "─" : "│"}
        tone={active ? "accent" : "neutral"}
      />
    </Grip>
  );
}
