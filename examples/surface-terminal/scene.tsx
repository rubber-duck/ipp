/** A scene declaration; any Host may supply its generated client and immutable assets. */
import type { ReactNode } from "react";
import type { ClientAssetSource } from "@ipp/client";
import {
  CanvasWorld,
  Children,
  Entity,
  Surface,
  SurfaceCache,
  Transform,
  type CanvasWorldHandle,
  type SurfaceCacheProps,
} from "@ipp/react";
import { Drawing, Image, Style, Text } from "@ipp/react/gui";

export interface TerminalAssets {
  font: ClientAssetSource;
  panel: ClientAssetSource;
  icon: ClientAssetSource;
  bitmap: ClientAssetSource;
}

export const TERMINAL_TEXT =
  "IPP  /  vector terminal\n\n$ render --surface\nQuadratic curves on the GPU\n0123456789  []{}() <> /\\\n\nReady at any viewing distance.";

const TERMINAL_SYSTEMS = [
  "ipp.animation",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

export interface TerminalLayerOptions {
  text?: string;
  textFontSize?: number;
  textPosition?: readonly [number, number];
  textColor?: readonly [number, number, number];
  font?: ClientAssetSource;
  panel?: ClientAssetSource;
  icon?: ClientAssetSource;
  bitmap?: ClientAssetSource;
  backgroundColor?: readonly [number, number, number, number];
  cursorColor?: readonly [number, number, number, number];
  cursorOpacity?: number;
  cursorPosition?: readonly [number, number];
  cursorScale?: readonly [number, number];
}

function drawingLayer(
  id: string,
  source: ClientAssetSource,
  position: readonly [number, number],
  scale: readonly [number, number],
  color: readonly [number, number, number, number] = [1, 1, 1, 1],
  opacity = 1,
) {
  return (
    <Entity key={id} id={id}>
      <Style
        x={position[0]}
        y={position[1]}
        scale_x={scale[0]}
        scale_y={scale[1]}
        red={color[0]}
        green={color[1]}
        blue={color[2]}
        alpha={color[3]}
        opacity={opacity}
      />
      <Drawing source={source.source} />
    </Entity>
  );
}

export function terminalLayers(
  assets: TerminalAssets,
  options: TerminalLayerOptions = {},
): ReactNode[] {
  const font = options.font ?? assets.font;
  const panel = options.panel ?? assets.panel;
  const [backgroundRed, backgroundGreen, backgroundBlue, backgroundAlpha] =
    options.backgroundColor ?? [0.015, 0.025, 0.05, 1];
  const [cursorRed, cursorGreen, cursorBlue, cursorAlpha] =
    options.cursorColor ?? [0.3, 1, 0.5, 1];

  return [
    drawingLayer(
      "background",
      panel,
      [1.9, 1.2],
      [3.8, 2.4],
      [backgroundRed, backgroundGreen, backgroundBlue, backgroundAlpha],
    ),
    drawingLayer(
      "highlight",
      panel,
      [1.9, 0.25],
      [3.8, 0.3],
      [0.04, 0.12, 0.22, 1],
    ),
    <Entity key="text" id="text">
      <Style
        x={options.textPosition?.[0] ?? 0.18}
        y={options.textPosition?.[1] ?? 0.28}
        red={options.textColor?.[0] ?? 0.65}
        green={options.textColor?.[1] ?? 0.92}
        blue={options.textColor?.[2] ?? 0.8}
      />
      <Text
        text={options.text ?? TERMINAL_TEXT}
        source={font.source}
        font_size={options.textFontSize ?? 0.14}
      />
    </Entity>,
    drawingLayer(
      "cursor",
      panel,
      options.cursorPosition ?? [0.25, 1.85],
      options.cursorScale ?? [0.075, 0.15],
      [cursorRed, cursorGreen, cursorBlue, cursorAlpha],
      options.cursorOpacity ?? 1,
    ),
    drawingLayer(
      "icon",
      options.icon ?? assets.icon,
      [3.12, 1.82],
      [0.006, 0.006],
    ),
    <Entity key="bitmap" id="bitmap">
      <Style x={3.21} y={0.61} />
      <Image
        source={(options.bitmap ?? assets.bitmap).source}
        width={0.28}
        height={0.28}
      />
    </Entity>,
  ];
}

export function Terminal({
  assets,
  id = "surface-terminal",
  worldId = `${id}-content`,
  x = 0,
  z = 0,
  angle = 0,
  cache,
  onWorld,
  children,
}: {
  assets: TerminalAssets;
  id?: string;
  worldId?: string;
  x?: number;
  z?: number;
  angle?: number;
  cache?: SurfaceCacheProps | undefined;
  onWorld?: ((handle: CanvasWorldHandle) => void) | undefined;
  children?: ReactNode;
}) {
  return (
    <>
      <Entity id={id}>
        <Transform x={x} z={z} ry={angle} />
        <Surface width={3.8} height={2.4} />
        {cache ? <SurfaceCache {...cache} /> : null}
      </Entity>
      <CanvasWorld
        presentation={{ anchor: id }}
        create={{ symbolicId: worldId, selectedSystems: TERMINAL_SYSTEMS }}
        extent={[3.8, 2.4]}
        unitsPerMetre={1}
        {...(onWorld ? { onReady: onWorld } : {})}
      >
        <Entity id="canvas">
          <Children>
            {children === undefined ? terminalLayers(assets) : children}
          </Children>
        </Entity>
      </CanvasWorld>
    </>
  );
}
