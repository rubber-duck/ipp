import {
  createElement,
  Fragment,
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  Children,
  Entity,
  componentContract,
  type ComponentProps,
  type ComponentFields,
} from "../components.js";
import type { GuiControlRef } from "./control-ref.js";
import type {
  GuiContextMenuListener,
  GuiFeedbackListeners,
  GuiRangeChangeListener,
  GuiScrollListener,
  GuiVirtualRange,
} from "./callbacks.js";

export type ScrollViewProps = ComponentProps &
  ComponentFields<"GuiScrollView"> &
  GuiFeedbackListeners & {
    ref?: GuiControlRef;
    onScroll?: GuiScrollListener;
    onRangeChange?: GuiRangeChangeListener;
    onContextMenu?: GuiContextMenuListener;
  };

export function ScrollView({ ref, ...props }: ScrollViewProps) {
  return createElement(componentContract.GuiScrollView.host, {
    ...props,
    controlRef: ref,
  });
}

export type VirtualItemProps = ComponentProps &
  ComponentFields<"GuiVirtualItem">;

export function VirtualItem(props: VirtualItemProps) {
  return createElement(componentContract.GuiVirtualItem.host, props);
}

export type VirtualListProps = ComponentProps &
  ComponentFields<"GuiVirtualList"> &
  GuiFeedbackListeners & {
    ref?: GuiControlRef;
    renderItem: (index: number) => ReactNode;
    onRangeChange?: GuiRangeChangeListener;
    onScroll?: GuiScrollListener;
    onContextMenu?: GuiContextMenuListener;
  };

export function VirtualList({
  ref,
  renderItem,
  onRangeChange,
  ...props
}: VirtualListProps) {
  const identity = useId();
  const [range, setRange] = useState<GuiVirtualRange | null>(null);
  const listener = useRef<GuiRangeChangeListener | undefined>(undefined);
  useLayoutEffect(() => {
    listener.current = onRangeChange;
  }, [onRangeChange]);
  // Ranges arrive in tick order, so the latest one is current.
  const receive = useCallback((next: GuiVirtualRange) => {
    setRange(next);
    listener.current?.(next);
  }, []);
  const items = [];
  for (
    let index = range?.first ?? 0;
    index < Math.min(range?.last ?? 0, range?.itemCount ?? 0);
    index++
  ) {
    items.push(
      createElement(
        Entity,
        { key: index, id: `${identity}/item/${index}` },
        createElement(VirtualItem, { index }),
        renderItem(index),
      ),
    );
  }
  return createElement(
    Fragment,
    null,
    createElement(componentContract.GuiVirtualList.host, {
      ...props,
      controlRef: ref,
      onRangeChange: receive,
    }),
    createElement(Children, null, items),
  );
}
