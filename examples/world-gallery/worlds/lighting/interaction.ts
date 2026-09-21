import type { GeometryPickResultEvent } from "@ipp/client";
import type { IppCanvasHandle } from "@ipp/react/web";
import { useCallback, useLayoutEffect, useRef } from "react";
import type { PickInteraction } from "../../shared/camera-controls.js";
import type { AnimationSession } from "./animation-controller.js";
import { SCENE_OBJECTS } from "./model.js";
import type {
  ObjectId,
  ObjectSettings,
  LightingWorldObjects,
} from "./model.js";

interface LightingState {
  objects: LightingWorldObjects;
  select(id: ObjectId | undefined): void;
  update(id: ObjectId, patch: Partial<ObjectSettings>): void;
}

/** Resolve a picked entity into a selection/drag, pausing its animation while held. */
export function useLightingInteraction(
  canvas: IppCanvasHandle | undefined,
  state: LightingState,
  session: AnimationSession | undefined,
  clearError: () => void,
) {
  const current = useRef({ state, session, clearError });
  useLayoutEffect(() => {
    current.current = { state, session, clearError };
  });

  return useCallback(
    async (
      result: GeometryPickResultEvent,
      signal: AbortSignal,
    ): Promise<PickInteraction | undefined> => {
      if (!canvas || !result.ok) return;
      const { select, update } = current.current.state;
      if (!result.hit)
        return {
          click: () => {
            if (!signal.aborted) select(undefined);
          },
          dragging: () => {},
          move: () => {},
        };
      const inspection = await canvas.client.inspect();
      if (signal.aborted) return;
      const entity = inspection.entities.find(
        (entity) => entity.id === result.hit!.entity,
      );
      const object = SCENE_OBJECTS.find(
        (object) => object.id === entity?.metadata.symbolicId,
      );
      if (!object) return;
      const resume = await current.current.session?.hold(object.id);
      if (signal.aborted) {
        resume?.();
        return;
      }
      const start = current.current.state.objects[object.id].position;

      return {
        finish: () => resume?.(),
        click: () => {
          if (!signal.aborted) {
            select(object.id);
            current.current.clearError();
          }
        },
        dragging: (active) => {
          if (active && !signal.aborted) select(object.id);
        },
        move: (delta) => {
          if (signal.aborted) return;
          update(object.id, {
            position: [
              start[0] + delta[0],
              start[1] + delta[1],
              start[2] + delta[2],
            ],
          });
          select(object.id);
          current.current.clearError();
        },
      };
    },
    [canvas],
  );
}
