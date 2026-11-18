import type { ReactNode } from "react";
import { useEntityId } from "../../scope/identity.js";
import { useGuiKit } from "../kit.js";
import { Floating, type GuiKitOverlaySide } from "./overlay.js";

/** Place inside the trigger's Children. Rich content participates in layout in both modes. */
export interface HoverCardProps {
  readonly id?: string;
  readonly side?: GuiKitOverlaySide;
  readonly interactive?: boolean;
  readonly width?: number;
  readonly children?: ReactNode;
}

export function HoverCard({
  id: supplied,
  side = "bottom",
  interactive = false,
  width,
  children,
}: HoverCardProps) {
  const id = useEntityId(supplied);
  const kit = useGuiKit();
  return (
    <Floating
      id={id}
      open={false}
      side={side}
      align="start"
      mode={interactive ? "hoverCard" : "hint"}
      layout={{
        width: width ?? kit.unit(240),
        fit_height: true,
        padding_left: kit.spacing.inset,
        padding_right: kit.spacing.inset,
        padding_top: kit.spacing.label,
        padding_bottom: kit.spacing.label,
      }}
    >
      {children}
    </Floating>
  );
}
