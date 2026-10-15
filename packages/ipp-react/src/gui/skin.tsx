import { Animation } from "../animation/declarations.js";
import { createElement, useMemo } from "react";
import { Entity } from "../components.js";
import { Paint } from "./components.js";
import { Metrics, Theme, ThemeMotion } from "./theme.js";
import type { GuiKitAppearance, GuiKitContract } from "../gui-kit/kit.js";
import type { GuiSkinDefinition } from "./skin-types.js";

export function skinAppearance(
  id: string,
  skin: GuiSkinDefinition,
): GuiKitAppearance {
  const refs = (names: Readonly<Record<string, string>>) =>
    Object.fromEntries(
      Object.entries(names).map(([role, name]) => [
        role,
        `${id}/theme/${name}`,
      ]),
    );
  return {
    tokens: skin.tokens,
    themes: refs(skin.kit),
    controls: refs(skin.controls),
    metrics: Object.fromEntries(
      Object.entries(skin.controls).flatMap(([role, name]) => {
        const metrics = skin.themes[name]?.metrics;
        return metrics ? [[role, metrics]] : [];
      }),
    ),
  };
}

/** Stable entity keys let a skin change without replacing any visible controls. */
export function SkinThemes({
  id,
  skin,
  contract,
  assetScope = id,
}: {
  readonly id: string;
  readonly skin: GuiSkinDefinition;
  readonly contract: GuiKitContract;
  readonly assetScope?: string;
}) {
  const encoded = useMemo(
    () =>
      Object.entries(skin.themes).map(([name, theme]) => {
        const table = (rows: typeof theme.rows) => ({
          nextSlot: rows.length,
          rows: new Map(rows.map((row, i) => [i, row])),
        });
        return {
          name,
          theme,
          parts: contract.GuiTheme.encodeParts(table(theme.rows)),
          motion: theme.motion
            ? contract.GuiThemeMotion.encodeParts(table(theme.motion))
            : undefined,
        };
      }),
    [skin, contract],
  );
  return (
    <>
      {encoded.map(({ name, theme, parts, motion }) => (
        <Entity key={name} id={`${id}/theme/${name}`}>
          <Theme em={theme.em} parts={parts} />
          {theme.metrics && <Metrics {...theme.metrics} />}
          {motion && <ThemeMotion parts={motion} />}
          {theme.paint && createElement(Paint, theme.paint(assetScope))}
          {theme.animation && <Animation source={theme.animation} looping />}
        </Entity>
      ))}
    </>
  );
}
