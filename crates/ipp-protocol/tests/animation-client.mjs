import assert from "node:assert/strict";
import test from "node:test";
import {
  encodeManifestLayout,
  generateClient,
  manifestVariant,
} from "./generated-client.mjs";

const client = await generateClient("animation");
const { codec, manifest } = client;
const layout = (name, values) => encodeManifestLayout(client, name, values);

test("contracts expose property animation", () => {
  assert.equal("Transform" in codec.components, true);
  assert.equal("request-upload-asset" in manifest.WIRE_LAYOUTS, false);
  assert.equal("uploadAsset" in codec.IppClient.prototype, false);
  assert.equal(manifest.ASSET_FORMATS.ASSET_ANIMATION.typeId, 10);
  assert.equal("encodeAnimationClip" in codec, true);
  assert.equal("playback" in codec.IppClient.prototype, true);
  assert.equal("registerAsset" in codec.IppClient.prototype, true);
  assert.equal("REQUEST_PLAYBACK" in codec.WIRE, true);
});

test("every playback control agrees with the wire manifest and reserves identity zero", () => {
  for (const [index, action] of [
    "play",
    "pause",
    "stop",
    "seek",
    "restart",
    "playAtSpeed",
  ].entries()) {
    const control =
      action === "seek"
        ? { action, time: 0.4375 }
        : action === "playAtSpeed"
          ? { action, speed: -1.5 }
          : { action };
    const body = {
      kind: "command",
      command: { type: "AnimationPlaybackCommand", controller: 41n, control },
    };
    assert.deepEqual(
      codec.encodeRequest({ session: 7n, requestId: 0n, body }),
      layout("request-playback", {
        session: 7n,
        request_id: 0n,
        tag: manifestVariant(client, "REQUEST_PLAYBACK"),
        controller: 41n,
        control: index,
        time: control.time ?? 0,
        speed: control.speed ?? 0,
      }).bytes,
    );
    assert.throws(
      () => codec.encodeRequest({ session: 7n, requestId: 1n, body }),
      /identity/,
    );
  }
  for (const control of [
    { action: "seek", time: NaN },
    { action: "seek", time: -1 },
    { action: "reverse" },
    { action: "play", time: 1 },
    { action: "playAtSpeed", speed: NaN },
    { action: "playAtSpeed", speed: Infinity },
  ]) {
    assert.throws(() =>
      codec.encodeRequest({
        session: 7n,
        requestId: 0n,
        body: {
          kind: "command",
          command: {
            type: "AnimationPlaybackCommand",
            controller: 41n,
            control,
          },
        },
      }),
    );
  }
});

test("playback events and controller clocks decode from manifest bytes with strict bounds", () => {
  const controller = layout("controller-state", {
    id: 41n,
    state: 2,
    time: 0.4375,
  });
  const event = layout("playback-event", { controller, kind: 1, reason: "" });
  const packet = layout("response-playback", {
    session: 7n,
    request_id: 0n,
    tick: 50n,
    tag: manifestVariant(client, "RESPONSE_PLAYBACK"),
    events: [event],
  }).bytes;
  assert.deepEqual(codec.decodeResponse(packet, 7n).body, {
    kind: "playback",
    events: [
      {
        controller: { id: 41n, state: "paused", time: 0.4375 },
        kind: "paused",
        reason: null,
      },
    ],
  });
  for (let length = 0; length < packet.length; length++)
    assert.throws(() => codec.decodeResponse(packet.slice(0, length), 7n));
  assert.throws(() => codec.decodeResponse(new Uint8Array([...packet, 0]), 7n));
  assert.throws(() => codec.decodeResponse(packet, 8n), /session/);
  for (const state of [4, 0xffffffff]) {
    const bad = layout("response-playback", {
      session: 7n,
      request_id: 0n,
      tick: 50n,
      tag: manifestVariant(client, "RESPONSE_PLAYBACK"),
      events: [
        layout("playback-event", {
          controller: layout("controller-state", { id: 41n, state, time: 0 }),
          kind: 1,
          reason: "",
        }),
      ],
    }).bytes;
    assert.throws(() => codec.decodeResponse(bad, 7n), /controller state/);
  }
  const inspected = codec.decodeResponse(
    layout("response-inspect", {
      next: 0n,
      session: 7n,
      request_id: 2n,
      tick: 50n,
      tag: manifestVariant(client, "RESPONSE_INSPECT"),
      gui_focus: [],
      gui_pointers: [],
      canvas: null,
      time: 10,
      entities: [],
      resources: [],
      render_diagnostics: [],
      controllers: [
        layout("animation-controller", {
          state: controller,
          description: layout("controller-description", {
            speed: 1,
            looping: false,
            drivers: [],
          }),
          transition: null,
        }),
      ],
    }).bytes,
    7n,
  );
  assert.deepEqual(inspected.body.controllers, [
    {
      id: 41n,
      state: "paused",
      time: 0.4375,
      description: { speed: 1, looping: false, drivers: [] },
    },
  ]);
});

test("immutable clip encoding rejects invalid time/value curves and retains owned payloads", () => {
  const property = {
    component: codec.components.Scalar.id,
    offsets: [codec.components.Scalar.fields.value.offset],
  };
  const clip = {
    duration: 2,
    tracks: [
      {
        property,
        keys: [
          {
            time: 0,
            value: { kind: "f32", value: 0 },
            interpolation: {
              kind: "bezier",
              time1: 0,
              time2: 0.5,
              value1: { kind: "f32", value: 8 },
              value2: { kind: "f32", value: 8 },
            },
          },
          { time: 2, value: { kind: "f32", value: 0 } },
        ],
      },
    ],
  };
  const bytes = codec.encodeAnimationClip(clip);
  assert.deepEqual([...bytes.slice(0, 8)], [73, 80, 80, 65, 4, 0, 0, 0]);
  assert.equal(new DataView(bytes.buffer).getFloat64(8, true), 2);
  for (const invalid of [
    { ...clip, duration: 0 },
    {
      ...clip,
      tracks: [
        {
          property,
          keys: [{ time: 0, value: { kind: "f32", value: Infinity } }],
        },
      ],
    },
    {
      ...clip,
      tracks: [
        {
          property,
          keys: [
            {
              ...clip.tracks[0].keys[0],
              interpolation: {
                ...clip.tracks[0].keys[0].interpolation,
                time1: 1,
              },
            },
            clip.tracks[0].keys[1],
          ],
        },
      ],
    },
  ])
    assert.throws(() => codec.encodeAnimationClip(invalid));
  assert.ok(
    codec.encodeAnimationClip({
      ...clip,
      tracks: [clip.tracks[0], clip.tracks[0]],
    }).length > bytes.length,
  );
  const owned = new Uint8Array([1, 2, 3]);
  const encoded = codec.encodeAnimationClip({
    duration: 1,
    tracks: [
      { property, keys: [{ time: 0, value: { kind: "bytes", value: owned } }] },
    ],
  });
  owned.fill(9);
  assert.deepEqual([...encoded.slice(-4)], [1, 2, 3, 0]);
  assert.ok(
    codec.encodeAnimationClip({
      duration: 1,
      tracks: [
        {
          property,
          keys: [
            { time: 0, value: { kind: "string", value: "a".repeat(70_000) } },
          ],
        },
      ],
    }).length > 70_000,
  );
  assert.ok(
    codec.encodeAnimationClip({
      duration: 1,
      tracks: [
        {
          property,
          keys: [
            {
              time: 0,
              value: { kind: "bytes", value: new Uint8Array(1 << 20) },
            },
          ],
        },
      ],
    }).length >
      1 << 20,
  );
  const manyTracks = codec.encodeAnimationClip({
    duration: 32,
    tracks: Array.from({ length: 300 }, () => ({
      property,
      keys: Array.from({ length: 33 }, (_, time) => ({
        time,
        value: { kind: "f32", value: time },
      })),
    })),
  });
  assert.equal(new DataView(manyTracks.buffer).getUint32(16, true), 300);
});

test("controller descriptions encode indexed multi-entity drivers and correlated operations", () => {
  const property = {
    component: codec.components.Scalar.id,
    offsets: [codec.components.Scalar.fields.value.offset],
  };
  const driver = {
    source: "memory:shared?literal=yes",
    variant: 7,
    track: 2,
    target: 41n,
    property,
    entityBindings: [],
    weight: 0.5,
    additive: true,
    referenceTime: 0.25,
    repeat: true,
  };
  const description = {
    speed: -1.5,
    looping: true,
    drivers: Array.from({ length: 300 }, (_, index) => ({
      ...driver,
      track: index,
      target: BigInt(41 + index),
    })),
  };
  const manifestDescription = layout("controller-description", {
    speed: description.speed,
    looping: description.looping,
    drivers: description.drivers.map((entry) =>
      layout("animation-driver", {
        source: entry.source,
        variant: entry.variant,
        track: entry.track,
        target: entry.target,
        property: layout("animation-property", {
          target: layout("animation-target-property", {
            kind: manifestVariant(client, "ANIMATION_TARGET_PROPERTY"),
            component: property.component,
            indices: property.offsets.map((value) =>
              layout("animation-index", { value }),
            ),
          }),
        }),
        entity_bindings: entry.entityBindings,
        weight: entry.weight,
        additive: entry.additive,
        reference_time: entry.referenceTime,
        repeat: entry.repeat,
      }),
    ),
  });
  for (const [action, tag, extra] of [
    [
      "create",
      "REQUEST_CONTROLLER_CREATE",
      { description: manifestDescription },
    ],
    [
      "update",
      "REQUEST_CONTROLLER_UPDATE",
      { id: 13n, description: manifestDescription },
    ],
    ["delete", "REQUEST_CONTROLLER_DELETE", { id: 13n }],
    [
      "control",
      "REQUEST_CONTROLLER_CONTROL",
      { id: 13n, control: 3, time: 0.75, speed: 0 },
    ],
    [
      "transition",
      "REQUEST_CONTROLLER_TRANSITION",
      {
        id: 13n,
        description: manifestDescription,
        duration: 0.5,
        easing: 1,
        start_time: 3,
        seek_time: 0.25,
      },
    ],
  ]) {
    const command =
      action === "create"
        ? { action, description }
        : action === "update"
          ? { action, id: 13n, description }
          : action === "delete"
            ? { action, id: 13n }
            : action === "control"
              ? { action, id: 13n, control: { action: "seek", time: 0.75 } }
              : {
                  action,
                  id: 13n,
                  transition: {
                    description,
                    duration: 0.5,
                    easing: "smoothstep",
                    startTime: { policy: "seek", time: 0.25 },
                  },
                };
    const request = {
      session: 7n,
      requestId: 19n,
      body: { kind: "animationController", command },
    };
    assert.deepEqual(
      codec.encodeRequest(request),
      layout(`request-controller-${action}`, {
        session: 7n,
        request_id: 19n,
        tag: manifestVariant(client, tag),
        ...extra,
      }).bytes,
    );
    assert.throws(
      () => codec.encodeRequest({ ...request, requestId: 0n }),
      /identity/,
    );
  }
  assert.deepEqual(
    codec.encodeRequest({
      session: 7n,
      requestId: 20n,
      body: {
        kind: "animationController",
        command: {
          action: "transition",
          id: 13n,
          transition: { description, duration: 0 },
        },
      },
    }),
    layout("request-controller-transition", {
      session: 7n,
      request_id: 20n,
      tag: manifestVariant(client, "REQUEST_CONTROLLER_TRANSITION"),
      id: 13n,
      description: manifestDescription,
      duration: 0,
      easing: 0,
      start_time: 0,
      seek_time: 0,
    }).bytes,
  );
  const state = layout("controller-state", { id: 13n, state: 2, time: 0.75 });
  const packet = layout("response-inspect", {
    next: 0n,
    session: 7n,
    request_id: 19n,
    tick: 50n,
    tag: manifestVariant(client, "RESPONSE_INSPECT"),
    gui_focus: [],
    gui_pointers: [],
    canvas: null,
    time: 10,
    entities: [],
    resources: [],
    render_diagnostics: [],
    controllers: [
      layout("animation-controller", {
        state,
        description: manifestDescription,
        transition: layout("controller-transition-state", {
          duration: 0.5,
          elapsed: 0.25,
          easing: 1,
          pending: false,
        }),
      }),
    ],
  }).bytes;
  assert.deepEqual(codec.decodeResponse(packet, 7n).body.controllers, [
    {
      id: 13n,
      state: "paused",
      time: 0.75,
      description,
      transition: {
        duration: 0.5,
        elapsed: 0.25,
        easing: "smoothstep",
        pending: false,
      },
    },
  ]);
  for (const id of [0n, 13n]) {
    const response = layout("response-controller", {
      session: 7n,
      request_id: 19n,
      tick: 50n,
      tag: manifestVariant(client, "RESPONSE_CONTROLLER"),
      id,
    }).bytes;
    assert.deepEqual(codec.decodeResponse(response, 7n).body, {
      kind: "animationController",
      id: id || null,
    });
  }
  const invalid = [
    { action: "delete", id: 0n },
    { action: "create", description: { ...description, speed: NaN } },
    {
      action: "transition",
      id: 13n,
      transition: { description, duration: -1 },
    },
    {
      action: "transition",
      id: 13n,
      transition: {
        description,
        duration: 1,
        startTime: { policy: "seek", time: -1 },
      },
    },
    {
      action: "transition",
      id: 13n,
      transition: { description, duration: 1, easing: "bounce" },
    },
    {
      action: "transition",
      id: 13n,
      transition: {
        description,
        duration: 1,
        startTime: { policy: "end" },
      },
    },
    {
      action: "create",
      description: {
        ...description,
        drivers: [
          {
            ...driver,
            property: { component: 1, offsets: Array(4097).fill(0) },
          },
        ],
      },
    },
  ];
  for (const command of invalid)
    assert.throws(() =>
      codec.encodeRequest({
        session: 7n,
        requestId: 19n,
        body: { kind: "animationController", command },
      }),
    );
  assert.equal("createAnimationController" in codec.IppClient.prototype, true);
  assert.equal(
    "transitionAnimationController" in codec.IppClient.prototype,
    true,
  );
  assert.equal("REQUEST_CONTROLLER_CREATE" in codec.WIRE, true);
});

test("structural clip slots reserve the null sentinel and reject invalid numbers", () => {
  const encodePlacement = (placement) =>
    codec.encodeAnimationClip({
      duration: 1,
      tracks: [
        {
          property: { entityLink: true },
          keys: [
            { time: 0, value: { kind: "entityPlacement", value: placement } },
          ],
        },
      ],
    });
  const root = encodePlacement({ parent: null, before: null });
  assert.deepEqual([...root.slice(34, 42)], Array(8).fill(255));
  const valid = encodePlacement({ parent: 0, before: 0xffff_fffe });
  assert.deepEqual([...valid.slice(34, 42)], [0, 0, 0, 0, 254, 255, 255, 255]);
  for (const field of ["parent", "before"]) {
    for (const slot of [
      0xffff_ffff,
      0x1_0000_0000,
      -1,
      0.5,
      NaN,
      Infinity,
      -Infinity,
      Number.MAX_SAFE_INTEGER,
      undefined,
      "0",
      true,
      0n,
    ]) {
      assert.throws(
        () => encodePlacement({ parent: null, before: null, [field]: slot }),
        /integer out of range/,
        `${field} slot ${String(slot)}`,
      );
    }
  }
});

test("structural drivers preserve explicit empty and large clip-local binding tables", () => {
  const rootClip = codec.encodeAnimationClip({
    duration: 1,
    tracks: [
      {
        property: { entityLink: true },
        keys: [
          {
            time: 0,
            value: {
              kind: "entityPlacement",
              value: { parent: null, before: null },
            },
            interpolation: { kind: "step" },
          },
          {
            time: 1,
            value: {
              kind: "entityPlacement",
              value: { parent: null, before: null },
            },
          },
        ],
      },
    ],
  });
  assert.deepEqual([...rootClip.slice(0, 8)], [73, 80, 80, 65, 4, 0, 0, 0]);
  const bindingLimit = manifest.WIRE_LAYOUTS["animation-driver"].fields.find(
    (field) => field.name === "entity_bindings",
  ).limit;
  assert.equal(bindingLimit, codec.MAX_MESSAGE_BYTES / 8);

  for (const entityBindings of [
    [],
    Array.from({ length: 4097 }, (_, index) => BigInt(index + 1)),
  ]) {
    const driver = {
      source: "memory:structural-v4",
      track: 0,
      target: 41n,
      property: { entityLink: true },
      entityBindings,
    };
    const description = { speed: 1, looping: false, drivers: [driver] };
    const encodedDriver = layout("animation-driver", {
      source: driver.source,
      variant: 0,
      track: 0,
      target: 41n,
      property: layout("animation-property", {
        target: layout("animation-target-entity-link", {
          kind: manifestVariant(client, "ANIMATION_TARGET_ENTITY_LINK"),
        }),
      }),
      entity_bindings: entityBindings,
      weight: 1,
      additive: false,
      reference_time: 0,
      repeat: false,
    });
    assert.deepEqual(
      codec.encodeRequest({
        session: 7n,
        requestId: 3n,
        body: {
          kind: "animationController",
          command: { action: "create", description },
        },
      }),
      layout("request-controller-create", {
        session: 7n,
        request_id: 3n,
        tag: manifestVariant(client, "REQUEST_CONTROLLER_CREATE"),
        description: layout("controller-description", {
          speed: 1,
          looping: false,
          drivers: [encodedDriver],
        }),
      }).bytes,
    );
    const response = layout("response-inspect", {
      session: 7n,
      request_id: 4n,
      tick: 5n,
      tag: manifestVariant(client, "RESPONSE_INSPECT"),
      gui_focus: [],
      gui_pointers: [],
      canvas: null,
      time: 0,
      next: 0n,
      entities: [],
      resources: [],
      render_diagnostics: [],
      controllers: [
        layout("animation-controller", {
          state: layout("controller-state", { id: 19n, state: 2, time: 0 }),
          description: layout("controller-description", {
            speed: 1,
            looping: false,
            drivers: [encodedDriver],
          }),
          transition: null,
        }),
      ],
    }).bytes;
    assert.deepEqual(
      codec.decodeResponse(response, 7n).body.controllers[0].description
        .drivers[0].entityBindings,
      entityBindings,
    );
    assert.throws(() => codec.decodeResponse(response.slice(0, -1), 7n));
  }
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 5n,
        body: {
          kind: "animationController",
          command: {
            action: "create",
            description: {
              drivers: [
                {
                  source: "memory:structural-v4",
                  track: 0,
                  target: 41n,
                  property: { entityLink: true },
                },
              ],
            },
          },
        },
      }),
    /structural animation bindings required/,
  );
  assert.throws(
    () =>
      codec.encodeRequest({
        session: 7n,
        requestId: 6n,
        body: {
          kind: "animationController",
          command: {
            action: "create",
            description: {
              drivers: [
                {
                  source: "memory:structural-v4",
                  track: 0,
                  target: 41n,
                  property: { entityLink: true },
                  entityBindings: Array(bindingLimit).fill(1n),
                },
              ],
            },
          },
        },
      }),
    /message limit/,
  );
});
