/**
 * A context menu: the commands for one target, opened at the point of the
 * target's context request, a secondary press or the Menu key or Shift+F10
 * on the focused target. The request has focused the target, and the menu's
 * rows take no focus, so focus stays on the target while hover and the
 * arrows move the active row; Enter or a press runs its command once and
 * closes the menu. Escape, an outside press, which is swallowed, or focus
 * leaving the target closes it without running anything, and focus never
 * left the target. The menu opens below the point and flips above it, or
 * shifts, to stay inside the canvas.
 *
 * `useContextMenu` holds the request: `opener(target)` is the target's
 * `onContextMenu`, and the application declares the commands for the
 * request's target. The menu is a root of the canvas, placed at a canvas
 * point: declare it outside any `Children`. Declared beside its target, inside
 * the target's own declaration, it unmounts with the target and closes; the
 * runtime also closes it when the focused target goes away, since focus then
 * leaves it. A command reports the target its request named.
 */
import { useEffect, useRef, useState } from "react";
import { Children, Entity } from "../components.js";
import { Style } from "../gui/components.js";
import type {
  GuiContextMenuEvent,
  GuiContextMenuListener,
} from "../gui/callbacks.js";
import type { GuiKitLayout } from "./layout.js";
import { Menu, type MenuItem } from "./menu.js";
import { Floating, useOverlayOpen, type OverlayOpen } from "./overlay.js";

/** One open request: its target and the canvas point it opens at. */
export interface ContextMenuRequest<Target> {
  readonly target: Target;
  readonly point: readonly [number, number];
}

/** A context menu's request and open state, from `useContextMenu`. */
export interface ContextMenuState<Target> {
  /** The open request, or undefined while the menu is closed. */
  readonly request: ContextMenuRequest<Target> | undefined;
  /** The `onContextMenu` of a target: opens the menu for `target`. */
  opener(target: Target): GuiContextMenuListener;
  /** Close the menu, or with `request`, only while it shows that request. */
  close(request?: ContextMenuRequest<Target>): void;
  /** The menu overlay's open state. */
  readonly overlay: OverlayOpen;
}

/** The request of a context menu for targets of type `Target`. */
export function useContextMenu<Target>(): ContextMenuState<Target> {
  const [request, setRequest] = useState<ContextMenuRequest<Target>>();
  // The request as last set or rendered, ahead of renders.
  const current = useRef(request);
  current.current = request;
  const overlay = useOverlayOpen({
    open: request !== undefined,
    onOpenChange: (open) => {
      if (open) return;
      current.current = undefined;
      setRequest(undefined);
    },
  });
  return {
    request,
    opener: (target) => (event: GuiContextMenuEvent) => {
      const next = { target, point: event.point };
      current.current = next;
      setRequest(next);
      overlay.setOpen(true);
    },
    close: (only) => {
      if (only === undefined || current.current === only)
        overlay.setOpen(false);
    },
    overlay,
  };
}

export interface ContextMenuProps<Target> {
  /** Symbolic id of the menu's anchor; its surface and rows extend it. */
  readonly id: string;
  readonly menu: ContextMenuState<Target>;
  /** The commands for the request's target. */
  readonly items: readonly MenuItem[];
  /** A command was activated for `target`; the menu has closed. */
  readonly onSelect?: (key: string, target: Target) => void;
  /** Layout of the command list, such as its `width`. */
  readonly layout?: GuiKitLayout;
}

export function ContextMenu<Target>({
  id,
  menu,
  items,
  onSelect,
  layout,
}: ContextMenuProps<Target>) {
  const { request, overlay } = menu;
  // The anchor stays where it was while the menu closes.
  const point = useRef(request?.point ?? ([0, 0] as const));
  if (request) point.current = request.point;
  // Unmounting while it shows a request, as with its target, closes that
  // request, but not one another target's menu has opened since.
  const shown = useRef(request);
  if (request) shown.current = request;
  const close = useRef(menu.close);
  close.current = menu.close;
  useEffect(
    () => () => {
      if (shown.current) close.current(shown.current);
    },
    [],
  );
  return (
    <Entity id={id}>
      <Style x={point.current[0]} y={point.current[1]} />
      <Children>
        <Floating
          id={`${id}/surface`}
          mode="light"
          open={overlay.open}
          onVisibleChange={overlay.onVisibleChange}
        >
          {request && (
            <Menu
              id={`${id}/items`}
              items={items}
              overlay={overlay}
              onSelect={(key) => onSelect?.(key, request.target)}
              {...(layout ? { layout } : {})}
            />
          )}
        </Floating>
      </Children>
    </Entity>
  );
}
