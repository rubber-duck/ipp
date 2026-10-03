/** Deterministic authoring and assertions, independent of process launch and wire layout. */
import assert from "node:assert/strict";
import type {
  AnimationWorldClient,
  Client,
  Command,
  DatasetValue,
  DynamicValue,
  HostClientBase,
  RowPropertyValue,
  RootBinding,
} from "@ipp/client";
import { canvasOutput } from "@ipp/client";
import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type * as Generated from "@ipp/host-contract";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../examples/world-gallery/worlds/charts/shared/commands.js";

export type ChartContract = Pick<
  typeof Generated,
  | "ExpressionBuilder"
  | "encodeRowsTable"
  | "encodeDataWindows"
  | "encodeAnimationClip"
>;
export interface Workload {
  name: string;
  rows: number;
  bindings: number;
  labels: number;
  component:
    | "PlotLine2d"
    | "PlotBars2d"
    | "PlotPoints3d"
    | "PlotGridBars3d"
    | "PlotHeightSurface3d";
  stream?: boolean;
  grid?: number;
  /** Full-size bindings plus a constant 1k-row chart; data scale never enlarges GPU paint. */
  dataOnly?: boolean;
}
export const SCHEMA = ["x", "y", "z"].map((name) => ({
  name,
  kind: "f32" as const,
}));
const CYAN = [0, 0.8, 1, 1] as const;
const handle = (id: bigint) => ({ kind: "handle" as const, id });

/** No random seeds or clocks determine row values or identity. */
export function sample(
  index: number,
  count: number,
  grid?: number,
): DatasetValue[] {
  const x = grid ? (index % grid) / (grid - 1) : index / Math.max(1, count - 1);
  const z = grid
    ? Math.floor(index / grid) / (grid - 1)
    : ((index * 17) % 101) / 100;
  return [x, 0.5 + 0.3 * Math.sin(x * 16 + z * 3), z].map((value) => ({
    kind: "f32",
    value: Math.fround(value),
  }));
}

export function workloads(preset: "smoke" | "full"): Workload[] {
  const dataScales = preset === "full" ? [1000, 10000, 100000] : [1000];
  const markScales = preset === "full" ? [1000, 10000] : [1000];
  return [
    ...dataScales.flatMap((rows) =>
      [1, 4].map((bindings) => ({
        name: `data-${rows}-shared-${bindings}`,
        rows,
        bindings,
        labels: 0,
        component: "PlotLine2d" as const,
        dataOnly: true,
      })),
    ),
    ...markScales.flatMap((rows) =>
      ["PlotBars2d", "PlotPoints3d", "PlotGridBars3d"].map((component) => ({
        name: `${component}-${rows}`,
        rows,
        bindings: 1,
        labels: 4,
        component: component as Workload["component"],
      })),
    ),
    ...[10, ...(preset === "full" ? [32] : [])].map((grid) => ({
      name: `surface-${grid}x${grid}`,
      rows: grid * grid,
      grid,
      bindings: 1,
      labels: 4,
      component: "PlotHeightSurface3d" as const,
    })),
    ...[4, 32, ...(preset === "full" ? [128] : [])].map((labels) => ({
      name: `labels-${labels}`,
      rows: 1000,
      bindings: 1,
      labels,
      component: "PlotPoints3d" as const,
    })),
    {
      name: "rolling-1000",
      rows: 1000,
      bindings: 1,
      labels: 0,
      stream: true,
      component: "PlotLine2d",
    },
  ];
}

export async function openWorkload(
  host: HostClientBase<Client>,
  contract: ChartContract,
  font: Uint8Array<ArrayBuffer>,
  workload: Workload,
  nonce: string,
) {
  assert.ok(
    workload.rows <= 100000 && workload.bindings <= 4 && workload.labels <= 128,
  );
  const spatial = workload.component.endsWith("3d");
  const world = await host.createWorld({
    symbolicId: `chart-data/${nonce}/${workload.name}`,
    temporary: true,
    ...(spatial
      ? {}
      : { canvas: { extent: [640, 400] as const, unitsPerMetre: 96 } }),
    selectedSystems: [
      ...(spatial
        ? ["ipp.hierarchy", "ipp.look-at", "ipp.final-propagation"]
        : []),
      "ipp.animation",
      "ipp.asset-dependencies",
      "ipp.data-bindings",
      "ipp.plot",
      ...(spatial
        ? ["ipp.geometry", "ipp.camera", "ipp.render"]
        : ["ipp.canvas"]),
    ],
  });
  const client = (await host.openWorld(
    world.reference,
  )) as AnimationWorldClient;
  const source = `datasets://chart-data/${nonce}/${workload.name}`;
  const producer = await host.datasets.create(
    source,
    workload.stream ? "streaming" : "buffer",
    SCHEMA,
  );
  const entities: bigint[] = [];
  let visualProducer:
    | Awaited<ReturnType<typeof host.datasets.create>>
    | undefined;
  const bindingComponent = workload.stream
    ? "StreamingDataSourceBinding"
    : "BufferDataSourceBinding";
  let camera = 0n;
  let controller = 0n;
  const property = (
    entity: bigint,
    name: string,
    value: DynamicValue,
  ): Command => ({
    kind: "setDynamicProperty",
    entity: handle(entity),
    component: client.components[bindingComponent]!.id,
    name,
    value,
  });
  const set = async (
    entity: bigint,
    component: string,
    values: Parameters<typeof componentFields>[2],
  ) => {
    successfulBatch(
      await client.batch(
        componentFields(client, component, values).map((field) => ({
          kind: "setField" as const,
          entity: handle(entity),
          component: client.components[component]!.id,
          field,
        })),
      ),
    );
  };
  try {
    let assetId = 1n;
    const upload = async (kind: number, bytes: Uint8Array<ArrayBuffer>) => {
      const asset = clientAssetSource(client.session, kind, assetId++);
      await client.registerAsset(asset, bytes.buffer);
      return asset.source;
    };
    const fontSource = await upload(17, font);
    const expressions: Record<string, string> = {};
    for (const output of ["x", "y", "z"]) {
      const builder = new contract.ExpressionBuilder();
      const input = builder.input(`column:${output}`, "f32");
      const expression =
        output === "y"
          ? builder.binary("multiply", input, builder.input("parameter", "f32"))
          : input;
      expressions[output] = await upload(19, builder.encode(expression));
    }
    const rowBytes = (
      field: string,
      values: Record<string, RowPropertyValue>[],
    ) => {
      const layout =
        client.components[workload.component]!.fields[field]!.rows!;
      return contract.encodeRowsTable(layout, {
        nextSlot: values.length,
        rows: new Map(values.map((value, i) => [i, value])),
      });
    };
    const visualSource = `${source}/visual`;
    if (workload.dataOnly) {
      visualProducer = await host.datasets.create(
        visualSource,
        "buffer",
        SCHEMA,
      );
      assert.equal(
        (
          await host.datasets.update(visualProducer, [
            {
              operation: "append",
              rows: Array.from({ length: 1000 }, (_, i) => sample(i, 1000)),
            },
          ])
        ).failure,
        undefined,
      );
    }
    for (
      let index = 0;
      index < workload.bindings + (workload.dataOnly ? 1 : 0);
      index++
    ) {
      const visual = workload.dataOnly && index === workload.bindings;
      const ref = { kind: "alias" as const, alias: 1 };
      const commands: Command[] = [
        createEntity(1, `binding-${index}`),
        insertComponent(client, bindingComponent, ref, {
          source: visual ? visualSource : source,
          ...(workload.stream
            ? {
                windows: contract.encodeDataWindows([
                  { kind: "count", count: BigInt(workload.rows) },
                ]),
              }
            : {}),
        }),
        insertComponent(
          client,
          spatial ? "Transform" : "CanvasStyle",
          ref,
          spatial ? {} : { x: 8, y: 8 },
        ),
        insertComponent(client, spatial ? "PlotFrame3d" : "PlotFrame2d", ref, {
          width: spatial ? 10 : 624,
          height: spatial ? 5 : 384,
          ...(spatial
            ? { depth: 10, min_z: 0, max_z: 1, automatic_z: false }
            : {}),
          min_x: 0,
          max_x: 1,
          min_y: 0,
          max_y: 2,
          automatic_x: false,
          automatic_y: false,
          ticks: 4,
          source: fontSource,
          font_size: spatial ? 0.22 : 12,
          x_title: "SOURCE X",
          y_title: "COMPUTED Y",
          ...(spatial ? { z_title: "SOURCE Z" } : {}),
        }),
        (() => {
          const command = insertComponent(client, workload.component, ref, {
            series: rowBytes("series", [
              {
                name: "shared",
                x: "x",
                y: "y",
                z: "z",
                value: "y",
                radius: "",
                height: "",
                color_column: "",
                color: CYAN,
                visible: true,
              },
            ]),
            labels: rowBytes(
              "labels",
              Array.from({ length: workload.labels }, (_, i) => ({
                series: 0,
                row_id: String(
                  1 +
                    Math.floor(
                      (i * (workload.rows - 1)) /
                        Math.max(1, workload.labels - 1),
                    ),
                ),
                text: `ROW ${i}`,
                offset: spatial ? [0.5, -0.5] : [20, -20],
                highlighted: i === 0,
                connector: true,
              })),
            ),
            ...(workload.component === "PlotLine2d" ? { marker_size: 0 } : {}),
            ...(workload.component === "PlotGridBars3d"
              ? { bar_width: 0.04, bar_depth: 0.04 }
              : {}),
          });
          assert.equal(command.kind, "insertComponent");
          if (command.kind !== "insertComponent")
            throw new Error("Expected insert");
          return {
            ...command,
            fields: command.fields.map((field) =>
              field.value.kind === "bytes" &&
              Object.values(client.components[workload.component]!.fields).some(
                (descriptor) =>
                  descriptor.offset === field.offset && descriptor.rows,
              )
                ? {
                    ...field,
                    value: { kind: "rows" as const, value: field.value.value },
                  }
                : field,
            ),
          };
        })(),
      ];
      if (workload.dataOnly && !visual) commands.splice(2);
      const entity = aliasId(await client.batch(commands), 1);
      if (!visual) entities.push(entity);
      successfulBatch(
        await client.batch([
          ...Object.entries(expressions).map(([name, source]) =>
            property(entity, name, {
              kind: "asset",
              value: { kind: 19, source },
            }),
          ),
          property(entity, "y_parameter", {
            kind: "f32",
            value: visual ? 1 : index + 1,
          }),
        ]),
      );
    }
    for (let offset = 0; offset < workload.rows; offset += 1024) {
      const outcome = await host.datasets.update(producer, [
        {
          operation: "append",
          rows: Array.from(
            { length: Math.min(1024, workload.rows - offset) },
            (_, i) => sample(offset + i, workload.rows, workload.grid),
          ),
        },
      ]);
      assert.equal(outcome.failure, undefined);
    }
    if (spatial) {
      camera = aliasId(
        await client.batch([
          createEntity(1, "camera"),
          insertComponent(
            client,
            "Transform",
            { kind: "alias", alias: 1 },
            cameraPose(false),
          ),
          insertComponent(
            client,
            "Camera",
            { kind: "alias", alias: 1 },
            { projection: 1, ortho_height: 17, near: 0.1, far: 100 },
          ),
        ]),
        1,
      );
    }
    const output = spatial
      ? await host.bindOutput(world.reference, camera, "camera")
      : canvasOutput(world.reference);
    const binding: RootBinding = await host.setRootOutput(output, {
      width: 640,
      height: 400,
      devicePixelRatio: 1,
    });
    const clip = await upload(
      10,
      contract.encodeAnimationClip({
        duration: 1,
        tracks: [
          {
            property: {
              component: client.components[bindingComponent]!.id,
              name: "y_parameter",
            },
            keys: [
              {
                time: 0,
                value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
                interpolation: { kind: "linear" },
              },
              {
                time: 1,
                value: { kind: "dynamic", value: { kind: "f32", value: 0.5 } },
              },
            ],
          },
        ],
      }),
    );
    controller = await client.createAnimationController({
      speed: 0,
      drivers: [
        {
          source: clip,
          track: 0,
          target: entities[0]!,
          property: {
            component: client.components[bindingComponent]!.id,
            name: "y_parameter",
          },
        },
      ],
    });
    await client.controlAnimationController(controller, { action: "play" });
    await client.controlAnimationController(controller, { action: "pause" });
    let appended = 0;
    let parameter = 1;
    let contribution = 0;
    return {
      binding,
      source,
      entities,
      spatial,
      async action(mode: string, cycle: number) {
        if (mode === "edit") {
          const edit = sample(cycle + 1, workload.rows, workload.grid);
          // Height surfaces keep the complete x/z lattice; edits change height only.
          if (workload.grid) {
            edit[0] = { kind: "f32", value: 0 };
            edit[2] = { kind: "f32", value: 0 };
          }
          const outcome = await host.datasets.update(
            producer,
            workload.stream
              ? [
                  {
                    operation: "append",
                    rows: Array.from({ length: 64 }, (_, i) =>
                      sample(workload.rows + appended + i, workload.rows),
                    ),
                  },
                ]
              : [
                  {
                    operation: "edit",
                    row: 1n,
                    values: edit,
                  },
                ],
          );
          assert.equal(outcome.failure, undefined);
          if (workload.stream) appended += 64;
        } else if (mode === "parameter") {
          parameter = cycle % 2 ? 1 : 2;
          successfulBatch(
            await client.batch([
              property(entities[0]!, "y_parameter", {
                kind: "f32",
                value: cycle % 2 ? 1 : 2,
              }),
            ]),
          );
        } else if (mode === "animation") {
          contribution = (cycle % 2 ? 0.25 : 0.75) * 0.5;
          await client.controlAnimationController(controller, {
            action: "seek",
            time: cycle % 2 ? 0.25 : 0.75,
          });
        } else if (mode === "camera") {
          await set(camera, "Transform", cameraPose(cycle % 2 === 0));
        }
      },
      async verify() {
        const page = await host.datasets.read(source, { limit: 1 });
        assert.equal(page.memory.retainedRows, BigInt(workload.rows));
        assert.ok(page.memory.allocatedBytes >= page.memory.retainedBytes);
        if (workload.stream)
          assert.equal(page.rows[0]!.id, BigInt(appended + 1));
        for (const [index, entity] of entities.entries()) {
          const projection = await host.datasets.bindingView(
            client.session,
            entity,
            { limit: 1 },
          );
          assert.equal(projection.availability.reason, "Ready");
          assert.equal(projection.totalRows, BigInt(workload.rows));
          assert.equal(projection.sourceIncarnation, producer.incarnation);
          assert.equal(projection.rows[0]!.id, page.rows[0]!.id);
          const y = projection.columns.findIndex(
            (column) => column.name === "y",
          );
          const actual = projection.rows[0]!.values[y]!;
          assert.ok(actual.valid && actual.value.kind === "f32");
          assert.equal(
            actual.value.value,
            Math.fround(
              (page.rows[0]!.values[1] as { value: number }).value *
                (index === 0 ? parameter + contribution : index + 1),
            ),
          );
        }
        return page.memory;
      },
      async close() {
        await client.close();
        await host.destroyWorld(world.reference);
        await host.datasets.destroy(producer);
        if (visualProducer) await host.datasets.destroy(visualProducer);
      },
    };
  } catch (error) {
    await client.close().catch(() => {});
    await host.destroyWorld(world.reference).catch(() => {});
    await host.datasets.destroy(producer).catch(() => {});
    if (visualProducer)
      await host.datasets.destroy(visualProducer).catch(() => {});
    throw error;
  }
}

function cameraPose(opposite: boolean): Record<string, number> {
  const eye = opposite ? [-10, 10, 14] : [10, 8, 14];
  const dx = eye[0]! - 5,
    dy = eye[1]! - 2,
    dz = eye[2]! - 5;
  const yaw = Math.atan2(dx, dz),
    pitch = -Math.atan2(dy, Math.hypot(dx, dz));
  const sy = Math.sin(yaw / 2),
    cy = Math.cos(yaw / 2),
    sx = Math.sin(pitch / 2),
    cx = Math.cos(pitch / 2);
  return {
    x: eye[0]!,
    y: eye[1]!,
    z: eye[2]!,
    qx: cy * sx,
    qy: sy * cx,
    qz: -sy * sx,
    qw: cy * cx,
  };
}
