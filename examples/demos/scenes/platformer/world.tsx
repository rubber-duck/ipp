import { World } from "@ipp/react/canvas";
import type { ReactWorldRoot } from "@ipp/react";
import { PlatformerMovementPanel } from "./movement-panel.js";
import type { ComponentProps } from "react";

/** React adds runtime choreography to the Blender-authored hierarchy. */
export function PlatformerWorld({
  onCommit,
  movement,
}: {
  onCommit: (root: ReactWorldRoot) => void;
  movement: ComponentProps<typeof PlatformerMovementPanel>;
}) {
  return (
    <World onCommit={onCommit} onError={movement.movement.report}>
      <PlatformerMovementPanel {...movement} />
    </World>
  );
}
