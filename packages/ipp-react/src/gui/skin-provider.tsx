import { PlotSkinContext } from "../plots/skin-context.js";
import { GuiSkinContext, type GuiSkinContextValue } from "./skin-context.js";
import {
  createElement,
  Fragment,
  useContext,
  useMemo,
  type ReactNode,
} from "react";
import type { AssetReference } from "../assets/declarations.js";
import { AttachmentContext } from "../composition/attached-world.js";
import type { GuiKitAppearance, GuiKitContract } from "../gui-kit/kit.js";
import { useReducedMotionPreference } from "./preferences.js";
import { useSkinSelection } from "./skin-selection.js";
import { SkinThemes, skinAppearance } from "./skin.js";
import { Resources } from "../resources.js";
import { useEntityId } from "../scope/identity.js";
import { useOptionalWorld } from "../scope/world.js";

export interface SkinProviderProps {
  /**
   * The receiving runtime's generated contract module. Omit it to use the
   * receiving World's contract or inherit from another `SkinProvider`.
   */
  readonly contract?: GuiKitContract;
  /**
   * The GUI font kit text draws with:
   * a font source or an asset reference. Omit it inside another `SkinProvider` to
   * inherit.
   */
  readonly font?: string | AssetReference;
  /** Icon font, independent of proportional body text. Defaults to the inherited icon font or the text font. */
  readonly iconFont?: string | AssetReference;
  /**
   * Body text size in the World's logical units. Kit lengths are drawn
   * `fontSize / tokens.em` times their design size. Omit it inside
   * another `SkinProvider` to inherit.
   */
  readonly fontSize?: number;
  /**
   * Hold motion still: the World's GUI preference, which snaps the runtime's
   * skin transitions, and the kit's own animations, which stand still or
   * end at once. The kit that declares the World's themes sends it to the
   * World; a nested kit only holds the kit animations beneath it. Omit it to
   * inherit; omitted everywhere, the kit sends nothing and its animations
   * move.
   */
  readonly reducedMotion?: boolean;
  readonly appearance?: GuiKitAppearance;
  readonly skin?: import("./skin-types.js").GuiSkinDefinition;
  /** Stable id when several independent skins share a World. */
  readonly id?: string;
  readonly children?: ReactNode;
}

/**
 * Provide a shared client skin to native controls and kit components.
 * Shared themes remain outside native layout parenting, so providers may
 * appear inside ordinary layouts. Nested providers inherit within the same
 * World or select an independent skin.
 * Attached Worlds need their own provider and World-local theme entities.
 */
export function SkinProvider({
  contract,
  font,
  iconFont,
  fontSize,
  reducedMotion,
  appearance,
  skin,
  id,
  children,
}: SkinProviderProps) {
  const outer = useContext(GuiSkinContext);
  const outerPlots = useContext(PlotSkinContext);
  const container = useContext(AttachmentContext);
  const world = useOptionalWorld();
  const scopeId = useEntityId(id);
  const selection = useSkinSelection(scopeId, skin);
  const activeSkin = selection.selected?.skin;
  const plots = useMemo(
    () =>
      skin
        ? activeSkin?.plots?.(selection.selected!.scope)
        : outerPlots?.container === container
          ? outerPlots.appearance
          : undefined,
    [skin, activeSkin, selection.selected?.scope, outerPlots, container],
  );
  const selected = useMemo(
    () =>
      (activeSkin ?? skin)
        ? skinAppearance(scopeId, (activeSkin ?? skin)!)
        : appearance,
    [activeSkin, skin, scopeId, appearance],
  );
  const resolved = {
    contract:
      contract ??
      world?.contract ??
      (outer?.container === container ? outer.contract : undefined),
    font: font ?? outer?.font,
    iconFont: iconFont ?? outer?.iconFont ?? font ?? outer?.font,
    fontSize: fontSize ?? outer?.fontSize,
    reducedMotion: reducedMotion ?? outer?.reducedMotion,
    appearance:
      selected ??
      (outer?.container === container ? outer.appearance : undefined),
  };
  if (
    !resolved.contract ||
    !resolved.font ||
    resolved.fontSize === undefined ||
    !resolved.appearance
  )
    throw new Error(
      "SkinProvider needs a skin, font and fontSize from props or an enclosing provider, and a receiving World contract or explicit contract",
    );
  if (!(resolved.fontSize > 0) || !Number.isFinite(resolved.fontSize))
    throw new RangeError("SkinProvider fontSize must be positive and finite");
  const declares = outer?.container !== container;
  const emptyParts = useMemo(
    () =>
      resolved.contract!.GuiSkin.encodeParts({ nextSlot: 0, rows: new Map() }),
    [resolved.contract],
  );
  const value = useMemo<GuiSkinContextValue>(
    () => ({
      emptyParts,
      contract: resolved.contract!,
      font: resolved.font!,
      iconFont: resolved.iconFont!,
      fontSize: resolved.fontSize!,
      reducedMotion: resolved.reducedMotion,
      appearance: resolved.appearance!,
      container,
    }),
    [
      emptyParts,
      resolved.contract,
      resolved.font,
      resolved.iconFont,
      resolved.fontSize,
      resolved.reducedMotion,
      resolved.appearance,
      container,
    ],
  );
  useReducedMotionPreference(
    declares ? container?.commits : undefined,
    resolved.reducedMotion,
  );
  return createElement(
    GuiSkinContext,
    { value },
    createElement(
      Resources,
      null,
      selection.resources,
      activeSkin &&
        createElement(SkinThemes, {
          id: scopeId,
          skin: activeSkin,
          contract: resolved.contract!,
          assetScope: selection.selected!.scope,
        }),
    ),
    createElement(
      PlotSkinContext,
      { value: { container, appearance: plots } },
      createElement(Fragment, null, skin && !activeSkin ? null : children),
    ),
  );
}
