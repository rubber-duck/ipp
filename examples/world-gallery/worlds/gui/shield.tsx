import type { BoundingShape, GeometryEncoder } from "@ipp/client";
import {
  BoundingGeometry,
  CustomMaterial,
  Entity,
  MeshInstance,
  PickingGeometry,
  Transform,
  assetRef,
} from "@ipp/react";
import { GALLERY_RUNTIME } from "../../shared/runtime.js";
import { SHIELD_CONTENT_RECT } from "./dashboard.js";
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  UNITS_PER_METRE,
} from "./presentation.js";
import { PANEL_SCALE, placedOnPanel } from "./projector.js";

const { encodeBoundingShape }: { encodeBoundingShape: GeometryEncoder } =
  await import(`${GALLERY_RUNTIME}generated.js`);

/** Symbolic ID the scene resolves to name the shield as a GUI input blocker. */
export const SHIELD_ENTITY = "gui-input-shield";

export const SHIELD_MESH = "ipp://mesh/cube?width=1&height=1&length=1";

/** Gap between the panel and the shield's back face, and the shield
 * thickness, in metres. The gap keeps the glass clear of the panel's depth
 * while the parallax at the authored camera stays inside the shield's margin
 * around PURGE. */
const SHIELD_GAP = 0.08;
const SHIELD_THICKNESS = 0.02;

/** The unit cube's picking box; the Transform scale sizes it with the mesh. */
const SHIELD_PICKING: BoundingShape = {
  type: "box",
  min: [-0.5, -0.5, -0.5],
  max: [0.5, 0.5, 0.5],
};

/**
 * A glass shield in front of PURGE, drawn as its frame and hatch marks. It
 * is ordinary scene geometry with picking geometry; the gallery passes its
 * entity to `IppCanvas.guiInput.blockers` while armed, so pointer and wheel
 * input whose camera ray meets the glass before the panel is blocked.
 * Lifting the shield keeps the glass in place and only stops marking it,
 * because visual occlusion alone never blocks GUI input. PURGE lies on the
 * panel's base plane with the rest of its panel, so the shield stays where
 * it is when the panel explodes.
 */
export function InputShield({
  armed,
  stagingX,
}: {
  armed: boolean;
  stagingX: number;
}) {
  const [x, y, width, height] = SHIELD_CONTENT_RECT;
  const metres = PANEL_SCALE / UNITS_PER_METRE;
  const size = [width * metres, height * metres] as const;
  const placement = placedOnPanel(
    [
      (x + width / 2 - CANVAS_WIDTH / 2) * metres,
      (CANVAS_HEIGHT / 2 - y - height / 2) * metres,
      SHIELD_GAP + SHIELD_THICKNESS / 2,
    ],
    stagingX,
  );
  return (
    <Entity id={SHIELD_ENTITY}>
      <Transform
        {...placement}
        sx={size[0]}
        sy={size[1]}
        sz={SHIELD_THICKNESS}
      />
      <MeshInstance source={SHIELD_MESH} />
      <BoundingGeometry />
      <PickingGeometry geometry={encodeBoundingShape(SHIELD_PICKING)} />
      <CustomMaterial
        source={assetRef("gui-input-shield-shader")}
        size={size}
        // Armed: amber frame and hatching. Lifted: a plain grey frame, still
        // in front of the button.
        color={armed ? [1, 0.62, 0.12, 1] : [0.38, 0.42, 0.46, 1]}
        hatch={armed ? 1 : 0}
        // Cutout marks write depth, so the translucent panel drawn behind
        // them never washes them out; the glass between them is cut away.
        alpha_mode={1}
        receives_light={false}
        receives_shadows={false}
        casts_shadows={false}
      />
    </Entity>
  );
}
