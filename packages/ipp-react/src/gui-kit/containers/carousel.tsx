import { useRef, useState, type ReactNode } from "react";
import { Children, Entity, PresentationGate } from "../../components.js";
import { Behavior, Layout, Style } from "../../gui/components.js";
import { useEntityId } from "../../scope/identity.js";
import { useGuiKit } from "../kit.js";
import { Column, Row, type ContainerProps } from "../layout.js";
import { useApplicationValue } from "../selection/application-value.js";
import { Button } from "../button.js";
import { TextLine } from "../text.js";
import { FieldMotion } from "../values/field-motion.js";
import { Grip } from "./grip.js";

export interface CarouselSlide {
  readonly key: string;
  readonly label: string;
  readonly content: ReactNode;
}
export interface CarouselProps
  extends Omit<ContainerProps, "children" | "width" | "height"> {
  readonly width: number;
  readonly height: number;
  readonly slides: readonly CarouselSlide[];
  readonly value?: string;
  readonly defaultValue?: string;
  readonly onChange?: (key: string) => void;
}

/** Interactive content stays reachable; the separate grip owns swipe capture. */
export function Carousel({
  id: providedId,
  width,
  height,
  slides,
  value,
  defaultValue,
  onChange,
  ...layout
}: CarouselProps) {
  if (
    ![width, height].every((n) => Number.isFinite(n) && n > 0) ||
    new Set(slides.map((s) => s.key)).size !== slides.length
  )
    throw new RangeError("Invalid carousel dimensions or slide keys");
  const id = useEntityId(providedId),
    kit = useGuiKit(),
    choice = useApplicationValue(
      value,
      defaultValue ?? slides[0]?.key ?? "",
      onChange,
    );
  const index = Math.max(
      0,
      slides.findIndex((s) => s.key === choice.current),
    ),
    last = slides.length - 1,
    active = slides[index];
  const [drag, setDrag] = useState<{
    pointer: bigint;
    start: number;
    key: string;
  } | null>(null);
  const live = useRef(drag),
    [offset, setOffset] = useState(0);
  const clear = () => {
    live.current = null;
    setDrag(null);
    setOffset(0);
  };
  function move(n: number) {
    const slide = slides[Math.max(0, Math.min(last, n))];
    if (slide && slide.key !== active?.key) choice.change(() => slide.key);
  }
  const fraction =
    last > 0
      ? Math.max(
          0,
          Math.min(
            1,
            (index - (drag?.key === active?.key ? offset : 0) / width) / last,
          ),
        )
      : 0;
  return (
    <Column {...layout} id={id} width={width}>
      <Entity id={`${id}/viewport`}>
        <PresentationGate open />
        <Layout kind={3} width={width} height={height} clip />
        <Children>
          <Entity id={`${id}/strip`}>
            <Layout kind={3} width={width} height={height} align_x={-1} />
            <Style {...(last <= 0 ? { x: 0 } : {})} />
            <Children>
              {slides.map((slide, slot) => (
                <Entity key={slide.key} id={`${id}/slide/${slide.key}`}>
                  <Layout kind={2} width={width} height={height} align_x={-1} />
                  <Style x={slot * width} />
                  <Behavior
                    enabled={slide.key === active?.key}
                    semantic_label={slide.label}
                  />
                  <Children>{slide.content}</Children>
                </Entity>
              ))}
            </Children>
          </Entity>
        </Children>
      </Entity>
      {last > 0 && (
        <FieldMotion
          target={`${id}/strip`}
          component="CanvasStyle"
          field="x"
          from={0}
          to={-width * last}
          fraction={fraction}
          immediate={!!drag}
        />
      )}
      <Grip
        id={`${id}/grip`}
        label="Change slide"
        disabled={last < 1}
        layout={{ width, height: kit.unit(kit.tokens.smallHeight) }}
        onBlur={clear}
        onInput={(input) => {
          if (input.kind === "key" && input.pressed) {
            if (input.key === "ArrowRight") move(index + 1);
            if (input.key === "ArrowLeft") move(index - 1);
            if (input.key === "Home") move(0);
            if (input.key === "End") move(last);
          }
          if (input.kind !== "pointer") return;
          if (
            input.phase === "down" &&
            input.button === "primary" &&
            active &&
            !live.current
          ) {
            live.current = {
              pointer: input.pointer,
              start: input.point[0],
              key: active.key,
            };
            setDrag(live.current);
          }
          const held = live.current;
          if (!held || held.pointer !== input.pointer) return;
          if (input.phase === "cancel" || held.key !== active?.key) {
            clear();
            return;
          }
          const delta = input.point[0] - held.start;
          if (input.phase === "move") setOffset(delta);
          if (input.phase === "up") {
            clear();
            if (Math.abs(delta) >= Math.min(width / 4, 48))
              move(index - Math.sign(delta));
          }
        }}
      >
        <TextLine text={active?.label ?? "No slides"} tone="accent" />
      </Grip>
      <Row>
        <Button
          id={`${id}/previous`}
          label="Previous"
          disabled={index === 0}
          onPress={() => move(index - 1)}
        />
        <Button
          id={`${id}/next`}
          label="Next"
          disabled={index >= last}
          onPress={() => move(index + 1)}
        />
      </Row>
    </Column>
  );
}
