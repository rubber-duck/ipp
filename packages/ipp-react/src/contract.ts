import type {
  AssetWorldClient,
  AnimationWorldClient,
  BatchOutcome,
  Client,
  GuiWorldClient,
  SurfaceWorldClient,
  Command,
} from "@ipp/client";

/** Public metadata and asynchronous operations used by the renderer. */
export type ReactWorldClient = Pick<
  Client,
  | "session"
  | "schemaHash"
  | "capabilities"
  | "components"
  | "batch"
  | "onDiagnostic"
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
  Partial<Pick<SurfaceWorldClient, "encodeSurfaceItems">> &
  Partial<
    Pick<
      GuiWorldClient,
      | "editGui"
      | "editGuiBatch"
      | "inspectGui"
      | "encodeGuiTree"
      | "decodeGuiTree"
      | "createGuiNodeHandle"
    >
  >;

export type {
  ComponentOverlayMode,
  EntityOverlayMode,
  StateOverlayLifecycleDiagnostic,
  StateOverlayRef,
} from "@ipp/client";

export type StateOverlayOutcome = BatchOutcome;
export type StateOverlayCommand = Extract<
  Command,
  {
    kind:
      | "createStateOverlayOwner"
      | "releaseStateOverlayOwner"
      | "attachEntityOverlayBinding"
      | "releaseEntityOverlayBinding"
      | "attachComponentStateOverlay"
      | "updateComponentStateOverlay"
      | "updateDynamicComponentStateOverlay"
      | "releaseComponentStateOverlay";
  }
>;
