/** Dense data through the real System monitor; no clock or renderer overrides. */
import type {
  Client,
  DatasetProducer,
  Inspection,
  WorldReference,
} from "@ipp/client";
import { componentFields, successfulBatch } from "../../fixtures/commands.js";
import { settle } from "./application.js";

let fixture:
  | {
      client: Client;
      entity: bigint;
      producer: DatasetProducer;
      originalSource: string;
      next: number;
    }
  | undefined;

async function source(client: Client, entity: bigint, value: string) {
  successfulBatch(
    await client.batch(
      componentFields(client, "StreamingDataSourceBinding", {
        source: value,
      }).map((field) => ({
        kind: "setField" as const,
        entity: { kind: "handle" as const, id: entity },
        component: client.components.StreamingDataSourceBinding!.id,
        field,
      })),
    ),
  );
  await settle();
}

export async function open() {
  if (fixture) throw new Error("Dense FPS fixture is already open");
  const app = window.ippDemoApplication!;
  const canvas = window.ippDemoCanvas!;
  const snapshot = (await app.inspect()) as {
    panels: (Inspection & { world: WorldReference })[];
  };
  const client = await canvas.host.openWorld(snapshot.panels[1]!.world);
  let producer: DatasetProducer | undefined;
  try {
    const chart = (await client.inspect()).entities.find(
      (entity) => entity.metadata.symbolicId === "cockpit-fps-chart",
    );
    if (!chart) throw new Error("System monitor chart is missing");
    const originalSource = chart.components.find(
      (component) =>
        component.component ===
        client.components.StreamingDataSourceBinding!.id,
    )!.fields.source as string;
    const name = `datasets://dense-fps-test/${client.session}`;
    producer = await canvas.host.datasets.create(name, "streaming", [
      { name: "seconds", kind: "f32" },
      { name: "fps", kind: "f32" },
      { name: "frame_ms", kind: "f32" },
    ]);
    fixture = { client, entity: chart.id, producer, originalSource, next: 0 };
    // Acquire the real binding's retention window before publishing history.
    await source(client, chart.id, name);
    return await append(3);
  } catch (error) {
    if (fixture) await close();
    else {
      if (producer) await canvas.host.datasets.destroy(producer);
      await client.close();
    }
    throw error;
  }
}

export async function append(count: number) {
  if (!fixture) throw new Error("Dense FPS fixture is not open");
  const current = fixture;
  const datasets = window.ippDemoCanvas!.host.datasets;
  const outcome = await datasets.update(current.producer, [
    {
      operation: "append",
      rows: Array.from({ length: count }, () => {
        const index = current.next++;
        const fps = 32 + 6 * Math.sin(index * 1.7) + 3 * Math.cos(index * 0.27);
        return [index / 10, fps, 1000 / fps].map((value) => ({
          kind: "f32" as const,
          value,
        }));
      }),
    },
  ]);
  if (outcome.failure) throw new Error(outcome.failure.reason);
  await settle();
  const page = await datasets.bindingView(
    current.client.session,
    current.entity,
  );
  const last = await datasets.bindingView(
    current.client.session,
    current.entity,
    {
      offset: page.totalRows - 1n,
      limit: 1,
    },
  );
  return {
    rows: Number(page.totalRows),
    availability: page.availability.reason,
    columns: page.columns.map((column) => column.name),
    first: page.rows[0],
    last: last.rows[0],
  };
}

export async function close() {
  const current = fixture;
  fixture = undefined;
  if (!current) return;
  try {
    await source(current.client, current.entity, current.originalSource);
  } finally {
    try {
      await window.ippDemoCanvas!.host.datasets.destroy(current.producer);
    } finally {
      await current.client.close();
    }
  }
}
