import type {
  AssetWorldClient,
  AnimationWorldClient,
  Client,
  GuiWorldClient,
} from "@ipp/client";

/** Public metadata and asynchronous operations used by the renderer. */
export type ReactWorldClient = Pick<
  Client,
  | "session"
  | "schemaHash"
  | "capabilities"
  | "components"
  | "manifest"
  | "batch"
> &
  Partial<
    Pick<
      Client,
      "worldReference" | "closure" | "closed" | "watchLifecycle" | "inspectPage"
    >
  > &
  Partial<
    Pick<
      AssetWorldClient,
      "registerAsset" | "releaseAsset" | "onResourceChange"
    >
  > &
  Partial<
    Pick<
      AnimationWorldClient,
      | "encodeAnimationClip"
      | "createAnimationController"
      | "updateAnimationController"
      | "transitionAnimationController"
      | "deleteAnimationController"
      | "controlAnimationController"
      | "onPlaybackEvent"
    >
  > &
  Partial<Pick<GuiWorldClient, "subscribeGuiEffects">>;
