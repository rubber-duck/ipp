/**
 * Rings of the kit: the arc shape painted by a skinned entity that is not a
 * control. Its part row paints a ring sector centred in the entity's box, the
 * outer edge on the shorter side, `border_width` thick, from `arc_start`
 * through `arc_sweep` turns clockwise from twelve o'clock. The arc themes give
 * the shape and colour; each ring's own row gives its thickness (absolute in
 * the World's units), start, sweep and opacity.
 *
 * A turning arc turns its start once a second and a cue fades its opacity,
 * both through the Host-clock motions of `row-motion.tsx`, which hold still
 * under the kit's reduced motion. The start wraps at a whole turn, so the
 * turn's return from one turn to none shows no jump.
 */
import type { ReactNode } from "react";
import { Children, Entity } from "../components.js";
import { Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_STACK, type GuiKitLayout } from "./layout.js";
import { PULSE_REST, Pulse, Turn, useOwnRow } from "./row-motion.js";
import type { KitThemeName } from "./themes.js";

/** Lit share of a turning arc: a quarter of the ring. */
export const TURNING_SWEEP = 0.25;

export interface ArcProps {
  readonly id: string;
  readonly theme: KitThemeName;
  /** Outer diameter at the tokens' `em`. */
  readonly size: number;
  /** Ring thickness at the tokens' `em`. */
  readonly thickness: number;
  /** Turns from twelve o'clock, clockwise, where the arc starts. */
  readonly start?: number;
  /** Turns from the start, clockwise; the whole ring by default. */
  readonly sweep?: number;
  /** Turn the arc once a second on the Host clock. */
  readonly turning?: boolean;
  /** A liveness cue: drawn at the pulse's rest opacity, and fading. */
  readonly cue?: boolean;
  readonly layout?: GuiKitLayout;
  /** Entities centred over the ring, such as its lit arc or a readout. */
  readonly children?: ReactNode;
}

/** A ring sector `size` across, centred in its container by default. */
export function Arc({
  id,
  theme,
  size,
  thickness,
  start = 0,
  sweep = 1,
  turning = false,
  cue = false,
  layout,
  children,
}: ArcProps) {
  const kit = useGuiKit();
  const parts = useOwnRow({
    border_width: kit.unit(thickness),
    // Present, so that a turn can bind it.
    arc_start: start,
    arc_sweep: sweep,
    ...(cue ? { opacity: PULSE_REST } : {}),
  });
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_STACK}
        width={kit.unit(size)}
        height={kit.unit(size)}
        align_x={0}
        align_y={0}
        {...layout}
      />
      <Skin theme={kit.theme(theme)} parts={parts} />
      {children !== undefined && <Children>{children}</Children>}
      {turning && <Turn target={id} />}
      {cue && <Pulse target={id} />}
    </Entity>
  );
}
