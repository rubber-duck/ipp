import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";

function concatenate(...chunks) {
  const bytes = new Uint8Array(
    chunks.reduce((size, chunk) => size + chunk.length, 0),
  );
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes;
}

const u8 = (value) => Uint8Array.of(value);
function integer(value, size) {
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  if (size === 8) view.setBigUint64(0, value, true);
  else if (size === 4) view.setUint32(0, value, true);
  else view.setUint16(0, value, true);
  return bytes;
}
const u16 = (value) => integer(value, 2);
const u32 = (value) => integer(value, 4);
const u64 = (value) => integer(value, 8);
function f32(value) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setFloat32(0, value, true);
  return bytes;
}
const text = (value) => {
  const bytes = new TextEncoder().encode(value);
  return concatenate(u32(bytes.length), bytes);
};
const world = { id: 1n, incarnation: 2n };
const target = { world, entity: 3n, component: 51, incarnation: 4n };
const targetBytes = concatenate(u64(1n), u64(2n), u64(3n), u16(51), u64(4n));
const header = concatenate(u64(7n), u64(9n));

export async function guiCodecCases(name, directory) {
  const codec = await import(
    pathToFileURL(resolve(directory, "generated.js")).href
  );
  const request = (body) =>
    codec.encodeRequest({ session: 7n, requestId: 9n, body });
  test(`${name} ordinary GUI methods replace all legacy node lanes`, () => {
    assert.equal(
      typeof codec.IppClient.prototype.subscribeGuiEffects,
      "function",
    );
    assert.equal(typeof codec.Entity.guiAction, "function");
    // Control values are component fields: no snapshot or replacement lane.
    // Semantic actions are batch commands, not a request kind.
    for (const method of [
      "semanticAction",
      "guiSnapshot",
      "guiSnapshotPage",
      "replaceControl",
      "guiSemanticSnapshot",
      "guiSemanticAction",
      "guiInput",
      "guiCommands",
      "subscribeGuiObservations",
    ])
      assert.equal(method in codec.IppClient.prototype, false, method);
    assert.ok(codec.WIRE.COMMAND_GUI_ACTION);
    assert.equal(codec.WIRE.REQUEST_GUI_ACTION, undefined);
    assert.equal(codec.WIRE.RESPONSE_GUI_TERMINAL, undefined);
    assert.ok(codec.WIRE.REQUEST_GUI_OBSERVATION);
    assert.ok(codec.WIRE.RESPONSE_GUI_OBSERVATION);
    assert.equal(codec.WIRE.REQUEST_GUI_INPUT, undefined);
    assert.equal(codec.WIRE.REQUEST_GUI_SNAPSHOT, undefined);
    assert.equal(codec.WIRE.RESPONSE_GUI_SNAPSHOT, undefined);
  });

  test(`${name} actions encode as one batch command with exact ordinary identities`, () => {
    const batch = (command) =>
      request({
        kind: "submitBatch",
        batchId: 5,
        last: true,
        operations: [command],
      });
    for (const [action, tag, payload] of [
      [{ kind: "press" }, 0, new Uint8Array()],
      [{ kind: "toggle" }, 1, new Uint8Array()],
      [{ kind: "scalar", value: 0.5 }, 2, f32(0.5)],
      [{ kind: "text", value: "é🙂" }, 3, text("é🙂")],
      [{ kind: "focus" }, 4, new Uint8Array()],
      [{ kind: "blur" }, 6, new Uint8Array()],
      [{ kind: "submit" }, 7, new Uint8Array()],
      [
        { kind: "scrollTo", offset: [12, 34] },
        8,
        concatenate(f32(12), f32(34)),
      ],
      [
        { kind: "scrollBy", delta: [-12, 34] },
        9,
        concatenate(f32(-12), f32(34)),
      ],
      [
        { kind: "scrollToIndex", index: 40, offset: 2 },
        10,
        concatenate(u32(40), f32(2)),
      ],
    ]) {
      const command = codec.Entity.guiAction(target, action);
      assert.deepEqual(command, {
        kind: "guiAction",
        entity: { kind: "handle", id: 3n },
        component: 51,
        incarnation: 4n,
        action,
      });
      assert.deepEqual(
        batch(command),
        concatenate(
          header,
          u8(codec.WIRE.REQUEST_SUBMIT_BATCH),
          u32(5),
          u8(1),
          u32(1),
          u8(codec.WIRE.COMMAND_GUI_ACTION),
          u8(codec.WIRE.REF_HANDLE),
          u64(3n),
          u16(51),
          u64(4n),
          u8(tag),
          payload,
        ),
      );
    }
    // A symbolic reference names the control's entity like any command's.
    const symbolic = codec.Entity.guiAction(
      { entity: codec.Entity.symbol("ok"), component: 51, incarnation: 4n },
      { kind: "press" },
    );
    assert.deepEqual(
      batch(symbolic),
      concatenate(
        header,
        u8(codec.WIRE.REQUEST_SUBMIT_BATCH),
        u32(5),
        u8(1),
        u32(1),
        u8(codec.WIRE.COMMAND_GUI_ACTION),
        u8(codec.WIRE.REF_SYMBOL),
        text("ok"),
        u16(51),
        u64(4n),
        u8(0),
      ),
    );
    // Action 5 was the retired value replacement.
    assert.throws(() =>
      batch(
        codec.Entity.guiAction(target, {
          kind: "replace",
          value: { kind: "bool", value: true },
        }),
      ),
    );
    for (const value of [NaN, Infinity, -Infinity])
      assert.throws(() =>
        batch(codec.Entity.guiAction(target, { kind: "scalar", value })),
      );
    assert.throws(() =>
      batch(
        codec.Entity.guiAction(target, {
          kind: "text",
          value: "x".repeat(65537),
        }),
      ),
    );
  });

  test(`${name} GUI System query collections page focus and pointer records`, () => {
    for (const [collection, tag] of [
      ["guiFocus", codec.WIRE.INSPECT_GUI_FOCUS],
      ["guiPointers", codec.WIRE.INSPECT_GUI_POINTERS],
    ])
      assert.deepEqual(
        request({ kind: "inspect", collection, target: 3n, limit: 16 }),
        concatenate(
          header,
          u8(codec.WIRE.REQUEST_INSPECT),
          u8(tag),
          u64(0n),
          u64(3n),
          u16(16),
          u16(0),
        ),
      );
    const flags = (hovered, pressed, captured) =>
      concatenate(
        u8(Number(hovered)),
        u8(Number(pressed)),
        u8(Number(captured)),
      );
    const inspection = concatenate(
      header,
      u64(8n),
      u8(codec.WIRE.RESPONSE_INSPECT),
      new Uint8Array(new Float64Array([0.5]).buffer),
      u64(0n),
      u32(0),
      u32(0),
      u32(0),
      u32(0),
      u32(1),
      targetBytes,
      u8(1),
      u32(2),
      targetBytes,
      u64(5n),
      flags(true, false, true),
      targetBytes,
      u64(6n),
      flags(false, true, false),
      u8(0),
    );
    const body = codec.decodeResponse(inspection, 7n).body;
    assert.deepEqual(body.guiFocus, [{ target, visible: true }]);
    assert.deepEqual(body.guiPointers, [
      {
        target,
        pointer: 5n,
        state: { hovered: true, pressed: false, captured: true },
      },
      {
        target,
        pointer: 6n,
        state: { hovered: false, pressed: true, captured: false },
      },
    ]);
    for (let end = 0; end < inspection.length; end++)
      assert.throws(() =>
        codec.decodeResponse(inspection.subarray(0, end), 7n),
      );
    const invalid = inspection.slice();
    invalid[invalid.length - 1] = 2;
    assert.throws(() => codec.decodeResponse(invalid, 7n));
  });

  const effectIdentity = (identity) =>
    identity === null
      ? u8(0)
      : concatenate(
          u8(1),
          u64(identity.world.id),
          u64(identity.world.incarnation),
          u64(identity.ordinal),
        );
  const effect = (kind, identity = null) =>
    concatenate(
      effectIdentity(identity),
      targetBytes,
      u8(0),
      u64(8n),
      u32(2),
      u64(20n),
      u64(3n),
      kind,
    );

  const observed = (payload, request = 0n) =>
    concatenate(
      u64(7n),
      u64(request),
      u64(0n),
      u8(codec.WIRE.RESPONSE_GUI_OBSERVATION),
      u32(payload.length),
      payload,
    );
  const subscription = concatenate(u64(13n), u64(17n));
  const appliedIdentity = { world, ordinal: 29n };
  const record = (kind, identity = appliedIdentity) =>
    concatenate(subscription, u8(1), effect(kind, identity));
  const observedEffect = record(u8(0));

  test(`${name} observer registration and ordered markers use exact identities without clocks`, () => {
    for (const [classes, tag] of [
      ["application", 0],
      ["feedback", 1],
      ["all", 2],
    ]) {
      const payload = concatenate(
        u8(0),
        u64(world.id),
        u64(world.incarnation),
        u8(tag),
      );
      assert.deepEqual(
        request({
          kind: "guiObservation",
          control: { kind: "subscribe", world, classes },
        }),
        concatenate(
          header,
          u8(codec.WIRE.REQUEST_GUI_OBSERVATION),
          u32(payload.length),
          payload,
        ),
      );
    }
    const payload = concatenate(
      u8(1),
      u64(world.id),
      u64(world.incarnation),
      subscription,
    );
    assert.deepEqual(
      request({
        kind: "guiObservation",
        control: {
          kind: "unsubscribe",
          world,
          subscription: { output: 13n, generation: 17n },
        },
      }),
      concatenate(
        header,
        u8(codec.WIRE.REQUEST_GUI_OBSERVATION),
        u32(payload.length),
        payload,
      ),
    );
    for (const [tag, result] of [
      "subscribed",
      "unsubscribed",
      "cancelled",
      "staleWorld",
      "staleSubscription",
      "alreadySubscribed",
    ].entries()) {
      const response = codec.decodeResponse(
        observed(
          concatenate(
            subscription,
            u8(0),
            u64(world.id),
            u64(world.incarnation),
            u8(tag),
          ),
          9n,
        ),
        7n,
      );
      assert.equal(response.tick, 0n);
      assert.deepEqual(response.body.record, {
        kind: "control",
        world,
        subscription: { output: 13n, generation: 17n },
        result,
      });
    }
    const response = codec.decodeResponse(observed(observedEffect), 7n);
    assert.equal(response.tick, 0n);
    assert.equal(response.requestId, 0n);
    assert.deepEqual(response.body.record.effect.id, appliedIdentity);
    assert.deepEqual(response.body.record.effect.ancestry, [20n, 3n]);
    assert.equal(response.body.record.effect.source, "semantic");
  });

  test(`${name} observer decoder rejects incomplete records and forged applied provenance`, () => {
    const bytes = observed(observedEffect);
    for (let end = 0; end < bytes.length; end++)
      assert.throws(() => codec.decodeResponse(bytes.subarray(0, end), 7n));
    for (const payload of [
      concatenate(u64(0n), observedEffect.subarray(8)),
      concatenate(subscription, u8(2)),
      concatenate(
        subscription,
        u8(0),
        u64(world.id),
        u64(world.incarnation),
        u8(6),
      ),
      record(u8(0), null),
      record(u8(0), {
        world: { id: 999n, incarnation: 2n },
        ordinal: 29n,
      }),
      concatenate(observedEffect, u8(0)),
    ])
      assert.throws(() => codec.decodeResponse(observed(payload), 7n));
  });

  test(`${name} effects preserve focus results, submitted text and exact ancestry`, () => {
    for (const focused of [false, true]) {
      const result = codec.decodeResponse(
        observed(record(concatenate(u8(1), u8(Number(focused)), u8(1)))),
        7n,
      ).body.record.effect;
      assert.deepEqual(result.effect, {
        kind: "focusChanged",
        focused,
        changed: true,
      });
      assert.deepEqual(result.id, appliedIdentity);
      assert.deepEqual(result.ancestry, [20n, 3n]);
      assert.deepEqual(result.target, target);
    }
    assert.deepEqual(
      codec.decodeResponse(
        observed(record(concatenate(u8(4), text("é🙂")))),
        7n,
      ).body.record.effect.effect,
      { kind: "submitted", text: "é🙂" },
    );
    assert.throws(() =>
      codec.decodeResponse(
        observed(record(concatenate(u8(4), u32(6), u8(1)))),
        7n,
      ),
    );
    // Effect kinds 2 (value applied) and 5 (scroll changed) are retired:
    // values are observed as component fields.
    for (const kind of [
      concatenate(u8(2), u8(1)),
      u8(5),
      concatenate(u8(1), u8(2), u8(0)),
    ])
      assert.throws(() => codec.decodeResponse(observed(record(kind)), 7n));
    const bytes = observed(record(concatenate(u8(1), u8(1), u8(1))));
    assert.throws(() => codec.decodeResponse(concatenate(bytes, u8(0)), 7n));
    assert.throws(() => codec.decodeResponse(bytes, 8n));
  });
}
