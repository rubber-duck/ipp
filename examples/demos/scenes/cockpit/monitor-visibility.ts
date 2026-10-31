import { carouselStep } from "./carousel.js";

/** Conservative admission for the authored, forward-facing seated camera.
 * Camera edits and carousel motion keep the monitor active. Wide viewports keep
 * neighbouring panes active too; this is not a selected-tab visibility rule. */
export function monitorInView(
  focus: number,
  radius: number,
  width: number,
  verticalFov: number,
  aspect: number,
): boolean {
  const halfView = Math.atan(Math.tan(verticalFov / 2) * Math.max(1, aspect));
  const nearestEdge =
    Math.abs(1 - focus) * carouselStep(width, radius) - width / (2 * radius);
  // Include decoration and small residual spring motion outside the pane.
  return nearestEdge <= halfView + 0.1;
}
