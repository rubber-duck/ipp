/** A scene declaration; any Host may supply its generated client and immutable assets. */
import {
  Entity,
  Surface,
  SurfaceCache,
  Transform,
  type SurfaceCacheProps,
  type SurfaceItemProps,
} from "@ipp/react";
import type { ClientAssetSource } from "@ipp/client";

export interface TerminalAssets {
  font: ClientAssetSource;
  panel: ClientAssetSource;
  icon: ClientAssetSource;
  bitmap: ClientAssetSource;
}

export const TERMINAL_TEXT =
  "IPP  /  vector terminal\n\n$ render --surface\nQuadratic curves on the GPU\n0123456789  []{}() <> /\\\n\nReady at any viewing distance.";

export function terminalItems(
  assets: TerminalAssets,
  text = TERMINAL_TEXT,
): SurfaceItemProps[] {
  return [
    {
      key: "background",
      content: { kind: "drawing" },
      asset: assets.panel,
      position: [1.9, 1.2],
      scale: [3.8, 2.4],
      color: [0.015, 0.025, 0.05, 1],
    },
    {
      key: "highlight",
      content: { kind: "drawing" },
      asset: assets.panel,
      position: [1.9, 0.25],
      scale: [3.8, 0.3],
      color: [0.04, 0.12, 0.22, 1],
    },
    {
      key: "text",
      content: { kind: "label", text },
      asset: assets.font,
      position: [0.18, 0.28],
      fontSize: 0.14,
      color: [0.65, 0.92, 0.8, 1],
    },
    {
      key: "cursor",
      content: { kind: "drawing" },
      asset: assets.panel,
      position: [0.25, 1.85],
      scale: [0.075, 0.15],
      color: [0.3, 1, 0.5, 1],
    },
    {
      key: "icon",
      content: { kind: "drawing" },
      asset: assets.icon,
      position: [3.12, 1.82],
      scale: [0.006, 0.006],
    },
    {
      key: "bitmap",
      content: { kind: "bitmap", size: [0.28, 0.28] },
      asset: assets.bitmap,
      position: [3.21, 0.61],
    },
  ];
}

export function Terminal({
  assets,
  text,
  angle = 0,
  items,
  cache,
}: {
  assets: TerminalAssets;
  text?: string;
  angle?: number;
  items?: readonly SurfaceItemProps[];
  /** Opt the terminal into distance-based texture caching; direct when absent. */
  cache?: SurfaceCacheProps | undefined;
}) {
  return (
    <Entity id="surface-terminal">
      <Transform bound={false} ry={angle} />
      <Surface
        bound={false}
        width={3.8}
        height={2.4}
        items={items ?? terminalItems(assets, text)}
      />
      {cache ? <SurfaceCache bound={false} {...cache} /> : null}
    </Entity>
  );
}
