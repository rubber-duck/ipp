/** Canvas lifecycle assertions shared by real native and browser transports. */
import type {
  AnimationWorldClient,
  HostClientBase,
  RowsInput,
  RowsLayoutDescriptor,
} from "@ipp/client";
import type { TerminalAssets } from "../../examples/surface-terminal/scene.js";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "./camera-fixtures.js";

export type SurfaceTestClient = AnimationWorldClient;

export interface SurfaceRowsEncoder {
  encodeRowsTable<Row extends object>(
    layout: RowsLayoutDescriptor,
    rows: RowsInput<Row>,
  ): Uint8Array<ArrayBuffer>;
}

/** Cache policy the lifecycle scenario leaves authored for snapshot checks. */
export const SURFACE_CACHE_POLICY = {
  direct_distance: 0.5,
  texels_per_metre: 128,
  max_refresh_hz: 2,
} as const;

const CANVAS_SYSTEMS = [
  "ipp.animation",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

function expect(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/** Deterministic text for inspected state, including bigint and rows values. */
function stableText(value: unknown) {
  return JSON.stringify(value, (_, item) =>
    typeof item === "bigint"
      ? `${item}n`
      : item instanceof Map
        ? [...item.entries()]
        : item,
  );
}

export async function canvasSnapshot(
  client: SurfaceTestClient,
  entity: bigint,
) {
  const inspection = await client.inspect();
  const root = inspection.entities.find((item) => item.id === entity);
  expect(root, "Canvas root entity disappeared");
  // Extent and density are the Canvas System's World-level state.
  const canvas = (await client.inspectPage({ collection: "canvas" })).canvas;
  expect(canvas, "Canvas World omitted its Canvas System state");
  const names = new Map(
    Object.entries(client.components).flatMap(([name, descriptor]) =>
      descriptor ? [[descriptor.id, name] as const] : [],
    ),
  );
  const children = inspection.entities
    .filter((item) => item.link.parent === entity)
    .sort((left, right) =>
      left.link.order < right.link.order
        ? -1
        : left.link.order > right.link.order
          ? 1
          : 0,
    )
    .map((item) => ({
      symbolicId: item.metadata.symbolicId,
      components: Object.fromEntries(
        item.components.map((component) => [
          names.get(component.component) ?? `component-${component.component}`,
          {
            fields: component.fields,
            properties: component.properties ?? {},
          },
        ]),
      ),
    }));
  return { canvas: canvas.state, children };
}

/** Authored SurfaceCache fields on one entity, or undefined when direct. */
export async function surfaceCachePolicy(
  client: AnimationWorldClient,
  entity: bigint,
) {
  const descriptor = client.components.SurfaceCache;
  expect(descriptor, "Surface target omitted the SurfaceCache component");
  const snapshot = (await client.inspect()).entities.find(
    (item) => item.id === entity,
  );
  expect(snapshot, "Surface entity disappeared");
  const component = snapshot.components.find(
    (item) => item.component === descriptor.id,
  );
  if (!component) return undefined;
  const { direct_distance, texels_per_metre, max_refresh_hz } =
    component.fields;
  return { direct_distance, texels_per_metre, max_refresh_hz };
}

/**
 * Opt-in cache policy authoring through the generated client: Surfaces start
 * direct, invalid thresholds are rejected without changing the authored
 * policy, removal resets to direct presentation and reinsertion restores the
 * runtime defaults. Leaves `authored` in place for snapshot checks.
 */
export async function exerciseSurfaceCachePolicy(
  client: AnimationWorldClient,
  entity: bigint,
  authored: {
    direct_distance: number;
    texels_per_metre: number;
    max_refresh_hz: number;
  },
) {
  const descriptor = client.components.SurfaceCache;
  expect(descriptor, "Surface target omitted the SurfaceCache component");
  const ref = { kind: "handle", id: entity } as const;
  const same = (actual: unknown, expected: unknown) =>
    JSON.stringify(actual) === JSON.stringify(expected);
  expect(
    (await surfaceCachePolicy(client, entity)) === undefined,
    "Surfaces must present directly until opted in",
  );
  const initial = {
    direct_distance: 2,
    texels_per_metre: 256,
    max_refresh_hz: 10,
  };
  successfulBatch(
    await client.batch([insertComponent(client, "SurfaceCache", ref, initial)]),
  );
  expect(
    same(await surfaceCachePolicy(client, entity), initial),
    "SurfaceCache insertion did not author every field",
  );
  for (const [field, value] of [
    ["direct_distance", -1],
    ["texels_per_metre", 0],
    ["max_refresh_hz", 1e6],
  ] as const) {
    const outcome = await client.batch([
      {
        kind: "setField",
        entity: ref,
        component: descriptor.id,
        field: componentFields(client, "SurfaceCache", { [field]: value })[0]!,
      },
    ]);
    expect(
      !outcome.ok && outcome.error.reason === "InvalidValue",
      `SurfaceCache ${field}=${value} was not rejected as invalid`,
    );
    expect(
      same(await surfaceCachePolicy(client, entity), initial),
      `Rejected SurfaceCache ${field} changed the authored policy`,
    );
  }
  successfulBatch(
    await client.batch([
      { kind: "removeComponent", entity: ref, component: descriptor.id },
    ]),
  );
  expect(
    (await surfaceCachePolicy(client, entity)) === undefined,
    "Removing SurfaceCache did not reset to direct presentation",
  );
  successfulBatch(
    await client.batch([insertComponent(client, "SurfaceCache", ref)]),
  );
  expect(
    same(await surfaceCachePolicy(client, entity), {
      direct_distance: 4,
      texels_per_metre: 512,
      max_refresh_hz: 30,
    }),
    "Reinserted SurfaceCache did not use the runtime defaults",
  );
  successfulBatch(
    await client.batch(
      componentFields(client, "SurfaceCache", authored).map((field) => ({
        kind: "setField" as const,
        entity: ref,
        component: descriptor.id,
        field,
      })),
    ),
  );
  expect(
    same(await surfaceCachePolicy(client, entity), authored),
    "SurfaceCache field edits were not authored",
  );
}

export async function waitSurfaceAssets(
  client: SurfaceTestClient,
  sources: readonly string[],
) {
  for (;;) {
    const state = await client.inspect();
    const resources = state.resources.filter((item) =>
      sources.includes(item.source),
    );
    const failed = resources.find((item) => item.status === "failed");
    if (failed)
      throw new Error(
        `Canvas asset failed: ${JSON.stringify(failed, (_, value) => (typeof value === "bigint" ? String(value) : value))}`,
      );
    if (
      resources.length >= sources.length &&
      resources.every((item) => item.status === "loaded")
    )
      return;
    await client.waitForFrame(state.tick);
  }
}

export async function exerciseSurfaceLifecycle(
  host: HostClientBase<SurfaceTestClient>,
  client: SurfaceTestClient,
  assets: TerminalAssets,
  glyphId: number,
  encoder: SurfaceRowsEncoder,
) {
  const entity = aliasId(
    await client.batch([
      createEntity(1, "surface-api"),
      insertComponent(client, "Transform", { kind: "alias", alias: 1 }),
      insertComponent(
        client,
        "FlatSurface",
        { kind: "alias", alias: 1 },
        {
          width: 4,
          height: 3,
        },
      ),
    ]),
    1,
  );
  const created = await host.createWorld({
    symbolicId: "surface-api-canvas",
    selectedSystems: CANVAS_SYSTEMS,
    canvas: { extent: [4, 3], unitsPerMetre: 1 },
  });
  const canvasClient = await host.openWorld(created.reference);
  const rootRef = { kind: "alias", alias: 1 } as const;
  const backgroundRef = { kind: "alias", alias: 2 } as const;
  const textRef = { kind: "alias", alias: 3 } as const;
  const iconRef = { kind: "alias", alias: 4 } as const;
  const bitmapRef = { kind: "alias", alias: 5 } as const;
  const glyphRef = { kind: "alias", alias: 6 } as const;
  const cursorRef = { kind: "alias", alias: 7 } as const;
  const glyphDescriptor = canvasClient.components.CanvasGlyphRun;
  const glyphField = glyphDescriptor?.fields.glyphs;
  const glyphLayout = glyphField?.rows;
  expect(glyphLayout, "CanvasGlyphRun omitted its generated rows layout");
  const glyphs = encoder.encodeRowsTable(glyphLayout, {
    nextSlot: 1,
    rows: new Map([[0, { glyph_id: glyphId, position: [0.4, 0.2] }]]),
  });
  const canvasOutcome = successfulBatch(
    await canvasClient.batch([
      createEntity(1, "canvas"),
      createEntity(2, "surface-background"),
      insertComponent(canvasClient, "CanvasStyle", backgroundRef, {
        scale_x: 4,
        scale_y: 3,
        red: 0.015,
        green: 0.025,
        blue: 0.05,
      }),
      insertComponent(canvasClient, "CanvasDrawing", backgroundRef, {
        source: assets.panel.source,
      }),
      createEntity(3, "surface-text"),
      insertComponent(canvasClient, "CanvasStyle", textRef, {
        x: 0.18,
        y: 0.28,
        red: 0.65,
        green: 0.92,
        blue: 0.8,
      }),
      insertComponent(canvasClient, "CanvasText", textRef, {
        text: "A\nAV",
        source: assets.font.source,
        font_size: 0.14,
      }),
      createEntity(4, "surface-icon"),
      insertComponent(canvasClient, "CanvasStyle", iconRef, {
        x: 1.2,
        y: 1,
        scale_x: 0.006,
        scale_y: 0.006,
      }),
      insertComponent(canvasClient, "CanvasDrawing", iconRef, {
        source: assets.icon.source,
      }),
      createEntity(5, "surface-bitmap"),
      insertComponent(canvasClient, "CanvasStyle", bitmapRef, {
        x: 2.8,
        y: 0.6,
      }),
      insertComponent(canvasClient, "CanvasBitmap", bitmapRef, {
        source: assets.bitmap.source,
        width: 1,
        height: 1,
      }),
      createEntity(6, "surface-glyph-run"),
      insertComponent(canvasClient, "CanvasStyle", glyphRef, {
        x: 0.2,
        y: 0.6,
        red: 0.65,
        green: 0.92,
        blue: 0.8,
      }),
      {
        kind: "insertComponent",
        entity: glyphRef,
        component: glyphDescriptor!.id,
        fields: [
          ...componentFields(canvasClient, "CanvasGlyphRun", {
            source: assets.font.source,
            font_size: 0.14,
          }),
          {
            offset: glyphField!.offset,
            value: { kind: "rows", value: glyphs },
          },
        ],
      },
      createEntity(7, "surface-cursor"),
      insertComponent(canvasClient, "CanvasStyle", cursorRef, {
        x: 0.25,
        y: 1.85,
        scale_x: 0.075,
        scale_y: 0.15,
        red: 0.3,
        green: 1,
        blue: 0.5,
      }),
      insertComponent(canvasClient, "CanvasDrawing", cursorRef, {
        source: assets.panel.source,
      }),
      ...[backgroundRef, textRef, iconRef, bitmapRef, glyphRef, cursorRef].map(
        (child) => ({
          kind: "placeEntity" as const,
          entity: child,
          placement: { parent: rootRef, before: null },
        }),
      ),
    ]),
  );
  const canvasEntity = aliasId(canvasOutcome, 1);
  const attachment = client.components.WorldAttachment;
  expect(attachment, "Surface parent omitted WorldAttachment");
  successfulBatch(
    await client.batch([
      {
        kind: "insertComponent",
        entity: { kind: "handle", id: entity },
        component: attachment.id,
        fields: [
          {
            offset: attachment.fields.mode!.offset,
            value: { kind: "u32", value: 1 },
          },
          {
            offset: attachment.fields.child!.offset,
            value: { kind: "world", value: created.reference },
          },
        ],
      },
    ]),
  );
  await waitSurfaceAssets(canvasClient, [
    assets.font.source,
    assets.panel.source,
    assets.icon.source,
    assets.bitmap.source,
  ]);
  const initial = await canvasSnapshot(canvasClient, canvasEntity);
  expect(
    initial.children.map((item) => item.symbolicId).join(",") ===
      "surface-background,surface-text,surface-icon,surface-bitmap,surface-glyph-run,surface-cursor",
    "Canvas entity order or keyed children were not authored",
  );
  expect(
    initial.children[0]?.components.CanvasDrawing?.fields.source ===
      assets.panel.source &&
      initial.children[1]?.components.CanvasText?.fields.text === "A\nAV" &&
      initial.children[1]?.components.CanvasText?.fields.source ===
        assets.font.source &&
      initial.children[2]?.components.CanvasDrawing?.fields.source ===
        assets.icon.source &&
      initial.children[3]?.components.CanvasBitmap?.fields.source ===
        assets.bitmap.source &&
      initial.children[4]?.components.CanvasGlyphRun?.fields.source ===
        assets.font.source,
    "Canvas entity components did not preserve the authored assets and content",
  );
  const backgroundStyle = initial.children[0]?.components.CanvasStyle?.fields;
  const textStyle = initial.children[1]?.components.CanvasStyle?.fields;
  const iconStyle = initial.children[2]?.components.CanvasStyle?.fields;
  const bitmapStyle = initial.children[3]?.components.CanvasStyle?.fields;
  const glyphStyle = initial.children[4]?.components.CanvasStyle?.fields;
  const glyphRun = initial.children[4]?.components.CanvasGlyphRun;
  const glyphTable = glyphRun?.fields.glyphs as
    | {
        nextSlot: number;
        rows: Map<number, { glyph_id: number; position: readonly number[] }>;
      }
    | undefined;
  const glyphRow = glyphTable?.rows.get(0);
  const isNear = (actual: unknown, expected: number) =>
    typeof actual === "number" && Math.abs(actual - expected) < 1e-6;
  expect(
    initial.canvas.extent[0] === 4 &&
      initial.canvas.extent[1] === 3 &&
      initial.canvas.unitsPerMetre === 1 &&
      isNear(backgroundStyle?.scale_x, 4) &&
      isNear(backgroundStyle?.scale_y, 3) &&
      isNear(textStyle?.x, 0.18) &&
      isNear(textStyle?.y, 0.28) &&
      isNear(iconStyle?.x, 1.2) &&
      isNear(iconStyle?.y, 1) &&
      isNear(iconStyle?.scale_x, 0.006) &&
      isNear(iconStyle?.scale_y, 0.006) &&
      isNear(bitmapStyle?.x, 2.8) &&
      isNear(bitmapStyle?.y, 0.6) &&
      initial.children[3]?.components.CanvasBitmap?.fields.width === 1 &&
      initial.children[3]?.components.CanvasBitmap?.fields.height === 1 &&
      isNear(glyphStyle?.x, 0.2) &&
      isNear(glyphStyle?.y, 0.6) &&
      glyphTable?.nextSlot === 1 &&
      glyphRow?.glyph_id === glyphId &&
      isNear(glyphRow?.position[0], 0.4) &&
      isNear(glyphRow?.position[1], 0.2),
    "Canvas components changed the authored dimensions, geometry or glyph row",
  );

  const styleDescriptor = canvasClient.components.CanvasStyle;
  expect(styleDescriptor, "Canvas target omitted CanvasStyle");
  const cursorEntity = aliasId(canvasOutcome, 7);
  const opacityTarget = {
    component: styleDescriptor.id,
    offsets: [styleDescriptor.fields.opacity!.offset],
  };
  const setOpacity = async (opacity: number) =>
    successfulBatch(
      await canvasClient.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: cursorEntity },
          component: styleDescriptor.id,
          field: componentFields(canvasClient, "CanvasStyle", { opacity })[0]!,
        },
      ]),
    );
  const cursorStyle = async () => {
    const snapshot = await canvasSnapshot(canvasClient, canvasEntity);
    return snapshot.children.find(
      (item) => item.symbolicId === "surface-cursor",
    )?.components.CanvasStyle;
  };
  const effectiveOpacity = (style: Awaited<ReturnType<typeof cursorStyle>>) =>
    Number(style?.fields.opacity);
  // Writes to CanvasStyle are ordinary field writes: the last one wins.
  await setOpacity(0.25);
  expect(
    effectiveOpacity(await cursorStyle()) === 0.25,
    "CanvasStyle write was not stored",
  );
  await setOpacity(0.8);
  expect(
    Math.abs(effectiveOpacity(await cursorStyle()) - 0.8) < 1e-6,
    "A later CanvasStyle write did not replace the earlier one",
  );

  const clip = await canvasClient.createAsset(
    10,
    canvasClient.encodeAnimationClip({
      duration: 1,
      tracks: [
        {
          property: opacityTarget,
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: 0.2 },
              interpolation: { kind: "linear" },
            },
            {
              time: 1,
              value: { kind: "f32", value: 0.8 },
              interpolation: { kind: "step" },
            },
          ],
        },
      ],
    }).buffer,
  );
  await waitSurfaceAssets(canvasClient, [clip.source]);
  // A controller adds its contribution eval(t) - eval(0) to the field's
  // current value: from 0.25, seeking to 0.5 adds 0.5 - 0.2.
  await setOpacity(0.25);
  const animated = 0.25 + (0.5 - 0.2);
  const controller = await canvasClient.createAnimationController({
    speed: 0,
    drivers: [
      {
        source: clip.source,
        track: 0,
        target: cursorEntity,
        property: opacityTarget,
      },
    ],
  });
  await canvasClient.controlAnimationController(controller, { action: "play" });
  await canvasClient.controlAnimationController(controller, {
    action: "seek",
    time: 0.5,
  });
  expect(
    Math.abs(effectiveOpacity(await cursorStyle()) - animated) < 1e-5,
    "CanvasStyle animation did not use the entity target",
  );
  successfulBatch(
    await canvasClient.batch([
      {
        kind: "placeEntity",
        entity: { kind: "handle", id: cursorEntity },
        placement: {
          parent: { kind: "handle", id: canvasEntity },
          before: { kind: "handle", id: aliasId(canvasOutcome, 2) },
        },
      },
    ]),
  );
  expect(
    Math.abs(effectiveOpacity(await cursorStyle()) - animated) < 1e-5,
    "Reordering invalidated live CanvasStyle animation",
  );
  const reordered = await canvasSnapshot(canvasClient, canvasEntity);
  expect(
    reordered.children[0]?.symbolicId === "surface-cursor",
    "Canvas child reordering changed entity identity",
  );

  const replacementOutcome = successfulBatch(
    await canvasClient.batch([
      { kind: "delete", entity: { kind: "handle", id: cursorEntity } },
      createEntity(8, "surface-cursor-replacement"),
      insertComponent(canvasClient, "CanvasStyle", { kind: "alias", alias: 8 }),
      insertComponent(
        canvasClient,
        "CanvasDrawing",
        { kind: "alias", alias: 8 },
        {
          source: assets.panel.source,
        },
      ),
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias: 8 },
        placement: {
          parent: { kind: "handle", id: canvasEntity },
          before: { kind: "handle", id: aliasId(canvasOutcome, 2) },
        },
      },
    ]),
  );
  const replacementEntity = aliasId(replacementOutcome, 8);
  const staleSeek = await canvasClient
    .controlAnimationController(controller, { action: "seek", time: 0.5 })
    .then(
      () => undefined,
      (error: unknown) => error,
    );
  expect(
    staleSeek instanceof Error &&
      "code" in staleSeek &&
      staleSeek.code === "IPP_REQUEST_REJECTED" &&
      staleSeek.message.endsWith(": InvalidEntity"),
    "A controller with a deleted Canvas target did not reject its stale seek",
  );
  const replacementStyle = (
    await canvasSnapshot(canvasClient, canvasEntity)
  ).children.find((item) => item.symbolicId === "surface-cursor-replacement")
    ?.components.CanvasStyle;
  expect(
    replacementEntity !== cursorEntity &&
      replacementStyle?.fields.opacity === 1 &&
      effectiveOpacity(replacementStyle) === 1,
    "A stale animation retargeted a replacement Canvas entity",
  );
  await canvasClient.deleteAnimationController(controller);
  const replacement = async () =>
    (await canvasSnapshot(canvasClient, canvasEntity)).children.find(
      (item) => item.symbolicId === "surface-cursor-replacement",
    )?.components.CanvasStyle;
  expect(
    effectiveOpacity(await replacement()) === 1,
    "Deleting a stale controller changed its replacement entity",
  );
  for (const probe of [
    {
      entity: replacementEntity,
      symbolicId: "surface-cursor-replacement",
      component: "CanvasStyle",
      field: "opacity",
      invalid: 1.1,
      changed: 0.5,
    },
    {
      entity: aliasId(canvasOutcome, 5),
      symbolicId: "surface-bitmap",
      component: "CanvasBitmap",
      field: "width",
      invalid: -1,
      changed: 2,
    },
    {
      entity: aliasId(canvasOutcome, 3),
      symbolicId: "surface-text",
      component: "CanvasText",
      field: "font_size",
      invalid: 0,
      changed: 0.2,
    },
  ]) {
    const descriptor = canvasClient.components[probe.component];
    expect(descriptor, `Canvas target omitted ${probe.component}`);
    const write = (value: number) =>
      canvasClient.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: probe.entity },
          component: descriptor.id,
          field: componentFields(canvasClient, probe.component, {
            [probe.field]: value,
          })[0]!,
        },
      ]);
    const current = async () =>
      (await canvasSnapshot(canvasClient, canvasEntity)).children.find(
        (item) => item.symbolicId === probe.symbolicId,
      )?.components[probe.component];
    // An out-of-range write is refused and has no effect: the component
    // keeps every previous value. A later valid write changes the value.
    const previous = await current();
    const authored = previous?.fields[probe.field];
    expect(
      typeof authored === "number",
      `${probe.component} has no authored ${probe.field}`,
    );
    const invalid = await write(probe.invalid);
    expect(
      !invalid.ok && invalid.error.reason === "InvalidValue",
      `${probe.component} accepted invalid ${probe.field}`,
    );
    expect(
      stableText(await current()) === stableText(previous),
      `Invalid ${probe.component} ${probe.field} changed the stored component`,
    );
    successfulBatch(await write(probe.changed));
    expect(
      isNear((await current())?.fields[probe.field], probe.changed),
      `A valid ${probe.component} ${probe.field} write after a refused one had no effect`,
    );
    successfulBatch(await write(authored));
    expect(
      stableText(await current()) === stableText(previous),
      `Restoring ${probe.component} ${probe.field} did not restore the component`,
    );
  }

  const staleDelete = await canvasClient.batch([
    { kind: "delete", entity: { kind: "handle", id: cursorEntity } },
  ]);
  expect(
    !staleDelete.ok && staleDelete.error.reason === "InvalidEntity",
    "Deleting a removed Canvas entity did not return a correlated error",
  );

  expect(canvasClient.world, "Canvas client omitted its World descriptor");
  const settled = stableText(await canvasSnapshot(canvasClient, canvasEntity));
  const textDescriptor = canvasClient.components.CanvasText;
  expect(textDescriptor, "Canvas target omitted CanvasText");
  const foreignWorld = canvasClient.world.id === 1n ? 2n : 1n;
  for (const [label, source, message] of [
    ["foreign producer", `producer://${foreignWorld}/17/1`, /another World/],
    ["reserved numeric", "asset://1", /Numeric asset references/],
  ] as const) {
    for (const operations of [
      [
        createEntity(1, "surface-rejected-text"),
        insertComponent(
          canvasClient,
          "CanvasText",
          { kind: "alias", alias: 1 },
          { text: label, source, font_size: 0.14 },
        ),
      ],
      [
        {
          kind: "setField" as const,
          entity: { kind: "handle" as const, id: aliasId(canvasOutcome, 3) },
          component: textDescriptor.id,
          field: componentFields(canvasClient, "CanvasText", { source })[0]!,
        },
      ],
    ]) {
      const rejection = await canvasClient.batch(operations).then(
        () => undefined,
        (error: unknown) => error,
      );
      expect(
        rejection instanceof Error &&
          "code" in rejection &&
          rejection.code === "IPP_REQUEST_REJECTED" &&
          message.test(rejection.message),
        `${label} Canvas asset reference returned an unexpected result: ${String(rejection)}`,
      );
      expect(
        stableText(await canvasSnapshot(canvasClient, canvasEntity)) ===
          settled,
        `${label} Canvas asset rejection changed entity or component state`,
      );
    }
  }

  expect(client.world, "Surface client omitted its World descriptor");
  await exerciseSurfaceCachePolicy(client, entity, SURFACE_CACHE_POLICY);
  return {
    entity,
    canvasWorld: created.reference,
    canvasClient,
    canvasEntity,
    replacementEntity,
  };
}
