/** Native chart study: compose actual Camera outputs into a reusable Canvas sheet. */
import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type RootBinding,
} from "@ipp/client";
import {
  Asset,
  AttachedWorld,
  Entity,
  Surface,
  assetRef,
  createRoot,
  type ReactWorldRoot,
} from "@ipp/react";
import { Box, Style, Text } from "@ipp/react/gui";
import { componentFields, successfulBatch } from "../charts/shared/commands.js";
import type { Plot3dScene } from "./content.js";
import type * as Generated from "@ipp/host-contract";

const EXTENT = { width: 1536, height: 1024, devicePixelRatio: 1 } as const;
export interface Plot3dSheet {
  readonly world: Awaited<ReturnType<HostClientBase<Client>["createWorld"]>>;
  readonly client: Client;
  readonly root: ReactWorldRoot;
  readonly binding: RootBinding;
}

export async function openPlot3dSheet(
  scene: Plot3dScene,
  tokens: typeof Generated.GUI_SKIN_TOKENS,
  font: Uint8Array<ArrayBuffer>,
  name: string,
): Promise<Plot3dSheet> {
  const host = scene.host;
  const world = await host.createWorld({
    symbolicId: `chart-sheet/${name}`,
    temporary: true,
    canvas: { extent: [1536, 1024], unitsPerMetre: 96 },
    selectedSystems: [
      "ipp.animation",
      "ipp.asset-dependencies",
      "ipp.canvas",
      "ipp.world-attachment",
      "ipp.hierarchy",
      "ipp.look-at",
      "ipp.final-propagation",
      "ipp.geometry",
      "ipp.surface",
    ],
  });
  const client = await host.openWorld(world.reference);
  const root = createRoot(client, { host });
  const color = (value: readonly number[]) => ({
    red: value[0],
    green: value[1],
    blue: value[2],
    alpha: value[3],
  });
  const labels = [
    [
      "grid-bars",
      "01 / GRID BARS",
      "Shared source / zero baseline / authored highlight",
    ],
    [
      "height-surface",
      "02 / HEIGHT SURFACE",
      "Triangulated samples / an explicit missing-data hole",
    ],
    [
      "point-plot",
      "03 / DISCONNECTED POINTS",
      "Two evaluated series / independent binding on the bar source",
    ],
    [
      "variable-pie",
      "04 / VARIABLE PIE",
      "Share determines angle / radius and height are independent",
    ],
  ] as const;
  try {
    for (const [name] of labels) {
      const chart = scene.charts.find((item) => item.name === name)!;
      await host.clearRootOutput(chart.binding);
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, "PlotFrame3d", { font_size: 0.38 }).map(
            (field) => ({
              kind: "setField",
              entity: { kind: "handle", id: chart.entity },
              component: chart.client.components.PlotFrame3d!.id,
              field,
            }),
          ),
        ),
      );
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, "Camera", { ortho_height: 12.5 }).map(
            (field) => ({
              kind: "setField",
              entity: { kind: "handle", id: chart.camera },
              component: chart.client.components.Camera!.id,
              field,
            }),
          ),
        ),
      );
    }
    await root.render(
      <>
        <Asset id="font" kind={17} data={font} encode={(bytes) => bytes} />
        <Entity id="page">
          <Style {...color(tokens.page)} />
          <Box width={1536} height={1024} />
        </Entity>
        <Entity id="heading">
          <Style x={30} y={24} {...color(tokens.accent)} />
          <Text
            source={assetRef("font")}
            text="IPP / 3D CHARTS"
            font_size={34}
          />
        </Entity>
        <Entity id="intro">
          <Style x={30} y={72} {...color(tokens.text)} />
          <Text
            source={assetRef("font")}
            text="Spatial data / shared axes / source-row labels"
            font_size={17}
          />
        </Entity>
        {labels.map(([key, title, subtitle], index) => {
          const chart = scene.charts.find((item) => item.name === key)!;
          const x = 24 + (index % 2) * 760;
          const y = 115 + Math.floor(index / 2) * 438;
          return (
            <Entity key={key} id={`panel/${key}`}>
              <Style x={x} y={y} {...color(tokens.surface)} />
              <Box width={744} height={420} />
              <Entity id={`title/${key}`}>
                <Style x={x + 18} y={y + 12} {...color(tokens.accent)} />
                <Text source={assetRef("font")} text={title} font_size={21} />
              </Entity>
              <Entity id={`subtitle/${key}`}>
                <Style x={x + 18} y={y + 44} {...color(tokens.text)} />
                <Text
                  source={assetRef("font")}
                  text={subtitle}
                  font_size={13}
                />
              </Entity>
              <Entity id={`view/${key}`}>
                <Style x={x + 6} y={y + 66} />
                <Surface width={732 / 96} height={348 / 96} />
                <AttachedWorld
                  anchor={`view/${key}`}
                  child={{ borrow: chart.world.reference }}
                  attachment={{
                    mode: "surface-camera",
                    output: chart.binding
                      .output as import("@ipp/client").CameraOutputReference,
                  }}
                />
              </Entity>
            </Entity>
          );
        })}
        <Entity id="footer">
          <Style x={30} y={1000} {...color(tokens.text)} />
          <Text
            source={assetRef("font")}
            text="SHARED SOURCE / TYPED REACT AUTHORING"
            font_size={12}
          />
        </Entity>
      </>,
    );
    const binding = await host.setRootOutput(
      canvasOutput(world.reference),
      EXTENT,
    );
    return { world, client, root, binding };
  } catch (error) {
    await root.render(null).catch(() => {});
    await root.unmount().catch(() => {});
    await client.close().catch(() => {});
    await host.destroyWorld(world.reference).catch(() => {});
    throw error;
  }
}

export async function closePlot3dSheet(
  sheet: Plot3dSheet,
  host: HostClientBase<Client>,
) {
  await sheet.root.render(null);
  await sheet.root.unmount();
  await sheet.client.close();
  await host.destroyWorld(sheet.world.reference);
}
