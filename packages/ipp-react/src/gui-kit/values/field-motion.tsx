import { useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  Animation,
  type AnimationHandle,
} from "../../animation/declarations.js";
import { Resources } from "../../resources.js";
import { Entity } from "../../components.js";
import { AnimationAsset, assetRef } from "../../assets/declarations.js";
import { useWorld } from "../../scope/world.js";
import { useGuiKit } from "../kit.js";

/** The native controller's playhead is the normalized position. Retargeting a
 * spring preserves its rendered position and velocity. A held sample snaps. */
export function FieldMotion({
  target,
  component,
  field,
  from,
  to,
  fraction,
  immediate = false,
}: {
  target: string;
  component: "CanvasStyle" | "GuiLayout";
  field: string;
  from: number;
  to: number;
  fraction: number;
  immediate?: boolean;
}) {
  const kit = useGuiKit(),
    world = useWorld(),
    ref = useRef<AnimationHandle | null>(null);
  const descriptor = world.client.components[component];
  const offset = descriptor?.fields[field]?.offset;
  if (!descriptor || offset === undefined)
    throw new Error(`Missing animation field ${component}.${field}`);
  const id = `${target}/motion/${field}`;
  const clip = useMemo(
    () => ({
      duration: 1,
      tracks: [
        {
          property: { component: descriptor.id, offsets: [offset] },
          keys: [from, to].map((value, time) => ({
            time,
            value: { kind: "f32" as const, value },
          })),
        },
      ],
    }),
    [descriptor, offset, from, to],
  );
  const initial = useRef(true);
  const [failure, setFailure] = useState<Error | null>(null);
  useLayoutEffect(() => {
    let active = true;
    const snap = initial.current || kit.reducedMotion || immediate;
    initial.current = false;
    void ref.current
      ?.motion(
        snap
          ? {
              mode: { kind: "hold" },
              position: fraction,
              velocity: 0,
              min: 0,
              max: 1,
            }
          : {
              mode: {
                kind: "spring",
                target: fraction,
                frequency: 4,
                damping: 1,
              },
              min: 0,
              max: 1,
            },
      )
      .catch((reason: unknown) => {
        if (active)
          setFailure(
            reason instanceof Error ? reason : new Error(String(reason)),
          );
      });
    return () => {
      active = false;
    };
  }, [fraction, kit.reducedMotion, immediate, clip]);
  if (failure) throw failure;
  return (
    <Resources>
      <Entity bindTo={target}>
        <AnimationAsset id={id} clip={clip} />
        <Animation source={assetRef(id)} autoPlay={false} ref={ref} />
      </Entity>
    </Resources>
  );
}
