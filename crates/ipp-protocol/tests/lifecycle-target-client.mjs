import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";

const u8 = (value) => Uint8Array.of(value);
const integer = (size, value) => {
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  if (size === 8) view.setBigUint64(0, value, true);
  else if (size === 4) view.setUint32(0, value, true);
  else view.setUint16(0, value, true);
  return bytes;
};
const u64 = (value) => integer(8, value);
const u32 = (value) => integer(4, value);
const u16 = (value) => integer(2, value);
const join = (...parts) =>
  Uint8Array.from(parts.flatMap((bytes) => [...bytes]));
const world = { id: 3n, incarnation: 5n };
const worldBytes = join(u64(3n), u64(5n));

for (const target of ["native", "wasm"]) {
  const codec = await import(
    pathToFileURL(resolve(`target/world-host-build/${target}/generated.js`))
      .href
  );
  const request = (control) =>
    codec.encodeRequest({
      session: 7n,
      requestId: 2n,
      body: { kind: "lifecycleWatch", control: { world, ...control } },
    });
  const response = (requestId, record) =>
    join(u64(7n), u64(requestId), u64(0n), u8(37), worldBytes, u64(9n), record);
  test(`${target} lifecycle membership bytes and malformed pages`, () => {
    assert.deepEqual(
      request({
        kind: "add",
        targets: [{ target: { kind: "entity", entity: 99n }, kinds: 4 }],
      }),
      join(
        u64(7n),
        u64(2n),
        u8(35),
        worldBytes,
        u8(0),
        u32(1),
        u8(0),
        u64(99n),
        u8(4),
      ),
    );
    assert.deepEqual(
      request({ kind: "remove", output: 9n, generations: [2n, 6n] }),
      join(
        u64(7n),
        u64(2n),
        u8(35),
        worldBytes,
        u8(1),
        u64(9n),
        u32(2),
        u64(2n),
        u64(6n),
      ),
    );
    for (const generations of [[], [2n, 2n], [6n, 2n], [0n]])
      assert.throws(() => request({ kind: "remove", output: 9n, generations }));
    for (const kinds of [0, 8, 255])
      assert.throws(() =>
        request({
          kind: "add",
          targets: [{ target: { kind: "entity", entity: 99n }, kinds }],
        }),
      );
  });
  test(`${target} frozen baseline, cancellation and incarnation validation`, () => {
    const baseline = join(
      u8(0),
      u8(0),
      u8(1),
      u64(6n),
      u64(8n),
      u8(0),
      u32(1),
      u64(4n),
      u8(1),
      u64(99n),
      u16(1),
      u8(1),
      u8(1),
      u64(12n),
    );
    const bytes = response(2n, baseline);
    const decoded = codec.decodeResponse(bytes, 7n);
    assert.equal(decoded.tick, 0n);
    assert.deepEqual(decoded.body.record.result.baselines, [
      {
        member: { output: 9n, generation: 4n },
        target: { kind: "component", entity: 99n, component: 1 },
        lifetime: { kind: "component", entityLive: true, incarnation: 12n },
      },
    ]);
    for (let length = 0; length < bytes.length; length++)
      assert.throws(() => codec.decodeResponse(bytes.subarray(0, length), 7n));
    assert.throws(() => codec.decodeResponse(join(bytes, u8(0)), 7n));
    assert.throws(() => codec.decodeResponse(bytes, 8n));
    assert.throws(() => codec.decodeResponse(response(0n, baseline), 7n));
    assert.equal(
      codec.decodeResponse(response(2n, join(u8(0), u8(1), u8(0), u8(2))), 7n)
        .body.record.result.kind,
      "cancelled",
    );
    // Tag 2 was the removed terminal overflow record; it no longer decodes.
    assert.throws(() =>
      codec.decodeResponse(response(0n, join(u8(2), u64(129n))), 7n),
    );
    const event = join(
      u8(1),
      u64(4n),
      u64(7n),
      u64(8n),
      u8(codec.WIRE.LIFECYCLE_COMPONENT_REPLACED),
      u64(99n),
      u16(1),
      u64(11n),
      u64(12n),
    );
    assert.equal(
      codec.decodeResponse(response(0n, event), 7n).body.record.observation
        .previousIncarnation,
      11n,
    );
    event.fill(0, event.length - 8);
    assert.throws(() => codec.decodeResponse(response(0n, event), 7n));
  });
  test(`${target} value targets, baselines and records`, () => {
    const valueTarget = {
      kind: "value",
      entity: 99n,
      component: 1,
      fields: [0, 12],
    };
    const encodedTarget = join(
      u8(2),
      u64(99n),
      u16(1),
      u32(2),
      u32(0),
      u32(12),
    );
    assert.deepEqual(
      request({
        kind: "add",
        targets: [{ target: valueTarget, kinds: 128 }],
      }),
      join(
        u64(7n),
        u64(2n),
        u8(35),
        worldBytes,
        u8(0),
        u32(1),
        encodedTarget,
        u8(128),
      ),
    );
    // Value targets select only value changes over ascending schema offsets.
    for (const [fields, kinds] of [
      [[0, 12], 16],
      [[0, 12], 129],
      [[], 128],
      [[12, 12], 128],
      [[12, 0], 128],
    ])
      assert.throws(() =>
        request({
          kind: "add",
          targets: [{ target: { ...valueTarget, fields }, kinds }],
        }),
      );
    for (const kinds of [128, 136])
      assert.throws(() =>
        request({
          kind: "add",
          targets: [{ target: { kind: "entity", entity: 99n }, kinds }],
        }),
      );

    // The sole ACK echoes the whole value target with its component lifetime.
    const baseline = join(
      u8(0),
      u8(0),
      u8(1),
      u64(6n),
      u64(8n),
      u8(0),
      u32(1),
      u64(4n),
      encodedTarget,
      u8(1),
      u8(1),
      u64(12n),
    );
    assert.deepEqual(
      codec.decodeResponse(response(2n, baseline), 7n).body.record.result
        .baselines,
      [
        {
          member: { output: 9n, generation: 4n },
          target: valueTarget,
          lifetime: { kind: "component", entityLive: true, incarnation: 12n },
        },
      ],
    );

    // Values arrive as snapshot fields in the target's order; absence is null.
    const text = new TextEncoder().encode("é");
    const present = join(
      u8(3),
      u64(4n),
      u64(8n),
      u8(1),
      u32(2),
      u32(0),
      u8(codec.WIRE.SNAPSHOT_VALUE_F32),
      new Uint8Array(new Float32Array([1.5]).buffer),
      u32(12),
      u8(codec.WIRE.SNAPSHOT_VALUE_STRING),
      u32(text.length),
      text,
    );
    const record = codec.decodeResponse(response(0n, present), 7n).body.record;
    assert.equal(record.kind, "value");
    assert.deepEqual(record.member, { output: 9n, generation: 4n });
    assert.equal(record.tick, 8n);
    assert.deepEqual(record.values, [
      { offset: 0, value: 1.5 },
      { offset: 12, value: "é" },
    ]);
    for (let length = 0; length < present.length; length++)
      assert.throws(() =>
        codec.decodeResponse(response(0n, present.subarray(0, length)), 7n),
      );
    const absent = join(u8(3), u64(4n), u64(9n), u8(0));
    assert.equal(
      codec.decodeResponse(response(0n, absent), 7n).body.record.values,
      null,
    );
    for (const invalid of [
      join(u8(3), u64(0n), u64(9n), u8(0)),
      join(u8(3), u64(4n), u64(9n), u8(2)),
    ])
      assert.throws(() => codec.decodeResponse(response(0n, invalid), 7n));
  });
}
