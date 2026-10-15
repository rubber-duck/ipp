/** Shared layout roles, in the receiving World's logical units. */
export interface GuiKitSpacing {
  readonly compact: number;
  readonly label: number;
  readonly field: number;
  readonly section: number;
  readonly inset: number;
}

/** Role overrides at the skin's em; container padding can differ from control padding. */
export type GuiKitSpacingOverrides = Partial<GuiKitSpacing>;

/** One scale for fields and containers; an explicit component gap still wins. */
export function guiKitSpacing(
  tokens: { readonly inset: number; readonly spacing?: GuiKitSpacingOverrides },
  scale = 1,
): GuiKitSpacing {
  return {
    compact: (tokens.spacing?.compact ?? tokens.inset / 4) * scale,
    label: (tokens.spacing?.label ?? tokens.inset / 2) * scale,
    field: (tokens.spacing?.field ?? tokens.inset) * scale,
    section: (tokens.spacing?.section ?? tokens.inset * 1.5) * scale,
    inset: (tokens.spacing?.inset ?? tokens.inset) * scale,
  };
}
