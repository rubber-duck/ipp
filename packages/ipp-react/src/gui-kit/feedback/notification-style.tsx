import { useState } from "react";
import { Style } from "../../gui/components.js";
import { useGuiKit } from "../kit.js";

/** One transform for the card and its contents throughout its visible lifetime. */
export function NotificationStyle({
  layer,
  opacity,
}: {
  layer: number;
  opacity?: number;
}) {
  const angles = useGuiKit().notificationAngles;
  const [sample] = useState(Math.random);
  const rotation = angles?.[Math.floor(sample * angles.length)] ?? 0;
  return (
    <Style
      layer={layer}
      {...(opacity === undefined ? {} : { opacity })}
      rotation={rotation}
      pivot_x={0.5}
      pivot_y={0.5}
    />
  );
}
