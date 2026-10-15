import { useMemo } from "react";
import { assetRef, ShaderAsset } from "../assets/declarations.js";
import { Entity } from "../components.js";
import { PaintShader } from "../shaders.js";
import { LIQUID_GLASS_SHADER } from "./liquid-skin-material.js";
import { GuiKit } from "../gui-kit/kit.js";
import { kitThemes, type KitThemeName } from "../gui-kit/themes.js";
import type { GuiSkinDefinition, GuiSkinTheme } from "../gui/skin-types.js";
import { GlassSkin, type GlassSkinProps } from "./skin.js";
import { glassTextRows } from "./rows.js";
import {
  glassLight,
  glassMaterial,
  glassPanelBacking,
  glassReflectionStrength,
  glassBody,
  GLASS_PARAMETERS,
  GLASS_SHADER,
} from "./material.js";
import { glassAlpha, resolveGlassPalette } from "./palette.js";
import {
  glassKitContract,
  glassKitTheme,
  type GlassKitContract,
  type GlassKitTheme,
} from "./kit-theme.js";
import { selectionKitThemes } from "./selection-kit-themes.js";
import { valueKitThemes, valueKitControls } from "./value-kit-themes.js";
import {
  containerKitThemes,
  containerKitControls,
} from "./container-kit-themes.js";
import { actionThemes } from "./action-themes.js";
import { valueThemes } from "./value-themes.js";
import { surfaceThemes } from "./surface-themes.js";
import { glassMetrics } from "./metrics.js";
import { GlassChartAssets, glassChartProperties } from "./chart.js";

const glassAssets = (scope: string, sceneLighting: boolean, liquid = false) => (
  <>
    <Entity id={`${scope}/material`}>
      <ShaderAsset
        id={`${scope}/shader`}
        recipe={{ presentation: true, lighting: sceneLighting }}
        parameters={GLASS_PARAMETERS}
      >
        <PaintShader>{liquid ? LIQUID_GLASS_SHADER : GLASS_SHADER}</PaintShader>
      </ShaderAsset>
    </Entity>
    <GlassChartAssets id={`${scope}/charts`} />
  </>
);

const authoredAssets = (scope: string) => glassAssets(scope, false);
const sceneAssets = (scope: string) => glassAssets(scope, true);
const liquidAuthoredAssets = (scope: string) => glassAssets(scope, false, true);
const liquidSceneAssets = (scope: string) => glassAssets(scope, true, true);

export type GlassSkinOptions = Pick<
  GlassSkinProps,
  | "tokens"
  | "appearance"
  | "palette"
  | "light"
  | "material"
  | "panelOpacity"
  | "reflectionStrength"
  | "sceneLighting"
  | "lightAnimation"
  | "reducedMotion"
>;

/** A complete Glass skin, including basic controls, composite recipes and one shared material. */
export function glassSkin(
  runtime: GlassKitContract,
  options: GlassSkinOptions = {},
): GuiSkinDefinition {
  const palette = resolveGlassPalette(options.palette);
  const light = glassLight(options.light);
  const material = glassMaterial(options.material);
  const backing = glassPanelBacking(palette.background, options.panelOpacity);
  const reflections = glassReflectionStrength(options.reflectionStrength);
  const sheetTint = glassAlpha(palette.secondary, backing[3]);
  const sheetLighting = [
    options.sceneLighting ? 1 : 0,
    backing[0],
    backing[1],
    backing[2],
  ] as const;
  const contract = glassKitContract(runtime, palette, options.tokens);
  const base = kitThemes(contract);
  const shared: Partial<Record<KitThemeName, GlassKitTheme>> = {};
  for (const name of [
    "secondary",
    "secondarySmall",
    "secondarySmallAmber",
    "amber",
    "frame",
  ] as const) {
    shared[name] = glassKitTheme(contract, palette, base[name], {
      accent: name.toLowerCase().includes("amber")
        ? palette.warning
        : palette.primary,
      rows: [
        ...glassTextRows(contract, palette),
        ...(name === "frame"
          ? [
              {
                part: contract.guiPaintPartIndex({ part: "background" }),
                corner_cut: [0, 0, 0, 0],
              },
            ]
          : []),
      ],
    });
  }
  for (const name of ["quiet", "division"] as const) {
    shared[name] = {
      ...base[name],
      rows: base[name].rows.map((row) => ({
        ...row,
        paint: false,
        glow_intensity: 0,
        color: [
          ...palette.secondary
            .slice(0, 3)
            .map((channel) => channel * (name === "quiet" ? 0.12 : 0.08)),
          1,
        ],
      })),
    };
  }
  const themes = {
    ...base,
    ...shared,
    ...selectionKitThemes(contract, palette, base),
    ...valueKitThemes(contract, palette, base),
    ...containerKitThemes(contract, palette, base),
  };
  const controls = {
    ...valueKitControls(contract, palette),
    ...containerKitControls(contract, palette),
  };
  const basic = {
    ...actionThemes(contract, palette),
    ...valueThemes(contract, palette),
    ...surfaceThemes(contract, palette),
  };
  const definition: Record<string, GuiSkinTheme> = {};
  const finish = (recipe: GlassKitTheme): GuiSkinTheme => ({
    ...recipe,
    metrics: recipe.metrics ?? glassMetrics,
    motion: recipe.motion ?? contract.looks.button!.motion,
    ...(recipe.body
      ? {
          paint: (scope: string) => ({
            source: assetRef(`${scope}/shader`),
            accent: glassAlpha(recipe.accent ?? palette.primary, reflections),
            glassTint: sheetTint,
            light: [light.x, light.y, light.height, light.intensity],
            finish: [
              material.ior,
              material.roughness,
              material.thickness,
              material.edge,
            ],
            lighting: sheetLighting,
            body: recipe.body,
          }),
          animation: !options.reducedMotion
            ? options.lightAnimation
            : undefined,
        }
      : {}),
  });
  for (const [name, recipe] of Object.entries(basic))
    definition[name] = finish({
      ...recipe,
      body: glassBody(name as keyof typeof basic),
    });
  for (const [name, recipe] of Object.entries(themes))
    definition[`kit/${name}`] = finish(recipe);
  for (const [name, recipe] of Object.entries(controls))
    definition[name] = finish(recipe);
  return {
    tokens: contract.tokens,
    themes: definition,
    controls: {
      button: "button",
      checkbox: "checkbox",
      switch: "switch",
      textInput: "textInput",
      slider: "slider",
      dial: "dial",
      scroll: "scroll",
      color: "color",
    },
    kit: Object.fromEntries(
      Object.keys(themes).map((name) => [name, `kit/${name}`]),
    ) as Record<KitThemeName, string>,
    assetIds: ["shader", "charts-paint", "charts-mesh"],
    assets:
      options.appearance !== "optical"
        ? options.sceneLighting
          ? liquidSceneAssets
          : liquidAuthoredAssets
        : options.sceneLighting
          ? sceneAssets
          : authoredAssets,
    plots: (scope) => {
      const common = {
        material,
        sceneLighting: options.sceneLighting ?? false,
      };
      const paint = glassChartProperties(
        { ...common, light: [light.x, light.y, light.height, light.intensity] },
        false,
      );
      const mesh = glassChartProperties(
        {
          ...common,
          lightSpace: "chart",
          light: [
            (light.x - 0.5) * 6,
            light.height,
            (light.y - 0.5) * 6,
            light.intensity,
          ],
        },
        true,
      );
      const colours = {
        red: palette.text[0],
        green: palette.text[1],
        blue: palette.text[2],
        alpha: 1,
        grid_red: palette.neutral[0],
        grid_green: palette.neutral[1],
        grid_blue: palette.neutral[2],
        grid_alpha: 0.24,
      };
      return {
        paint: {
          ...paint,
          tint: palette.secondary,
          source: assetRef(`${scope}/charts-paint`),
        },
        material: {
          ...mesh,
          tint: palette.secondary,
          source: assetRef(`${scope}/charts-mesh`),
          receives_light: true,
          alpha_mode: 2,
          conservative_bounds: true,
        },
        frame2d: {
          ...colours,
          highlight_red: palette.primary[0],
          highlight_green: palette.primary[1],
          highlight_blue: palette.primary[2],
          highlight_alpha: 1,
        },
        frame3d: { ...colours, mark_outlines: false },
        callout: {
          padding_x: 0.7,
          padding_y: 0.45,
          radius: 0.4,
          text_red: palette.text[0],
          text_green: palette.text[1],
          text_blue: palette.text[2],
          leader_red: 0.38,
          leader_green: 0.5,
          leader_blue: 0.56,
          leader_alpha: 0.6,
          leader_width: 0.035,
        },
        calloutPaint: {
          source: assetRef(`${scope}/shader`),
          accent: palette.primary,
          glassTint: sheetTint,
          light: [light.x, light.y, light.height, light.intensity],
          finish: [
            material.ior,
            Math.max(material.roughness, 0.34),
            material.thickness * 2.2,
            material.edge,
          ],
          lighting: sheetLighting,
          body: [4, 0.8, 1, 0],
        },
        legend: {
          textColor: palette.text,
          paint: {
            source: assetRef(`${scope}/shader`),
            accent: palette.primary,
            glassTint: sheetTint,
            light: [light.x, light.y, light.height, light.intensity],
            finish: [
              material.ior,
              Math.max(material.roughness, 0.3),
              material.thickness * 1.8,
              material.edge,
            ],
            lighting: sheetLighting,
            body: [4, 1, 1, 0],
          },
          swatchPaint: { ...paint, source: assetRef(`${scope}/charts-paint`) },
        },
      };
    },
  };
}

export interface GlassGuiKitProps
  extends Omit<GlassSkinProps, "contract" | "declarations"> {
  readonly contract: GlassKitContract;
}

export function GlassGuiKit({
  id = "ipp-glass",
  fontSize = 16,
  ...props
}: GlassGuiKitProps) {
  const skin = useMemo(
    () => glassSkin(props.contract, props),
    [
      props.contract,
      props.appearance,
      props.tokens,
      props.palette,
      props.light,
      props.material,
      props.panelOpacity,
      props.reflectionStrength,
      props.sceneLighting,
      props.lightAnimation,
      props.reducedMotion,
    ],
  );
  return (
    <GlassSkin {...props} id={id} fontSize={fontSize} declarations={false}>
      <GuiKit
        id={id}
        skin={skin}
        contract={props.contract}
        font={props.font}
        fontSize={fontSize}
        {...(props.reducedMotion === undefined
          ? {}
          : { reducedMotion: props.reducedMotion })}
      >
        {props.children}
      </GuiKit>
    </GlassSkin>
  );
}
