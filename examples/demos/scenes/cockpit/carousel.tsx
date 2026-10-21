/** One Host-driven rotor moves every persistent pane through the same cylinder. */
import { useEffect, useState, useSyncExternalStore } from "react";
import { Animation, AnimationAsset, SurfaceCache, assetRef } from "@ipp/react";
import type { AnimationClipSource } from "@ipp/client";
import type { CockpitNavigationSource } from "./control-screen.js";
import { COCKPIT_PANE_COUNT, type CockpitOptions } from "./options.js";

export const CAROUSEL_ENTITY = "cockpit-carousel";
export const CAROUSEL_ASSET = "cockpit-carousel-rotation";
export const CAROUSEL_SPRING_FREQUENCY = 4.5;
export const CAROUSEL_SPRING_DAMPING = 0.82;
export const CAROUSEL_GAP = 0.07;

/** Only this small declaration observes drag progress; pane trees stay retained.
 * Both incoming and outgoing panes stay sharp through a focus handover. */
export function CarouselQuality({
  index,
  navigation,
  caching,
  refreshHz,
  textureSize,
}: {
  index: number;
  navigation: CockpitNavigationSource;
  caching: CockpitOptions["surfaceCaching"];
  refreshHz: number;
  textureSize: number;
}) {
  const near = useSyncExternalStore(
    navigation.subscribe,
    () => Math.abs(navigation.focus() - index) < 1,
  );
  const [retained, setRetained] = useState(near);
  useEffect(() => {
    if (near) {
      setRetained(true);
      return;
    }
    // Keep the outgoing pane sharp through the rotor's spring settling.
    // This delays only a quality downgrade; native motion owns the actual pose.
    const timer = setTimeout(() => setRetained(false), 800);
    return () => clearTimeout(timer);
  }, [near]);
  // Disabling optional caching keeps even side panes current. Curved geometry
  // still needs rasterization, with the same sharp resolution as a focused pane.
  const current =
    caching === "off" || (caching === "auto" && (near || retained));
  const sharp = caching !== "auto" || near || retained;
  return (
    <SurfaceCache
      resolution_scale={sharp ? 1.5 : 0.75}
      direct_distance={current ? 100 : 0}
      max_refresh_hz={refreshHz}
      max_texture_size={textureSize}
      force_cache={caching === "force"}
    />
  );
}

export function carouselStep(width: number, radius: number) {
  return (width + CAROUSEL_GAP) / radius;
}

export function CarouselRotation({
  component,
  offsets,
  step,
}: {
  component: number;
  offsets: readonly number[];
  step: number;
}) {
  const rotation = (value: number) => ({
    kind: "rotation" as const,
    value: [0, Math.sin(value / 2), 0, Math.cos(value / 2)] as const,
  });
  const clip: AnimationClipSource = {
    duration: COCKPIT_PANE_COUNT,
    tracks: [
      {
        property: { component, offsets },
        keys: [
          ...Array.from({ length: COCKPIT_PANE_COUNT }, (_, time) => ({
            time,
            value: rotation(time * step),
          })),
          {
            time: COCKPIT_PANE_COUNT,
            value: rotation((COCKPIT_PANE_COUNT - 1) * step),
          },
        ],
      },
    ],
  };
  return (
    <>
      <AnimationAsset id={CAROUSEL_ASSET} clip={clip} />
      <Animation
        source={assetRef(CAROUSEL_ASSET)}
        target={CAROUSEL_ENTITY}
        speed={0}
        autoPlay={false}
      />
    </>
  );
}
