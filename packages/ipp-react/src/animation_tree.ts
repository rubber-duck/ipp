import type { AnimationClipSource, AnimationDriverTarget } from "@ipp/client";
import { isAssetReference } from "./assets.js";
import {
  AnimationMailbox,
  type AnimationDescription,
  type AnimationBinding,
  type AnimationProps,
} from "./animation.js";
import type { EntityOverlayBindingDescription } from "./tree.js";

export function describeAnimation(
  identity: number,
  props: AnimationProps & { mailbox: AnimationMailbox },
  parent: number | undefined,
  entities: readonly EntityOverlayBindingDescription[],
  assets: ReadonlyMap<string, { kind: number; clip?: AnimationClipSource }>,
): AnimationDescription {
  if (!(props.mailbox instanceof AnimationMailbox))
    throw new Error("Invalid Animation declaration");
  const sourceClip = (source: AnimationProps["source"]) => {
    if (isAssetReference(source)) {
      const asset = assets.get(source.assetId);
      if (!asset) throw new Error(`Unknown asset id: ${source.assetId}`);
      if (asset.kind !== 10)
        throw new Error(
          `Animation requires an animation asset: ${source.assetId}`,
        );
      return asset.clip;
    }
    if (typeof source !== "string" || !source)
      throw new Error("Animation requires a source");
    return undefined;
  };
  const bindings: readonly AnimationBinding[] | undefined =
    props.bindings ??
    sourceClip(props.source)?.tracks.map((_, track) => ({ track }));
  if (!bindings?.length)
    throw new Error(
      "Animation requires bindings or an AnimationAsset with track hints",
    );
  const resolved = bindings.map((binding) => {
    const source = binding.source ?? props.source;
    const clip = sourceClip(source);
    if (
      !Number.isInteger(binding.track) ||
      binding.track < 0 ||
      (clip && binding.track >= clip.tracks.length)
    )
      throw new Error("Invalid Animation track");
    const hint = clip?.tracks[binding.track];
    const property =
      binding.property ??
      (hint && ("joints" in hint ? { joints: hint.joints } : hint.property));
    if (!property)
      throw new Error(
        "Animation binding requires a property or clip track hint",
      );
    const target = binding.target ?? props.target;
    let entity = parent;
    if (typeof target === "string") {
      const matches = entities.filter((entry) => entry.symbolicId === target);
      if (matches.length !== 1)
        throw new Error(
          `Animation target must identify one scene Entity: ${target}`,
        );
      entity = matches[0]!.identity;
    } else if (target !== undefined && typeof target !== "bigint")
      throw new Error("Invalid Animation target");
    if (typeof target !== "bigint" && entity === undefined)
      throw new Error("Animation requires a target or enclosing Entity");
    // Capture mutable authoring arrays at the committed boundary.
    return {
      ...binding,
      source: isAssetReference(source) ? { assetId: source.assetId } : source!,
      property: structuredClone(property) as AnimationDriverTarget,
      target: typeof target === "bigint" ? target : { entity: entity! },
    };
  });
  if (props.speed !== undefined && !Number.isFinite(props.speed))
    throw new Error("Animation speed must be finite");
  if (
    props.transition !== undefined &&
    (!Number.isFinite(props.transition.duration) ||
      props.transition.duration < 0)
  )
    throw new Error(
      "Animation transition duration must be finite and nonnegative",
    );
  if (
    props.transition?.easing !== undefined &&
    !["linear", "smoothstep"].includes(props.transition.easing)
  )
    throw new Error("Invalid Animation transition easing");
  if (
    props.transition?.startTime?.policy === "seek" &&
    (!Number.isFinite(props.transition.startTime.time) ||
      props.transition.startTime.time < 0)
  )
    throw new Error("Animation transition time must be finite and nonnegative");
  if (
    props.transition?.startTime !== undefined &&
    !["restart", "preserve", "matchPhase", "seek"].includes(
      props.transition.startTime.policy,
    )
  )
    throw new Error("Invalid Animation transition start time");
  for (const key of ["looping", "autoPlay"] as const)
    if (props[key] !== undefined && typeof props[key] !== "boolean")
      throw new Error(`Animation ${key} must be boolean`);
  return {
    identity,
    mailbox: props.mailbox,
    bindings: resolved,
    speed: props.speed ?? 1,
    looping: props.looping ?? false,
    ...(props.transition === undefined
      ? {}
      : {
          transition: {
            duration: props.transition.duration,
            easing: props.transition.easing ?? "linear",
            startTime:
              props.transition.startTime === undefined
                ? { policy: "restart" as const }
                : structuredClone(props.transition.startTime),
          },
        }),
    autoPlay: props.autoPlay ?? false,
    onPlaybackEvent: props.onPlaybackEvent,
  };
}

export function animationSignature(value: unknown): string {
  return JSON.stringify(value, (_key, entry) =>
    typeof entry === "bigint" ? `${entry}n` : entry,
  );
}

export function animationMutation(
  previousSignature: string | undefined,
  previousDriverSignature: string | undefined,
  signature: string,
  driverSignature: string,
  transition: boolean,
): "none" | "update" | "transition" {
  if (previousSignature === signature) return "none";
  return transition && previousDriverSignature !== driverSignature
    ? "transition"
    : "update";
}
