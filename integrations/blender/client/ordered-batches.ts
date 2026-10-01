/** Commands the Blender adapter authors through the shared client batches. */
import type { Command } from "@ipp/client";

export type BlenderCommand = Extract<
  Command,
  {
    kind:
      | "create"
      | "delete"
      | "placeEntity"
      | "setMetadata"
      | "insertComponent"
      | "setField"
      | "removeComponent";
  }
>;
