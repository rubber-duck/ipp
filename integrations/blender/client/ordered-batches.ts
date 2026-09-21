/** Blender uses the shared client command-page writer. */
import type { Command } from "@ipp/client";
export {
  applyCommandPages as applyBlenderBatches,
  CommandEncodingError as BlenderCommandError,
} from "@ipp/client";

export type BlenderCommand = Extract<
  Command,
  {
    kind:
      | "create"
      | "delete"
      | "setMetadata"
      | "insertComponent"
      | "setField"
      | "removeComponent";
  }
>;
