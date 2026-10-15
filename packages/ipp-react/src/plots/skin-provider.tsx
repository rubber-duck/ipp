import { useContext, useMemo, type ReactNode } from "react";
import { AttachmentContext } from "../composition/attached-world.js";
import { Resources } from "../resources.js";
import { useEntityId } from "../scope/identity.js";
import { useSkinSelection } from "../gui/skin-selection.js";
import type { GuiSkinDefinition } from "../gui/skin-types.js";
import { PlotSkinContext } from "./skin-context.js";

/** Chart-only skin resources for camera Worlds that do not select GUI or Canvas.
 * Ordinary SkinProvider supplies this same context when GUI and charts coexist.
 */
export function PlotSkinProvider({
  skin,
  id,
  children,
}: {
  readonly skin: GuiSkinDefinition;
  readonly id?: string;
  readonly children?: ReactNode;
}) {
  const container = useContext(AttachmentContext);
  const scope = useEntityId(id);
  const selection = useSkinSelection(scope, skin);
  const selected = selection.selected;
  const appearance = useMemo(
    () => selected?.skin.plots?.(selected.scope),
    [selected?.skin, selected?.scope],
  );
  const value = useMemo(
    () => ({ container, appearance }),
    [container, appearance],
  );
  return (
    <PlotSkinContext value={value}>
      <Resources>{selection.resources}</Resources>
      {selected ? children : null}
    </PlotSkinContext>
  );
}
