import { createContext, useContext, type ReactNode } from "react";
import type {
  Client,
  OutputReference,
  PresentationViewport,
} from "@ipp/client";
import type {
  CanvasHost,
  CanvasWorldSource,
  IppCanvasHandle,
} from "@ipp/react/canvas";

export type GallerySceneId =
  | "shapes"
  | "lighting"
  | "platformer"
  | "particles"
  | "gui"
  | "charts";
export type GalleryOptions = Readonly<Record<string, unknown>>;

/** Platform asset access; identifiers retain their ordinary resource meaning. */
export interface GalleryResource {
  readonly kind: number;
  readonly path: string;
}

export interface GalleryAssets {
  prepare(
    resources: readonly GalleryResource[],
    signal?: AbortSignal,
  ): Promise<void>;
  url(path: string): string;
  readBytes(
    path: string,
    signal?: AbortSignal,
  ): Promise<Uint8Array<ArrayBuffer>>;
  readJson<T>(path: string, signal?: AbortSignal): Promise<T>;
}

export interface GallerySceneContext {
  readonly canvas: IppCanvasHandle;
  readonly assets: GalleryAssets;
  /** The generated module belonging to this exact Host target. */
  readonly contract: Readonly<Record<string, unknown>>;
  readonly signal: AbortSignal;
  readonly onError?: (error: Error) => void;
}

export interface GallerySceneMount {
  readonly output: OutputReference;
  /** Authoring/resource readiness; presentation completion is a runner barrier. */
  readonly ready: Promise<void>;
  readonly options: GalleryOptions;
  /** Scene-specific live controller consumed by browser inspectors. */
  readonly controller?: unknown;
  subscribe?(listener: () => void): () => void;
  /** Adapt authored layout before a completed frame at this viewport. */
  resize?(viewport: PresentationViewport): Promise<void>;
  update(patch: GalleryOptions): Promise<void>;
  /** The runner serializes imperative actions, which may call update. */
  action(name: string, args?: unknown): Promise<unknown>;
  inspect(): Promise<unknown>;
  /** Remove declarations before unmounting; dispose only owned resources. */
  dispose(): Promise<void>;
}

export interface GallerySceneDefinition {
  readonly id: GallerySceneId;
  readonly label: string;
  readonly shortLabel: string;
  readonly description: string;
  readonly defaultOptions: GalleryOptions;
  readonly actions: readonly string[];
  readonly resources?: readonly GalleryResource[];
  /** Primary World owned by the runner, independent of scene-owned children. */
  world(assets: GalleryAssets): CanvasWorldSource;
  initialize?(
    client: Client,
    signal: AbortSignal,
    host: CanvasHost,
    assets: GalleryAssets,
  ): Promise<void>;
  mount(
    context: GallerySceneContext,
    options: GalleryOptions,
  ): Promise<GallerySceneMount>;
}

const SceneContext = createContext<GallerySceneContext | undefined>(undefined);

export function GallerySceneProvider({
  context,
  children,
}: {
  readonly context: GallerySceneContext;
  readonly children: ReactNode;
}) {
  return <SceneContext value={context}>{children}</SceneContext>;
}

export function useGallerySceneContext(): GallerySceneContext {
  const context = useContext(SceneContext);
  if (!context) throw new Error("A gallery scene requires its runner context");
  return context;
}

/** Systems used by the gallery's authored 3D Worlds. */
export const GALLERY_SYSTEMS = [
  "ipp.world-attachment",
  "ipp.lifecycle-publisher",
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.skeleton",
  "ipp.skinning",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.camera",
  "ipp.particles",
  "ipp.surface",
  "ipp.render",
] as const;
