/**
 * Toasts: brief notifications above the content that never take focus or
 * move it. Each toast is a control row in half-inset margins: its severity's
 * mark, one line of text, an optional secondary action and a docked close
 * button, framed as an inline alert of its severity. Success and information
 * take the accent, success with the check mark; warning amber; error the
 * error colour. The body is a Button that does not take focus, so a press on
 * the toast neither reaches what lies beneath nor moves focus; it does
 * nothing else.
 *
 * A toast that is not an error, has no action and is not `persistent`
 * dismisses itself after `duration` milliseconds of the Host clock: an
 * ordinary animation controller holds its opacity, fades it out over 100 ms
 * (at once under the kit's reduced motion) and reports completion, upon which
 * the toast calls `onDismiss`. The controller pauses while a pointer hovers
 * the toast or focus is on its buttons, from the controls' feedback
 * callbacks; its World must select `ipp.animation`. Errors and toasts with an
 * action stay until dismissed.
 *
 * `ToastStack` is a top-level manual overlay pinned inside a canvas edge at
 * the content inset, bottom-right by default, holding the application's
 * toasts in their order, the first `limit` of them when it has one; the
 * application owns the list and removes a toast when `onDismiss` reports it.
 * It sits on the toast plane above dialogs, so its toasts stay usable while
 * a modal dialog blocks what lies beneath it.
 * Declare it at the top level of the World's declarations, outside any
 * `Children`.
 */
import { useCallback, useMemo, useRef, type RefObject } from "react";
import { Animation, type AnimationHandle } from "../animation.js";
import { AnimationAsset, assetRef } from "../assets.js";
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Behavior, Font, Layout, Overlay, Style } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import type {
  GuiFocusChangeEvent,
  GuiInteractionEvent,
  GuiPressListener,
} from "../gui/callbacks.js";
import type { GuiKitIcon } from "./icons.js";
import { useGuiKit, type GuiKitTone } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_ROW,
  Strut,
  type GuiKitLayout,
} from "./layout.js";
import { GUI_KIT_OVERLAY_BANDS } from "./overlay.js";
import { SecondaryButton } from "./secondary-button.js";
import { Separator } from "./separator.js";
import { CheckMark, Icon, TextLine } from "./text.js";
import type { KitThemeName } from "./themes.js";
import { WindowControl } from "./window-controls.js";

export type ToastSeverity = "success" | "information" | "warning" | "error";

/** A secondary button before the close button. */
export interface ToastAction {
  readonly label: string;
  readonly onPress?: GuiPressListener;
}

/** One toast of a `ToastStack`. */
export interface ToastItem {
  /** Stable key, unique in the stack. */
  readonly key: string;
  /** Information by default. */
  readonly severity?: ToastSeverity;
  readonly text: string;
  readonly action?: ToastAction;
  /** Stay until dismissed, as errors and toasts with an action do. */
  readonly persistent?: boolean;
}

/** Milliseconds a toast that dismisses itself stays, by default. */
const DURATION = 6000;

/** Seconds of the fade before a toast dismisses itself. */
const FADE = 0.1;

/** Width of a toast, at the tokens' `em`. */
const WIDTH = 480;

const SEVERITIES: Readonly<
  Record<
    ToastSeverity,
    { tone: GuiKitTone; theme: KitThemeName; icon?: GuiKitIcon }
  >
> = {
  success: { tone: "accent", theme: "toastAccent" },
  information: { tone: "accent", theme: "toastAccent", icon: "information" },
  warning: { tone: "amber", theme: "toastAmber", icon: "warning" },
  error: { tone: "error", theme: "toastError", icon: "error" },
};

export interface ToastProps extends Omit<ToastItem, "key"> {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  /** The close button's press, or the end of the toast's time. */
  readonly onDismiss?: () => void;
  /** Milliseconds before a toast that dismisses itself does; 6000 by default. */
  readonly duration?: number;
  readonly layout?: GuiKitLayout;
}

export function Toast({
  id,
  layer = 0,
  severity = "information",
  text,
  action,
  persistent = false,
  onDismiss,
  duration = DURATION,
  layout,
}: ToastProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const { tone, theme, icon } = SEVERITIES[severity];
  const timed = !persistent && severity !== "error" && !action;
  const gap = kit.unit(t.inset / 2);
  const height = t.controlHeight + t.inset;

  // Hovering pointers and focused buttons, by control; any of them pauses
  // the toast's time.
  const holds = useRef(new Set<string>());
  const timer = useRef<AnimationHandle>(null);
  const hold = useCallback((name: string, held: boolean) => {
    const before = holds.current.size > 0;
    if (held) holds.current.add(name);
    else holds.current.delete(name);
    const after = holds.current.size > 0;
    if (before === after) return;
    void (after ? timer.current?.pause() : timer.current?.play())?.catch(
      () => {},
    );
  }, []);
  const feedback = (control: string) =>
    timed
      ? {
          onInteractionChange: (event: GuiInteractionEvent) =>
            hold(`${control}/${event.pointer}`, event.hovered),
          onFocusChange: (event: GuiFocusChangeEvent) =>
            hold(`${control}/focus`, event.focused),
        }
      : {};

  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_ROW}
        height={kit.unit(height)}
        padding_left={kit.unit(t.inset)}
        padding_right={gap}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme(theme)} />
      <Style layer={layer} opacity={1} />
      <Behavior focusable={false} semantic_label={text} />
      <Button label="" {...feedback("body")} />
      <Children>
        <Strut id={`${id}/strut`} height={kit.unit(height)} />
        {icon ? (
          <Icon id={`${id}/mark`} icon={icon} tone={tone} />
        ) : (
          <CheckMark id={`${id}/mark`} />
        )}
        <TextLine
          id={`${id}/text`}
          text={text}
          layout={{ flex: 1, margin_left: kit.unit(t.inset) }}
        />
        {action && (
          <>
            <SecondaryButton
              id={`${id}/action`}
              {...action}
              {...feedback("action")}
              layout={{ margin_left: gap }}
            />
            <Separator
              id={`${id}/divider`}
              vertical
              layout={{
                height: kit.unit(t.smallHeight),
                align_y: 0,
                margin_left: gap,
              }}
            />
          </>
        )}
        <WindowControl
          id={`${id}/close`}
          kind="close"
          label="Dismiss"
          {...(onDismiss ? { onPress: () => onDismiss() } : {})}
          {...feedback("close")}
          layout={{ margin_left: gap }}
        />
      </Children>
      {timed && (
        <Countdown
          id={id}
          duration={duration / 1000}
          timer={timer}
          {...(onDismiss ? { onEnd: onDismiss } : {})}
        />
      )}
    </Entity>
  );
}

/**
 * The toast's time on the Host clock: a clip that holds the toast's opacity,
 * then fades it out, or under reduced motion drops it at once. Animation adds
 * the clip's change since its first key to the authored opacity of 1.
 * Completion ends the toast.
 */
function Countdown({
  id,
  duration,
  timer,
  onEnd,
}: {
  readonly id: string;
  readonly duration: number;
  readonly timer: RefObject<AnimationHandle | null>;
  readonly onEnd?: () => void;
}) {
  const kit = useGuiKit();
  const { reducedMotion } = kit;
  const style = kit.contract.components.CanvasStyle;
  const clip = useMemo(() => {
    const opacity = (value: number) => ({ kind: "f32" as const, value });
    const keys = reducedMotion
      ? [
          {
            time: 0,
            value: opacity(1),
            interpolation: { kind: "step" as const },
          },
          { time: duration, value: opacity(0) },
        ]
      : [
          {
            time: 0,
            value: opacity(1),
            interpolation: { kind: "linear" as const },
          },
          {
            time: duration,
            value: opacity(1),
            interpolation: { kind: "linear" as const },
          },
          { time: duration + FADE, value: opacity(0) },
        ];
    return {
      duration: keys.at(-1)!.time,
      tracks: [
        {
          property: {
            component: style.id,
            offsets: [style.fields.opacity.offset],
          },
          keys,
        },
      ],
    };
  }, [duration, reducedMotion, style]);
  return (
    <>
      <AnimationAsset id={`${id}/countdown`} clip={clip} />
      <Animation
        ref={timer}
        source={assetRef(`${id}/countdown`)}
        target={id}
        autoPlay
        onPlaybackEvent={(event) => {
          if (event.kind === "completed") onEnd?.();
        }}
      />
    </>
  );
}

export interface ToastStackProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  /** The toasts in the order they stack, the first nearest the stack's top. */
  readonly toasts: readonly ToastItem[];
  /** A toast's close button was pressed or its time ended. */
  readonly onDismiss?: (key: string) => void;
  /** Show at most this many, the first of `toasts`; the rest wait. */
  readonly limit?: number;
  /** The canvas edge the stack rests on; bottom by default. */
  readonly side?: "top" | "bottom";
  /** Where along that edge; the end by default. */
  readonly align?: "start" | "centre" | "end";
  /** Milliseconds before toasts that dismiss themselves do; 6000 by default. */
  readonly duration?: number;
  /** Layout of the stack, such as its `width` in the World's units. */
  readonly layout?: GuiKitLayout;
}

const ALIGNS = { start: 0, centre: 1, end: 2 } as const;

export function ToastStack({
  id,
  layer = 0,
  toasts,
  onDismiss,
  limit,
  side = "bottom",
  align = "end",
  duration,
  layout,
}: ToastStackProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const shown = limit === undefined ? toasts : toasts.slice(0, limit);
  const inset = kit.unit(t.inset);
  return (
    <Entity id={id}>
      <Overlay
        side={side === "top" ? 1 : 0}
        align={ALIGNS[align]}
        band={GUI_KIT_OVERLAY_BANDS.notification}
      />
      <Style
        layer={layer}
        x={align === "start" ? inset : align === "end" ? -inset : 0}
        y={side === "top" ? inset : -inset}
      />
      <Behavior visible={shown.length > 0} />
      <Layout kind={LAYOUT_COLUMN} width={kit.unit(WIDTH)} {...layout} />
      <Children>
        {shown.map(({ key, ...toast }, index) => (
          <Toast
            key={key}
            id={`${id}/${key}`}
            {...toast}
            {...(onDismiss ? { onDismiss: () => onDismiss(key) } : {})}
            {...(duration === undefined ? {} : { duration })}
            {...(index > 0 ? { layout: { margin_top: inset } } : {})}
          />
        ))}
      </Children>
    </Entity>
  );
}
