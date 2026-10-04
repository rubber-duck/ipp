/**
 * Layout pieces of the kit. A row centres its children (`align_y` 0) on the
 * tallest of them rather than on the row's own height, so a kit row whose
 * children should centre on the row declares a strut: a zero-width leaf as
 * tall as the row's content box.
 */
import type { ReactNode } from "react";
import { Children, Entity, type ComponentFields } from "../components.js";
import { Style, Layout } from "../gui/components.js";
import { useGuiKit } from "./kit.js";

/**
 * Layout fields a kit component's root takes over its own, in the World's
 * units: margins, flex, alignment or an explicit width to place it.
 */
export type GuiKitLayout = ComponentFields<"GuiLayout">;

/** GuiLayout operations. */
export const LAYOUT_LEAF = 0;
export const LAYOUT_ROW = 1;
export const LAYOUT_COLUMN = 2;
export const LAYOUT_STACK = 3;
export const LAYOUT_PADDING = 4;

/** A zero-width leaf `height` tall, in the World's units, that its row centres on. */
export function Strut({
  id,
  height,
}: {
  readonly id: string;
  readonly height: number;
}) {
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_LEAF} width={0} height={height} />
    </Entity>
  );
}

export interface RowProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  /** Height at the tokens' `em`; a row of body text by default. */
  readonly height?: number;
  readonly layout?: GuiKitLayout;
  readonly children?: ReactNode;
}

/**
 * A fixed-height row filling its container's width whose children centre
 * vertically on it, such as a row of a list or a line of label and value.
 */
export function Row({ id, layer = 0, height, layout, children }: RowProps) {
  const kit = useGuiKit();
  const extent = layout?.height ?? kit.unit(height ?? kit.tokens.row);
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout kind={LAYOUT_ROW} {...layout} height={extent} />
      <Children>
        <Strut
          id={`${id}/strut`}
          height={
            extent - (layout?.padding_top ?? 0) - (layout?.padding_bottom ?? 0)
          }
        />
        {children}
      </Children>
    </Entity>
  );
}
