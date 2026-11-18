import { useRef, useState, type ReactNode } from "react";
import { useField } from "../../scope/field.js";
import { useEntityId } from "../../scope/identity.js";
import { useGuiKit } from "../kit.js";
import { TextLine } from "../text.js";
import { Button } from "../button.js";
import { Grip } from "../containers/grip.js";
import { FieldMotion } from "../values/field-motion.js";
import { useApplicationValue } from "../selection/application-value.js";
import { Floating, useOverlayOpen, type OverlayOpenProps } from "./overlay.js";

export interface DrawerProps extends OverlayOpenProps {
  readonly id?: string;
  readonly label: string;
  readonly edge?: "top" | "bottom" | "left" | "right";
  readonly mode?: "light" | "modal";
  readonly snapPoints: readonly number[];
  readonly snap?: number;
  readonly defaultSnap?: number;
  readonly onSnapChange?: (index: number) => void;
  readonly children?: ReactNode;
}

/** Declare as a top-level canvas overlay. Its grip owns drags, leaving scrolling in the body to its own controls. Snap lengths are local layout units. */
export function Drawer({
  id: providedId,
  label,
  edge = "bottom",
  mode = "modal",
  snapPoints,
  snap,
  defaultSnap = 0,
  onSnapChange,
  children,
  ...openProps
}: DrawerProps) {
  if (
    !snapPoints.length ||
    snapPoints.some(
      (v, i) =>
        !Number.isFinite(v) || v <= 0 || (i > 0 && v <= snapPoints[i - 1]!),
    )
  )
    throw new RangeError(
      "Drawer snap points must be positive and strictly increasing",
    );
  const id = useEntityId(providedId),
    kit = useGuiKit(),
    visibility = useOverlayOpen(openProps);
  const choice = useApplicationValue(snap, defaultSnap, onSnapChange);
  if (!Number.isInteger(choice.current))
    throw new RangeError("Drawer snap index must be an integer");
  const index = Math.max(0, Math.min(snapPoints.length - 1, choice.current)),
    size = snapPoints[index]!,
    max = snapPoints.at(-1)!;
  const vertical = edge === "top" || edge === "bottom",
    sign = edge === "bottom" || edge === "right" ? -1 : 1;
  const [drag, setDrag] = useState<number | null>(null);
  const gripOrigin = useField(
    id + "/grip",
    "CanvasBounds",
    vertical ? "y" : "x",
  );
  const gripScale = useField(
    id + "/grip",
    "CanvasBounds",
    vertical ? "scale_y" : "scale_x",
  );
  const gripExtent = useField(
    id + "/grip",
    "CanvasBounds",
    vertical ? "height" : "width",
  );
  const axis = vertical ? "height" : "width";
  const measured = useField(id, "GuiLayout", axis);
  const held = useRef<{
    pointer: bigint;
    offset: number;
    start: number;
    edge: string;
  } | null>(null);
  const reset = () => {
    held.current = null;
    setDrag(null);
  };
  const actual = Math.max(0, Math.min(max, drag ?? size));
  return (
    <Floating
      id={id}
      align="stretch"
      side={edge}
      mode={mode}
      band={mode === "modal" ? "dialog" : "popup"}
      open={visibility.open}
      controlled={visibility.controlled}
      onVisibleChange={visibility.onVisibleChange}
      layout={{ [axis]: 0, clip: true }}
    >
      <Grip
        id={`${id}/grip`}
        label={`Resize ${label}`}
        layout={{ height: kit.unit(kit.tokens.smallHeight) }}
        onBlur={reset}
        onInput={(e) => {
          if (e.kind === "key" && e.pressed) {
            const dir =
              e.key === (vertical ? "ArrowDown" : "ArrowRight")
                ? sign
                : e.key === (vertical ? "ArrowUp" : "ArrowLeft")
                  ? -sign
                  : 0;
            if (dir)
              choice.change(() =>
                Math.max(0, Math.min(snapPoints.length - 1, index + dir)),
              );
          } else if (e.kind === "pointer") {
            const scale = gripScale.value,
              origin = gripOrigin.value,
              extent = gripExtent.value;
            if (
              typeof scale !== "number" ||
              scale === 0 ||
              typeof origin !== "number" ||
              typeof extent !== "number"
            ) {
              reset();
              return;
            }
            // Hit points start at the signed local origin; CanvasBounds starts at the minimum.
            const at =
              origin +
              (scale < 0 ? extent : 0) +
              e.point[vertical ? 1 : 0] * scale;
            if (e.phase === "down" && e.button === "primary")
              held.current = {
                pointer: e.pointer,
                offset: at,
                start:
                  typeof measured.value === "number" && measured.value > 0
                    ? measured.value
                    : size,
                edge,
              };
            const lease = held.current;
            if (!lease || lease.pointer !== e.pointer) return;
            if (e.phase === "cancel" || lease.edge !== edge) {
              reset();
              return;
            }
            const next = Math.max(
              0,
              Math.min(
                max,
                lease.start + (sign * (at - lease.offset)) / Math.abs(scale),
              ),
            );
            if (e.phase === "move") setDrag(next);
            if (e.phase === "up") {
              reset();
              if (next < snapPoints[0]! / 2) {
                visibility.setOpen(false);
                return;
              }
              const nearest = snapPoints.reduce(
                (best, v, i) =>
                  Math.abs(v - next) < Math.abs(snapPoints[best]! - next)
                    ? i
                    : best,
                0,
              );
              choice.change(() => nearest);
            }
          }
        }}
      >
        <TextLine text={label} />
      </Grip>
      {children}
      <Button
        id={`${id}/close`}
        label="Close"
        onPress={() => visibility.setOpen(false)}
      />
      {visibility.open && (
        <FieldMotion
          target={id}
          component="GuiLayout"
          field={axis}
          from={0}
          to={max}
          fraction={actual / max}
          immediate={drag !== null}
        />
      )}
    </Floating>
  );
}
