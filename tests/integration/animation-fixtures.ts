import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type {
  AnimationWorldClient,
  AnimationClipSource,
  Command,
  FieldValue,
  Inspection,
  AnimationPlaybackEvent,
  AnimationControllerSnapshot,
  AnimationDriverDescription,
} from "@ipp/client";
import { aliasId, createEntity, successfulBatch } from "./camera-fixtures.js";

export interface AnimationContract {
  encodeAnimationClip(clip: AnimationClipSource): Uint8Array<ArrayBuffer>;
}
export type AnimationRecord = (kind: string, value: unknown) => Promise<void>;

export function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/** Shared fixtures use generated descriptors and production client operations. */
export class AnimationFixture {
  readonly events: AnimationPlaybackEvent[] = [];
  constructor(
    readonly client: AnimationWorldClient,
    readonly contract: AnimationContract,
    readonly record: AnimationRecord,
  ) {
    check(client.capabilities.animation, "animation capability missing");
    client.onPlaybackEvent((event) => {
      this.events.push(event);
    });
  }

  fields(
    name: string,
    values: Record<
      string,
      number | string | bigint | boolean | Uint8Array<ArrayBuffer>
    >,
  ) {
    const descriptor = this.client.components[name];
    check(descriptor, `missing ${name}`);
    return Object.entries(values).map(([name, value]) => {
      const field = descriptor.fields[name];
      check(field, `missing field ${name}`);
      let typed: FieldValue;
      switch (field.kind) {
        case 1:
          typed = { kind: "f32", value: Number(value) };
          break;
        case 2:
          typed = {
            kind: "entity",
            value: { kind: "handle", id: BigInt(value as bigint) },
          };
          break;
        case 3:
          typed = { kind: "u32", value: Number(value) };
          break;
        case 4:
          typed = { kind: "u64", value: BigInt(value as bigint) };
          break;
        case 5:
          typed = { kind: "string", value: String(value) };
          break;
        case 6:
          check(value instanceof Uint8Array, "bytes required");
          typed = { kind: "bytes", value };
          break;
        case 7:
          check(typeof value === "boolean", "boolean required");
          typed = { kind: "bool", value };
          break;
      }
      return { offset: field.offset, value: typed };
    });
  }

  async create(
    symbol: string,
    components: Record<
      string,
      Record<
        string,
        number | string | bigint | boolean | Uint8Array<ArrayBuffer>
      >
    >,
  ) {
    const entity = { kind: "alias", alias: 1 } as const;
    const operations: Command[] = [createEntity(1, symbol)];
    for (const [name, values] of Object.entries(components))
      operations.push({
        kind: "insertComponent",
        entity,
        component: this.client.components[name]!.id,
        fields: this.fields(name, values),
      });
    const outcome = await this.client.batch(operations);
    await this.record("animation.create", outcome);
    return aliasId(outcome, 1);
  }

  set(
    entity: bigint,
    component: string,
    values: Record<string, number | string | bigint | boolean>,
  ): Command[] {
    return this.fields(component, values).map((field) => ({
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: this.client.components[component]!.id,
      field,
    }));
  }

  curve(
    component: string,
    field: string,
    from: number,
    to: number,
  ): AnimationClipSource {
    const descriptor = this.client.components[component]!;
    return {
      duration: 2,
      tracks: [
        {
          property: {
            component: descriptor.id,
            offsets: [descriptor.fields[field]!.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: from },
              interpolation: {
                kind: "bezier",
                time1: 0,
                time2: 0.5,
                value1: { kind: "f32", value: from + 8 },
                value2: { kind: "f32", value: to + 8 },
              },
            },
            { time: 2, value: { kind: "f32", value: to } },
          ],
        },
      ],
    };
  }

  async upload(clip: AnimationClipSource, asset = 501n) {
    const bytes = this.contract.encodeAnimationClip(clip);
    const outcome = clientAssetSource(this.client.session, 10, asset);
    await this.client.registerAsset(outcome, bytes.buffer);
    await this.record("animation.asset", outcome);
    return outcome.source;
  }

  driver(
    target: bigint,
    source: string,
    track: number,
    component: string,
    fields: readonly string[],
  ): AnimationDriverDescription {
    const descriptor = this.client.components[component];
    check(descriptor, `missing ${component}`);
    return {
      source,
      track,
      target,
      property: {
        component: descriptor.id,
        offsets: fields.map((field) => {
          const definition = descriptor.fields[field];
          check(definition, `missing ${component}.${field}`);
          return definition.offset;
        }),
      },
    };
  }

  async controller(drivers: readonly AnimationDriverDescription[]) {
    const description = { drivers, speed: 0 };
    const id = await this.client.createAnimationController(description);
    await this.record("animation.controller.create", { id, description });
    return id;
  }

  async inspect() {
    const result = await this.client.inspect();
    await this.record("animation.inspect", result);
    return result;
  }

  state(inspection: Inspection, id: bigint): AnimationControllerSnapshot {
    const controller = inspection.controllers?.find(
      (controller) => controller.id === id,
    );
    check(controller, "controller observation missing");
    return controller;
  }

  value(
    inspection: Inspection,
    entity: bigint,
    component: string,
    field: string,
    layer: "base" | "effective" = "effective",
  ) {
    const definition = this.client.components[component]!;
    const value = inspection.entities
      .find((e) => e.id === entity)
      ?.[layer].find((c) => c.component === definition.id)?.fields[field];
    check(typeof value === "number", "numeric observation missing");
    return value;
  }

  async seekPaused(controller: bigint, time: number) {
    await this.client.controlAnimationController(controller, {
      action: "play",
    });
    await this.client.controlAnimationController(controller, {
      action: "pause",
    });
    await this.client.controlAnimationController(controller, {
      action: "seek",
      time,
    });
    const inspection = await this.inspect();
    check(
      this.state(inspection, controller).state === "paused",
      "controller did not pause",
    );
    check(
      this.state(inspection, controller).time === time,
      "seek advanced beyond destination",
    );
    return inspection;
  }
}

/** One scenario reused by native WebSocket and browser worker/WASM environments. */
export async function propertyAnimationScenario(
  client: AnimationWorldClient,
  contract: AnimationContract,
  record: AnimationRecord,
) {
  const fixture = new AnimationFixture(client, contract, record);
  const a = await fixture.create("animated-a", { Scalar: { value: 99 } });
  const b = await fixture.create("animated-b", { Scalar: { value: 88 } });
  const source = await fixture.upload(fixture.curve("Scalar", "value", 0, 0));
  const pa = await fixture.controller([
    fixture.driver(a, source, 0, "Scalar", ["value"]),
  ]);
  const pb = await fixture.controller([
    fixture.driver(b, source, 0, "Scalar", ["value"]),
  ]);
  let inspection = await fixture.seekPaused(pa, 0.4375);
  // Bézier u=.5: absolute x=.4375 and y=6 despite equal endpoint values.
  check(
    Math.abs(fixture.value(inspection, a, "Scalar", "value") - 6) < 1e-5,
    "Bézier time/value curve was not evaluated",
  );
  check(
    fixture.value(inspection, a, "Scalar", "value", "base") === 99,
    "animation overwrote producer base",
  );
  await fixture.seekPaused(pb, 2);
  for (let i = 0; i < 3; i++) await client.waitForFrame();
  inspection = await fixture.inspect();
  check(fixture.state(inspection, pa).time === 0.4375, "paused clock advanced");
  check(
    fixture.value(inspection, b, "Scalar", "value") === 0,
    "shared asset did not sample independently",
  );
  await client.updateAnimationController(pa, {
    ...fixture.state(inspection, pa).description,
    speed: 1,
  });
  client.playback(pa, { action: "play" });
  for (let attempt = 0; attempt < 30; attempt++) {
    await client.waitForFrame();
    inspection = await fixture.inspect();
    if (fixture.state(inspection, pa).time > 0.4375) break;
  }
  check(
    fixture.state(inspection, pa).time > 0.4375,
    "host did not advance running controller",
  );
  check(
    fixture.state(inspection, pb).time === 2,
    "one controller advanced another clock",
  );
  await fixture.seekPaused(pa, 0.4375);
  const rejected = await client.batch([
    {
      kind: "removeComponent",
      entity: { kind: "handle", id: a },
      component: client.components.Scalar!.id,
    },
    {
      kind: "removeComponent",
      entity: { kind: "handle", id: a },
      component: 65535,
    },
  ]);
  check(!rejected.ok, "invalid batch unexpectedly committed");
  inspection = await fixture.inspect();
  check(
    fixture.state(inspection, pa).state === "stopped",
    "Applied removal must invalidate playback even when a later command fails",
  );
  check(
    !inspection.entities
      .find((entity) => entity.id === a)!
      .effective.some(
        (value) => value.component === client.components.Scalar!.id,
      ),
    "Failed batches keep the earlier component removal",
  );
  successfulBatch(
    await client.batch([
      {
        kind: "insertComponent",
        entity: { kind: "handle", id: a },
        component: client.components.Scalar!.id,
        fields: fixture.fields("Scalar", { value: 7 }),
      },
    ]),
  );
  inspection = await fixture.inspect();
  check(
    fixture.state(inspection, pa).state === "stopped",
    "replacement did not invalidate playback",
  );
  check(
    fixture.value(inspection, a, "Scalar", "value") === 7,
    "old controller wrote replacement storage",
  );
  check(
    fixture.events.some(
      (e) => e.controller.id === pa && e.kind === "invalidated",
    ),
    "binding-loss event missing",
  );
  client.playback(pb, { action: "stop" });
  inspection = await fixture.inspect();
  check(
    fixture.value(inspection, b, "Scalar", "value") === 88,
    "stop did not expose current authored value",
  );
  // A new binding created while another controller contributes must inherit
  // that driver's original, not the currently sampled value.
  await fixture.seekPaused(pb, 0.4375);
  const inherited = await fixture.controller([
    fixture.driver(b, source, 0, "Scalar", ["value"]),
  ]);
  await fixture.seekPaused(inherited, 2);
  await client.deleteAnimationController(pb);
  inspection = await fixture.inspect();
  check(
    fixture.value(inspection, b, "Scalar", "value") === 0,
    "deleting one controller withdrew the surviving paused contribution",
  );
  await client.deleteAnimationController(inherited);
  inspection = await fixture.inspect();
  check(
    fixture.value(inspection, b, "Scalar", "value") === 88,
    "new binding retained animated output instead of the inherited original",
  );
  check(
    !inspection.controllers?.some(
      (controller) => controller.id === pb || controller.id === inherited,
    ),
    "deleted controllers remain visible",
  );

  const c = await fixture.create("synchronized-a", { Scalar: { value: 21 } });
  const d = await fixture.create("synchronized-b", { Scalar: { value: 34 } });
  const synchronized = await fixture.controller([
    fixture.driver(c, source, 0, "Scalar", ["value"]),
    fixture.driver(d, source, 0, "Scalar", ["value"]),
  ]);
  inspection = await fixture.seekPaused(synchronized, 0.4375);
  for (const target of [c, d]) {
    check(
      Math.abs(fixture.value(inspection, target, "Scalar", "value") - 6) < 1e-5,
      "shared controller did not sample both entities at one time",
    );
  }
  check(
    fixture.state(inspection, synchronized).description.drivers.length === 2,
    "controller lost a target binding",
  );
  successfulBatch(await client.batch(fixture.set(c, "Scalar", { value: 55 })));
  inspection = await fixture.inspect();
  check(
    fixture.value(inspection, c, "Scalar", "value", "base") === 55,
    "paused controller hid the latest producer input from inspection",
  );
  await client.deleteAnimationController(synchronized);
  inspection = await fixture.inspect();
  check(
    fixture.value(inspection, c, "Scalar", "value") === 55,
    "controller deletion lost latest producer input",
  );
  check(
    fixture.value(inspection, d, "Scalar", "value") === 34,
    "controller deletion did not restore its other target",
  );

  const reverseTarget = await fixture.create("reverse-playback", {
    Scalar: { value: 42 },
  });
  const reverseSource = await fixture.upload(
    {
      duration: 0.1,
      tracks: [
        {
          property: {
            component: client.components.Scalar!.id,
            offsets: [client.components.Scalar!.fields.value!.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: 0 },
              interpolation: { kind: "linear" },
            },
            { time: 0.1, value: { kind: "f32", value: 10 } },
          ],
        },
      ],
    },
    502n,
  );
  const reverse = await fixture.controller([
    fixture.driver(reverseTarget, reverseSource, 0, "Scalar", ["value"]),
  ]);
  await fixture.seekPaused(reverse, 0.1);
  await client.controlAnimationController(reverse, {
    action: "playAtSpeed",
    speed: -1,
  });
  for (let attempt = 0; attempt < 30; attempt++) {
    await client.waitForFrame();
    inspection = await fixture.inspect();
    if (fixture.state(inspection, reverse).state === "completed") break;
  }
  check(
    fixture.state(inspection, reverse).state === "completed" &&
      fixture.state(inspection, reverse).time === 0,
    "negative playback did not complete at the start endpoint",
  );
  check(
    fixture.value(inspection, reverseTarget, "Scalar", "value") === 0,
    "negative playback did not hold the start sample",
  );
  await client.controlAnimationController(reverse, {
    action: "playAtSpeed",
    speed: 1,
  });
  for (let attempt = 0; attempt < 30; attempt++) {
    await client.waitForFrame();
    inspection = await fixture.inspect();
    if (fixture.state(inspection, reverse).time > 0) break;
  }
  check(
    fixture.state(inspection, reverse).time > 0,
    "positive playback did not resume inward from the completed start",
  );
  await client.controlAnimationController(reverse, { action: "pause" });
  await client.updateAnimationController(reverse, {
    ...fixture.state(await fixture.inspect(), reverse).description,
    speed: -1,
    looping: true,
  });
  await client.controlAnimationController(reverse, {
    action: "seek",
    time: 0.001,
  });
  await client.controlAnimationController(reverse, {
    action: "playAtSpeed",
    speed: -1,
  });
  for (let attempt = 0; attempt < 30; attempt++) {
    await client.waitForFrame();
    inspection = await fixture.inspect();
    if (fixture.state(inspection, reverse).time > 0.05) break;
  }
  check(
    fixture.state(inspection, reverse).state === "playing" &&
      fixture.state(inspection, reverse).time > 0.05,
    "negative looping playback did not wrap to the clip end",
  );
  await client.controlAnimationController(reverse, { action: "pause" });
  inspection = await fixture.inspect();
  const heldValue = fixture.value(inspection, reverseTarget, "Scalar", "value");
  const delayed = clientAssetSource(client.session, 10, 503n);
  const delayedClip: AnimationClipSource = {
    duration: 0.1,
    tracks: [
      {
        property: {
          component: client.components.Scalar!.id,
          offsets: [client.components.Scalar!.fields.value!.offset],
        },
        keys: [
          {
            time: 0,
            value: { kind: "f32", value: 20 },
            interpolation: { kind: "linear" },
          },
          { time: 0.1, value: { kind: "f32", value: 30 } },
        ],
      },
    ],
  };
  await client.transitionAnimationController(reverse, {
    description: {
      ...fixture.state(inspection, reverse).description,
      drivers: [
        fixture.driver(reverseTarget, delayed.source, 0, "Scalar", ["value"]),
      ],
      speed: 1,
      looping: false,
    },
    duration: 0.1,
    easing: "smoothstep",
    startTime: { policy: "restart" },
  });
  for (let frame = 0; frame < 3; frame++) await client.waitForFrame();
  inspection = await fixture.inspect();
  check(
    fixture.state(inspection, reverse).transition?.pending === true &&
      fixture.state(inspection, reverse).transition?.elapsed === 0,
    "pending transition advanced before its destination source loaded",
  );
  check(
    fixture.value(inspection, reverseTarget, "Scalar", "value") === heldValue,
    "pending transition did not retain its origin contribution",
  );
  await client.registerAsset(
    delayed,
    fixture.contract.encodeAnimationClip(delayedClip).buffer,
  );
  for (let attempt = 0; attempt < 30; attempt++) {
    await client.waitForFrame();
    inspection = await fixture.inspect();
    if (fixture.state(inspection, reverse).transition?.pending === false) break;
  }
  check(
    fixture.state(inspection, reverse).transition?.pending === false &&
      fixture.state(inspection, reverse).transition?.elapsed === 0,
    "ready destination did not preserve the paused transition origin",
  );
  check(
    fixture.value(inspection, reverseTarget, "Scalar", "value") === heldValue,
    "destination readiness changed a paused transition sample",
  );
  await client.deleteAnimationController(reverse);
  await client.deleteAnimationController(pa);
  await record("animation.transitions", fixture.events);
  return {
    independentControllers: 2,
    synchronizedTargets: 2,
    inheritedOriginal: 88,
    bezierMidpoint: 6,
    signedPlayback: true,
    events: fixture.events.length,
  };
}
