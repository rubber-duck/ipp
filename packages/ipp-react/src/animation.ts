import {
  createElement,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  type Ref,
} from "react";
import type {
  AnimationDriverDescription,
  AnimationDriverTarget,
  AnimationTransitionEasing,
  AnimationTransitionStartTime,
  AnimationPlaybackControl,
  AnimationPlaybackEvent,
} from "@ipp/client";
import type { AssetReference } from "./assets.js";

export const ANIMATION_HOST_TYPE = "ipp-animation";
export interface AnimationHandle {
  play(): Promise<void>;
  playAtSpeed(speed: number): Promise<void>;
  pause(): Promise<void>;
  stop(): Promise<void>;
  restart(): Promise<void>;
  seek(time: number): Promise<void>;
}
export interface AnimationBinding
  extends Omit<AnimationDriverDescription, "source" | "target" | "property"> {
  source?: string | AssetReference;
  /** Scene Entity id/bindTo, an existing runtime handle, or the enclosing Entity. */
  target?: string | bigint;
  property?: AnimationDriverTarget;
}
export interface AnimationProps {
  ref?: Ref<AnimationHandle>;
  source?: string | AssetReference;
  target?: string | bigint;
  /** Omit to bind every hinted track of an AnimationAsset to target. */
  bindings?: readonly AnimationBinding[];
  speed?: number;
  looping?: boolean;
  /** Blend changed numeric, quaternion, and pose bindings; discrete or structural bindings reject. */
  transition?: {
    duration: number;
    easing?: AnimationTransitionEasing;
    startTime?: AnimationTransitionStartTime;
  };
  autoPlay?: boolean;
  onPlaybackEvent?: (event: AnimationPlaybackEvent) => void;
}

/** Local command mailbox. No transport work occurs while React renders. */
export class AnimationMailbox {
  mounted = false;
  pending: AnimationPlaybackControl[] = [];
  dispatch?: (control: AnimationPlaybackControl) => Promise<void>;
  control(control: AnimationPlaybackControl): Promise<void> {
    if (!this.mounted)
      return Promise.reject(new Error("Animation is unmounted"));
    if (
      control.action === "seek" &&
      (!Number.isFinite(control.time) || control.time < 0)
    )
      return Promise.reject(
        new RangeError("Animation time must be finite and nonnegative"),
      );
    if (control.action === "playAtSpeed" && !Number.isFinite(control.speed))
      return Promise.reject(new RangeError("Animation speed must be finite"));
    if (this.dispatch) return this.dispatch(control);
    this.pending.push(control);
    return Promise.resolve();
  }
}

/** React owns binding lifetime; the Host owns playback and time. */
export function Animation({ ref, ...props }: AnimationProps) {
  const mailbox = useRef<AnimationMailbox | null>(null);
  if (!mailbox.current) mailbox.current = new AnimationMailbox();
  const commands = mailbox.current;
  useLayoutEffect(() => {
    commands.mounted = true;
    return () => {
      commands.mounted = false;
    };
  }, [commands]);
  useImperativeHandle(
    ref,
    () => ({
      play: () => commands.control({ action: "play" }),
      playAtSpeed: (speed) =>
        commands.control({ action: "playAtSpeed", speed }),
      pause: () => commands.control({ action: "pause" }),
      stop: () => commands.control({ action: "stop" }),
      restart: () => commands.control({ action: "restart" }),
      seek: (time) => commands.control({ action: "seek", time }),
    }),
    [commands],
  );
  return createElement(ANIMATION_HOST_TYPE, { ...props, mailbox: commands });
}

export interface AnimationDescription {
  identity: number;
  mailbox: AnimationMailbox;
  bindings: readonly (Omit<
    AnimationBinding,
    "target" | "source" | "property"
  > & {
    source: string | AssetReference;
    target: { entity: number } | bigint;
    property: AnimationDriverTarget;
  })[];
  speed: number;
  looping: boolean;
  transition?: {
    duration: number;
    easing: AnimationTransitionEasing;
    startTime: AnimationTransitionStartTime;
  };
  autoPlay: boolean;
  onPlaybackEvent?: ((event: AnimationPlaybackEvent) => void) | undefined;
}
