import type { ReactNode } from "react";
import { SpecimenCaption } from "./caption.js";
import {
  GlassPanel,
  GlassProgress,
} from "../../../../packages/ipp-react/src/gui-glass/surfaces.js";
import { Children, Entity } from "@ipp/react";
import { Behavior, Layout, type GuiControlRef } from "@ipp/react/gui";
import {
  GlassCheckbox,
  GlassRangeSlider,
  GlassSlider,
  GlassSwitch,
} from "../../../../packages/ipp-react/src/gui-glass/values.js";
import {
  GLASS_SELECTION_CASES,
  GLASS_SLIDER_BOUNDS,
  GLASS_SLIDER_CASES,
  GLASS_VALUE_RECTS,
} from "../fixtures/values.js";

/** The enclosing driver supplies GlassSkin, font, page and native references. */
export function GlassValuesSpecimen({
  control,
}: {
  readonly control: (id: string) => GuiControlRef;
}) {
  return (
    <>
      {GLASS_SELECTION_CASES.map(({ id, kind, checked, disabled }) => {
        const Toggle = kind === "checkbox" ? GlassCheckbox : GlassSwitch;
        return (
          <ValueAt key={id} id={id}>
            <Behavior enabled={!disabled} />
            <Toggle checked={checked} label="" ref={control(id)} />
          </ValueAt>
        );
      })}
      {GLASS_SLIDER_CASES.map(({ id, axis, value, upper, range, disabled }) => {
        const Scalar = range ? GlassRangeSlider : GlassSlider;
        return (
          <ValueAt key={id} id={id}>
            <Behavior enabled={!disabled} />
            <Scalar
              {...GLASS_SLIDER_BOUNDS}
              axis={axis}
              value={value}
              upper={upper}
              ref={control(id)}
            />
          </ValueAt>
        );
      })}
      {Object.entries(GLASS_VALUE_RECTS).map(([id, [x, y, width, height]]) => (
        <SpecimenCaption
          key={`caption-${id}`}
          id={id}
          text={id}
          x={width < height ? x - 20 : width === 40 ? x - 8 : x}
          y={y + height + 6}
          width={width < height ? 80 : Math.max(width, 84)}
        />
      ))}
      {[0, 0.5, 1].map((value, index) => (
        <Entity key={index} id={`progress-slot-${index}`}>
          <Layout
            kind={3}
            width={240}
            height={12}
            margin_left={32}
            margin_top={308 + index * 32}
            align_x={-1}
            align_y={-1}
          />
          <Children>
            <GlassProgress
              id={`progress-${index}`}
              value={value}
              width={240}
              height={12}
            />
          </Children>
        </Entity>
      ))}
      <Entity id="glass-panel">
        <Layout
          kind={0}
          width={240}
          height={76}
          margin_left={320}
          margin_top={308}
          align_x={-1}
          align_y={-1}
        />
        <GlassPanel />
      </Entity>
      <SpecimenCaption
        id="progress"
        text="progress 0 / 50 / 100%"
        x={32}
        y={394}
        width={240}
      />
      <SpecimenCaption id="panel" text="panel" x={320} y={394} width={240} />
    </>
  );
}

function ValueAt({
  id,
  children,
}: {
  readonly id: string;
  readonly children: ReactNode;
}) {
  const [x, y, width, height] = GLASS_VALUE_RECTS[id]!;
  return (
    <Entity id={id}>
      <Layout
        kind={0}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      {children}
    </Entity>
  );
}
