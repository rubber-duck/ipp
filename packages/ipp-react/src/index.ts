import type { ReactNode } from "react";
import {
  ConcurrentRoot,
  DefaultEventPriority,
} from "react-reconciler/constants.js";
import { ReactWorldCommits } from "./commits.js";
import type { ReactWorldRootOptions } from "./commits.js";
import type { ReactWorldClient } from "./contract.js";
import {
  ReactWorldContainer,
  exchangePriority,
  reconciler,
} from "./reconciler.js";
import { ReactWorldTree } from "./tree.js";

export {
  ReactWorldBatchRejectedError,
  EntityOverlayBindingLostError,
} from "./commits.js";
export type { ReactWorldRootOptions } from "./commits.js";
export type {
  StateOverlayLifecycleDiagnostic,
  ReactWorldClient,
} from "./contract.js";
export type {
  EntityProps,
  ChildrenProps,
  HierarchyProps,
  LookAtProps,
  ScalarProps,
  TransformProps,
  UnlitMaterialProps,
  PbrMaterialProps,
  CustomMaterialProps,
  LightProps,
  UnlitTextureProps,
  MeshInstanceProps,
  CameraProps,
  PickingGeometryProps,
  BoundingGeometryProps,
  ComponentProps,
  SurfaceProps,
  SurfaceItemProps,
} from "./components.js";
export {
  Entity,
  Children,
  Hierarchy,
  LookAt,
  Scalar,
  Transform,
  UnlitMaterial,
  PbrMaterial,
  CustomMaterial,
  Light,
  MeshInstance,
  UnlitTexture,
  Camera,
  PickingGeometry,
  BoundingGeometry,
  Surface,
} from "./components.js";

export interface ReactWorldRoot {
  getAsset(id: string): import("./asset_state.js").ReactAssetState | undefined;
  onAssetChange(
    listener: (state: import("./asset_state.js").ReactAssetState) => void,
  ): () => void;
  render(element: ReactNode): Promise<void>;
  flush(): Promise<void>;
  unmount(): Promise<void>;
}

/** Create declarations from a connected, target-matched generated client. */
export function createRoot(
  client: ReactWorldClient,
  options: ReactWorldRootOptions = {},
): ReactWorldRoot {
  if (!client.capabilities.stateOverlays)
    throw new Error("This runtime does not support overlays");
  const tree = new ReactWorldTree(client);
  const commits = new ReactWorldCommits(client, options);
  const state = new ReactWorldContainer(tree, commits);
  const onUncaughtError = (error: Error): void => {
    state.uncaught(error);
  };
  const container = reconciler.createContainer(
    state,
    ConcurrentRoot,
    null,
    false,
    null,
    "ipp-",
    onUncaughtError,
    (error) => {
      commits.report(error);
    },
    (error) => {
      commits.report(error);
    },
    () => {},
  );
  let current: ReactNode = null;
  let closing: Promise<void> | undefined;

  const render = (element: ReactNode): Promise<void> => {
    if (closing)
      return Promise.reject(new Error("The React root is unmounted"));
    current = element;
    try {
      reconciler.flushSyncFromReconciler(() => {
        reconciler.updateContainerSync(element, container, null);
      });
    } catch (error) {
      state.uncaught(error);
    }
    state.publish();
    // React may bail out when given the same element; never retry the rejected
    // description just because a caller asks to render or flush it again.
    return commits.settled();
  };

  return {
    getAsset: (id) => commits.assets.get(id),
    onAssetChange: (listener) => commits.assets.subscribe(listener),
    render,
    async flush() {
      if (closing) return closing;
      // A default-priority root callback is an explicit React scheduling barrier
      // for ordinary hook updates, including passive effects already scheduled.
      reconciler.flushPassiveEffects();
      for (;;) {
        await new Promise<void>((resolve) => {
          const priority = exchangePriority(DefaultEventPriority);
          try {
            reconciler.updateContainer(current, container, null, resolve);
          } finally {
            exchangePriority(priority);
          }
        });
        reconciler.flushPassiveEffects();
        state.publish();
        const pending = commits.settled();
        await pending;
        // A later commit can arrive while an outcome is pending. Its promise
        // must settle too; an empty React callback commit does not restart work.
        if (pending === commits.settled()) return;
      }
    },
    unmount() {
      if (closing) return closing;
      // Local teardown is immediate; remote release follows every queued commit.
      // A failed attachment creates no resource; an acknowledged one is released.
      void render(null).catch(() => {});
      closing = commits.dispose();
      return closing;
    },
  };
}

export {
  f32,
  i32,
  u32,
  bool,
  vec2,
  vec3,
  vec4,
  mat2,
  mat3,
  mat4,
  asset,
  texture2D,
} from "@ipp/client";
export type { DynamicPropertyInput } from "@ipp/client";

export { VertexShader, FragmentShader } from "./shaders.js";
export type { ShaderProps } from "./shaders.js";

export { Asset, AnimationAsset, ShaderAsset, assetRef } from "./assets.js";
export type {
  AssetProps,
  AnimationAssetProps,
  ShaderAssetProps,
  AssetReference,
} from "./assets.js";

export type { ReactAssetState } from "./asset_state.js";

export { Animation } from "./animation.js";
export type {
  AnimationProps,
  AnimationHandle,
  AnimationBinding,
} from "./animation.js";

export {
  ParticleEmitter,
  ParticlePlayback,
  ParticleSprite,
  ParticleMesh,
} from "./components.js";
export type {
  ParticleEmitterProps,
  ParticlePlaybackProps,
  ParticleSpriteProps,
  ParticleMeshProps,
} from "./components.js";
