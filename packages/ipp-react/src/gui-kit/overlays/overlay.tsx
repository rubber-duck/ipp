import { useEntityId } from "../../scope/identity.js";
/**
 * What the kit's overlays share: semantic priority bands, a client
 * open state kept in step with the runtime, and the floating surface that
 * menus, option lists, popovers, tooltips and dialogs are drawn on.
 *
 * An overlay is open while its `Behavior.visible` holds. Clients write it to
 * open and close the overlay; the runtime writes it too, as the overlay's
 * mode decides: it closes a light overlay on an outside press or when focus
 * leaves, and a light or modal one on Escape, and it opens and closes a hint.
 * `useOverlayOpen` keeps a client's open state in step with those writes
 * through `onVisibleChange`, so the next opening is a change the client
 * declares again.
 */
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { PresentationGate, Children, Entity } from "../../components.js";
import { Font, Layout, Overlay, Style } from "../../gui/components.js";
import { useComposedControlRef } from "../../gui/control-refs.js";
import type { GuiControlHandle } from "../../gui/control-ref.js";
import { useValueSynchronization } from "../values/value-synchronization.js";
import { Skin } from "../../gui/theme.js";
import { BehaviorWithRef } from "../../gui/behavior-ref.js";
import type {
  GuiControlEvent,
  GuiVisibleChangeListener,
} from "../../gui/callbacks.js";
import { useGuiKit } from "../kit.js";
import { LAYOUT_COLUMN, type GuiKitLayout } from "../layout.js";

/** Semantic overlay priorities, independent of interaction mode and content offsets. */
export const GUI_KIT_OVERLAY_BANDS = {
  popup: 1,
  dialog: 2,
  notification: 3,
} as const;

/** An overlay's semantic ordering band; nested scopes inherit at least their owner's. */
export type GuiKitOverlayBand = keyof typeof GUI_KIT_OVERLAY_BANDS;

/** Where an overlay goes against its parent's box, or the canvas at the top level. */
export type GuiKitOverlaySide = "bottom" | "top" | "right" | "left" | "centre";

/** How it aligns along that side. */
export type GuiKitOverlayAlign = "start" | "centre" | "end" | "stretch";

/**
 * What besides the client opens and closes it: nothing (manual), an outside
 * press or focus leaving (light), Escape only while it blocks what lies
 * beneath it (modal), or hover and visible focus on its parent (hint).
 */
export type GuiKitOverlayMode = "manual" | "light" | "modal" | "hint";

/** `GuiOverlay` field values. */
export const OVERLAY_SIDE = {
  bottom: 0,
  top: 1,
  right: 2,
  left: 3,
  centre: 4,
} as const satisfies Record<GuiKitOverlaySide, number>;
export const OVERLAY_ALIGN = {
  start: 0,
  centre: 1,
  end: 2,
  stretch: 3,
} as const satisfies Record<GuiKitOverlayAlign, number>;
export const OVERLAY_MODE = {
  manual: 0,
  light: 1,
  modal: 2,
  hint: 3,
} as const satisfies Record<GuiKitOverlayMode, number>;

export interface OverlayOpenProps {
  /** Whether the overlay is open, controlled; pair it with `onOpenChange`. */
  readonly open?: boolean;
  /** Whether it starts open while `open` is omitted. */
  readonly defaultOpen?: boolean;
  /** It opened or closed: by the client, or the runtime closing it. */
  readonly onOpenChange?: (open: boolean) => void;
}

/** A light or modal overlay's open state as a client keeps it. */
export interface OverlayOpen {
  /** Whether open intent belongs to the application rather than the adapter. */
  readonly controlled: boolean;
  /**
   * Whether it is open as far as this client knows, ahead of its renders, so
   * a second press dispatched before the first one's render sees it closed.
   */
  readonly open: boolean;
  setOpen(open: boolean): void;
  toggle(): void;
  /** The overlay `Behavior`'s callback: adopts the runtime closing it. */
  readonly onVisibleChange: GuiVisibleChangeListener;
}

/**
 * The open state of a light or modal overlay. Declare the overlay's
 * `Behavior` with `visible={open}` and this `onVisibleChange`.
 *
 * The runtime only ever closes these overlays, so only its `false` writes are
 * adopted, and only from the entity last reported open since the client
 * last opened it: a report of an earlier state that arrives after the
 * client's own opening, such as the field's first report, does not close it
 * again, and neither does another overlay's report, such as one nested in
 * it. A hint is opened and closed by the runtime alone; declare it closed and
 * do not follow it.
 */
export function useOverlayOpen({
  open,
  defaultOpen = false,
  onOpenChange,
}: OverlayOpenProps = {}): OverlayOpen {
  const [own, setOwn] = useState(defaultOpen);
  const current = open ?? own;
  // The state as this client last set or rendered it.
  const latest = useRef(current);
  // The entity the runtime last reported open since the client opened the
  // overlay, so that its closing can be told from an earlier report and from
  // another entity's.
  const shown = useRef<bigint | undefined>(undefined);
  // A change the client did not set itself, such as a controlled `open`,
  // starts over too.
  const rendered = useRef(current);
  useLayoutEffect(() => {
    if (rendered.current !== current) {
      rendered.current = current;
      latest.current = current;
      shown.current = undefined;
      setOwn(current);
    }
  }, [current]);

  const set = (next: boolean) => {
    if (next === latest.current) return;
    latest.current = next;
    shown.current = undefined;
    setOwn(next);
    onOpenChange?.(next);
  };
  useEffect(() => {
    if (open !== undefined && latest.current !== open) {
      latest.current = open;
      shown.current = undefined;
      setOwn(open);
    }
  }, [open, own]);
  return {
    controlled: open !== undefined,
    get open() {
      return latest.current;
    },
    setOpen: set,
    toggle: () => set(!latest.current),
    onVisibleChange: (event: GuiControlEvent<boolean>) => {
      const { entity } = event.target;
      if (event.value) shown.current = latest.current ? entity : undefined;
      else if (
        shown.current === entity ||
        (open !== undefined &&
          shown.current === undefined &&
          latest.current &&
          !event.initial)
      )
        set(false);
    },
  };
}

export interface FloatingProps {
  /** Share a touching anchor's background paint coordinates without raising its surface. */
  readonly joinAnchor?: boolean;
  /** Restore application open intent after native closes; adapters pass their ownership. */
  readonly controlled?: boolean;
  /** Symbolic id of the overlay entity. */
  readonly id?: string;
  /** Below its parent's box by default. */
  readonly side?: GuiKitOverlaySide;
  /** At the start of that side by default. */
  readonly align?: GuiKitOverlayAlign;
  readonly mode: GuiKitOverlayMode;
  /** Popup by default, independently of interaction mode. */
  readonly band?: GuiKitOverlayBand;
  /** Nonnegative relative component offset inside this overlay scope; zero inherits. */
  readonly layer?: number;
  /** Its `Behavior.visible` as declared. */
  readonly open: boolean;
  readonly onVisibleChange?: GuiVisibleChangeListener;
  /** Its translation from where it is placed, in the World's units. */
  readonly offset?: readonly [number, number];
  /** The part cut, for a small surface such as a tooltip. */
  readonly small?: boolean;
  /** Layout of its column, such as its `width`; content fixes its height. */
  readonly layout?: GuiKitLayout;
  readonly children?: ReactNode;
}

/**
 * A floating surface: an overlay entity in the floating frame, a control
 * frame at rest whose content is a column inside its line, so separators
 * meet the frame. It declares the kit's font and size, since a top-level
 * overlay inherits none. Declare an anchored one inside its parent's
 * `Children` and a top-level one outside any `Children`.
 */
export function Floating({
  id: providedId,
  side = "bottom",
  align = "start",
  mode,
  band = "popup",
  layer = 0,
  open,
  onVisibleChange,
  offset,
  small = false,
  joinAnchor = false,
  layout,
  children,
  controlled = false,
}: FloatingProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const line = kit.unit(kit.tokens.lineWidth);
  const handle = useRef<GuiControlHandle | null>(null);
  type Visibility = {
    readonly visible: boolean;
    readonly event?: GuiControlEvent<boolean>;
  };
  const synchronization = useValueSynchronization<Visibility>({
    value: controlled ? { visible: open } : undefined,
    defaultValue: { visible: open },
    same: (left, right) => left.visible === right.visible,
    onChange: (next) => {
      if (next.event) onVisibleChange?.(next.event);
    },
    identity: () => handle.current,
    write: async (from, to, context) => {
      const target = handle.current;
      if (!target) return;
      await context.trackEcho({ visible: to.visible }, () =>
        target.compareAndSet("visible", from.visible, to.visible),
      );
    },
  });
  const attach = useComposedControlRef(
    handle,
    undefined,
    synchronization.notifyReady,
  );
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Overlay
        side={OVERLAY_SIDE[side]}
        align={OVERLAY_ALIGN[align]}
        mode={OVERLAY_MODE[mode]}
        band={GUI_KIT_OVERLAY_BANDS[band]}
        join_anchor={joinAnchor}
      />
      <Style
        layer={layer}
        {...(offset ? { x: offset[0], y: offset[1] } : {})}
      />
      <BehaviorWithRef
        visible={controlled ? synchronization.initial.visible : open}
        controlRef={attach}
        onVisibleChange={(event) => {
          if (controlled) {
            // The seed establishes a target without public notification.
            // All later own-write echoes remain suppressed.
            if (event.initial) onVisibleChange?.(event);
            synchronization.observe({ visible: event.value, event }, event);
          } else onVisibleChange?.(event);
        }}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Layout
        kind={LAYOUT_COLUMN}
        padding_left={line}
        padding_right={line}
        padding_top={line}
        padding_bottom={line}
        {...layout}
      />
      <Skin theme={kit.theme(small ? "floatingSmall" : "floating")} />
      {children !== undefined && <Children>{children}</Children>}
    </Entity>
  );
}
