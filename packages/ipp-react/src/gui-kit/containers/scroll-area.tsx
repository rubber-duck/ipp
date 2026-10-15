import { useState, type ReactNode } from "react";
import { PresentationGate, Children, Entity } from "../../components.js";
import { Font, Layout, Style } from "../../gui/components.js";
import { ScrollView } from "../../gui/scroll.js";
import { Skin } from "../../gui/theme.js";
import type { GuiControlRef } from "../../gui/control-ref.js";
import type {
  GuiRangeChangeListener,
  GuiScrollListener,
} from "../../gui/callbacks.js";
import { useGuiKit } from "../kit.js";
import { useEntityId } from "../../scope/identity.js";
import {
  Column,
  LAYOUT_COLUMN,
  layoutFields,
  type GuiKitFlowLayoutProps,
} from "../layout.js";
import { usePanelSurface, usePanelPadding } from "./panel.js";

export interface ScrollAreaProps extends GuiKitFlowLayoutProps {
  readonly id?: string;
  readonly layer?: number;
  readonly children?: ReactNode;
  readonly ref?: GuiControlRef;
  /** Standalone areas have a frame; areas inside a Panel reuse its surface. */
  readonly framed?: boolean;
  readonly onScroll?: GuiScrollListener;
  readonly onRangeChange?: GuiRangeChangeListener;
}

/**
 * A vertical content viewport that fits short forms and scrolls when its parent
 * or maxHeight limits it. Content has skin-scaled padding and gaps by default;
 * inside a Panel it shares the Panel's frame and padding. Set padding={0} or
 * gap={0} for joined content, and height or fitHeight={false} for a fixed fill.
 * The body is `${id}/body`; the root ref addresses the ordinary ScrollView.
 */
export function ScrollArea({
  id: providedId,
  layer = 0,
  children,
  ref,
  framed,
  padding,
  gap,
  onScroll,
  onRangeChange,
  ...layout
}: ScrollAreaProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const panel = usePanelSurface();
  const frame = framed ?? !panel;
  const parentPadding = usePanelPadding();
  const insets = parentPadding.map(
    (provided) =>
      padding ??
      Math.max(0, kit.spacing.inset - (panel && !frame ? provided : 0)),
  );
  const theme = frame ? kit.controlTheme("scroll") : kit.theme("gridBody");
  const bar = kit.unit(kit.tokens.bar);
  const [scrolling, setScrolling] = useState(false);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        {...layoutFields(layout, { fit_height: true, clip: true })}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      {theme && <Skin theme={theme} />}
      <ScrollView
        {...(ref === undefined ? {} : { ref })}
        axis={1}
        bar_thickness={scrolling ? bar : 0}
        bar_inset={bar}
        {...(onScroll === undefined ? {} : { onScroll })}
        onRangeChange={(event) => {
          setScrolling(event.capacity[1] > 0);
          onRangeChange?.(event);
        }}
      />
      <Children>
        <Column
          id={`${id}/body`}
          gap={gap ?? kit.spacing.field}
          layout={{
            padding_top: insets[0]!,
            padding_right: insets[1]! + (scrolling ? 3 * bar : 0),
            padding_bottom: insets[2]!,
            padding_left: insets[3]!,
          }}
        >
          {children}
        </Column>
      </Children>
    </Entity>
  );
}
