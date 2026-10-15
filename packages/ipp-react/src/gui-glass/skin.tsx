import { glassKitContract } from "./kit-theme.js";
import { Metrics } from "../gui/theme.js";
import { createContext, useContext, useMemo, type ReactNode } from "react";
import { Animation } from "../animation/declarations.js";
import { Entity } from "../components.js";
import {
  assetRef,
  ShaderAsset,
  type AssetReference,
} from "../assets/declarations.js";
import { PaintShader } from "../shaders.js";
import { LIQUID_GLASS_SHADER } from "./liquid-skin-material.js";
import { AttachmentContext } from "../composition/attached-world.js";
import { Paint } from "../gui/components.js";
import { Theme, ThemeMotion } from "../gui/theme.js";
import { useReducedMotionPreference } from "../gui/preferences.js";
import { actionThemes } from "./action-themes.js";
import { valueThemes } from "./value-themes.js";
import { surfaceThemes } from "./surface-themes.js";
import { glassAlpha, resolveGlassPalette } from "./palette.js";
import {
  glassBody,
  glassLight,
  glassMaterial,
  glassPanelBacking,
  glassReflectionStrength,
  GLASS_PARAMETERS,
  GLASS_SHADER,
} from "./material.js";
import type {
  GlassContract,
  GlassLight,
  GlassMaterial,
  GlassPalette,
  GlassPaletteInput,
  GlassTheme,
  GlassThemeName,
} from "./types.js";

export interface GlassSkinProps {
  /** Liquid is the default; optical retains the earlier material study for comparison. */
  readonly appearance?: "optical" | "liquid";
  readonly id?: string;
  readonly contract: GlassContract;
  readonly font: string | AssetReference;
  readonly fontSize?: number;
  readonly tokens?: Partial<import("../gui-kit/kit.js").GuiKitTokens>;
  readonly palette?: Partial<GlassPaletteInput>;
  readonly light?: Partial<GlassLight>;
  readonly material?: Partial<GlassMaterial>;
  /** Flat panel background opacity in 0..1; defaults to 0.98. */
  readonly panelOpacity?: number;
  /** Broad face reflection bands in 0..1; defaults to 1. Edges keep their shading. */
  readonly reflectionStrength?: number;
  /** Explicit opt-in to the presenting camera domain's selected scene light. */
  readonly sceneLighting?: boolean;
  readonly reducedMotion?: boolean;
  /** Native clip on CanvasPaint light/finish properties; suppressed by reduced motion. */
  readonly lightAnimation?: string | AssetReference;
  readonly children?: ReactNode;
  /** Internal composition can supply the shared themes through GuiKit. */
  readonly declarations?: boolean;
}

export interface GlassSkinScope {
  readonly contract: GlassContract;
  readonly font: string | AssetReference;
  readonly fontSize: number;
  readonly palette: GlassPalette;
  readonly reducedMotion: boolean;
  unit(value: number): number;
  theme(name: GlassThemeName): string;
}

const GlassContext = createContext<
  (GlassSkinScope & { container: unknown }) | null
>(null);

/**
 * Declare one optional glass skin per World, outside its visible entity tree.
 * Palette/light updates mutate shared Paint properties while native controls survive.
 * Multiple palettes in a World require distinct stable ids and one preference owner.
 */
export function GlassSkin({
  appearance = "liquid",
  id = "ipp-glass",
  contract,
  font,
  fontSize = 16,
  palette: input,
  tokens,
  light: lightInput,
  material: materialInput,
  panelOpacity,
  reflectionStrength,
  sceneLighting = false,
  reducedMotion,
  lightAnimation,
  children,
  declarations = true,
}: GlassSkinProps) {
  const container = useContext(AttachmentContext);
  if (!id || !Number.isFinite(fontSize) || fontSize <= 0)
    throw new RangeError(
      "GlassSkin needs a stable id and positive finite fontSize",
    );
  const palette = useMemo(() => resolveGlassPalette(input), [input]);
  const light = glassLight(lightInput);
  const material = glassMaterial(materialInput);
  const backing = glassPanelBacking(palette.background, panelOpacity);
  const reflections = glassReflectionStrength(reflectionStrength);
  const themes = useMemo<Record<GlassThemeName, GlassTheme>>(() => {
    const inputs = glassKitContract(contract, palette, tokens);
    return {
      ...actionThemes(inputs, palette),
      ...valueThemes(inputs, palette),
      ...surfaceThemes(inputs, palette),
    };
  }, [contract, palette, tokens]);
  const encoded = useMemo(
    () =>
      Object.entries(themes).map(([name, theme]) => ({
        name,
        em: theme.em,
        metrics: theme.metrics,
        motion: theme.motion
          ? contract.GuiThemeMotion.encodeParts({
              nextSlot: theme.motion!.length,
              rows: new Map(theme.motion!.map((row, index) => [index, row])),
            })
          : undefined,
        parts: contract.GuiTheme.encodeParts({
          nextSlot: theme.rows.length,
          rows: new Map(theme.rows.map((row, index) => [index, row])),
        }),
      })),
    [contract, themes],
  );
  const scope = useMemo(
    () => ({
      contract,
      font,
      fontSize,
      palette,
      reducedMotion: reducedMotion ?? false,
      container,
      unit: (length: number) => (length * fontSize) / 16,
      theme: (name: GlassThemeName) => `${id}/theme/${name}`,
    }),
    [contract, font, fontSize, palette, reducedMotion, container, id],
  );
  useReducedMotionPreference(container?.commits, reducedMotion);
  return (
    <GlassContext value={scope}>
      {declarations && (
        <Entity id={`${id}/material`}>
          <ShaderAsset
            id={`${id}/shader`}
            recipe={{ presentation: true, lighting: sceneLighting }}
            parameters={GLASS_PARAMETERS}
          >
            <PaintShader>
              {appearance === "liquid" ? LIQUID_GLASS_SHADER : GLASS_SHADER}
            </PaintShader>
          </ShaderAsset>
        </Entity>
      )}
      {declarations &&
        encoded.map((theme) => (
          <Entity
            key={theme.name}
            id={scope.theme(theme.name as GlassThemeName)}
          >
            <Theme parts={theme.parts} em={theme.em} />
            {theme.metrics && <Metrics {...theme.metrics} />}
            {theme.motion && <ThemeMotion parts={theme.motion} />}
            <Paint
              source={assetRef(`${id}/shader`)}
              accent={glassAlpha(palette.primary, reflections)}
              glassTint={glassAlpha(palette.secondary, backing[3])}
              light={[light.x, light.y, light.height, light.intensity]}
              finish={[
                material.ior,
                material.roughness,
                material.thickness,
                material.edge,
              ]}
              lighting={[
                sceneLighting ? 1 : 0,
                backing[0],
                backing[1],
                backing[2],
              ]}
              body={glassBody(theme.name)}
            />
            {lightAnimation && !reducedMotion && (
              <Animation source={lightAnimation} looping />
            )}
          </Entity>
        ))}
      {children}
    </GlassContext>
  );
}

export function useGlassSkin(): GlassSkinScope {
  const scope = useContext(GlassContext);
  const container = useContext(AttachmentContext);
  if (!scope || scope.container !== container)
    throw new Error("Glass components need GlassSkin inside their own World");
  return scope;
}
