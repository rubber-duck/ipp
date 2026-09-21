// Maintained target runner: execute the actual WASM module and compare native fixtures.
// Usage: node target-contract.mjs NATIVE.contract NATIVE.fixture HOST.wasm OUTPUT_DIR
import assert from "node:assert/strict";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
const [nativePath, fixturePath, wasmPath, out, minimalPath] =
  process.argv.slice(2);
if (!out)
  throw new Error(
    "usage: target-contract.mjs NATIVE.contract NATIVE.fixture HOST.wasm OUTPUT_DIR",
  );
await mkdir(out, { recursive: true });
const module = await WebAssembly.compile(await readFile(wasmPath));
const imports = {};
// Contract export is headless even when this target also includes rendering.
// Throwing imports prove that inspecting target layouts performs no host I/O.
for (const item of WebAssembly.Module.imports(module)) {
  assert.ok(["ipp_gl", "ipp_diagnostics"].includes(item.module));
  assert.equal(item.kind, "function");
  imports[item.module] ??= {};
  imports[item.module][item.name] = () => {
    throw new Error("Contract export attempted host I/O");
  };
}
const instance = await WebAssembly.instantiate(module, imports);
const e = instance.exports;
assert.equal(e.ipp_fixture_check(), 1, "WASM exact typed and owned writes");
// Calling len may initialize/grow memory, so only obtain the buffer after both calls.
function readExport(ptrFn, lenFn) {
  const ptr = ptrFn();
  const len = lenFn();
  return new Uint8Array(e.memory.buffer, ptr, len).slice();
}
const wasm = readExport(e.ipp_contract_ptr, e.ipp_contract_len);
const fixture = readExport(e.ipp_fixture_ptr, e.ipp_fixture_len);
await writeFile(path.join(out, "wasm.contract"), wasm);
await writeFile(path.join(out, "wasm.fixture"), fixture);
function reader(input) {
  let at = 0;
  const view = new DataView(input.buffer, input.byteOffset, input.byteLength);
  return {
    u8() {
      return view.getUint8(at++);
    },
    u16() {
      const v = view.getUint16(at, true);
      at += 2;
      return v;
    },
    u32() {
      const v = view.getUint32(at, true);
      at += 4;
      return v;
    },
    u64() {
      const v = view.getBigUint64(at, true);
      at += 8;
      return v;
    },
    f32() {
      const v = view.getFloat32(at, true);
      at += 4;
      return v;
    },
    raw(n) {
      const v = input.slice(at, at + n);
      assert.equal(v.length, n);
      at += n;
      return v;
    },
    string() {
      return new TextDecoder("utf-8", { fatal: true }).decode(
        this.raw(this.u32()),
      );
    },
    done() {
      assert.equal(at, input.length);
    },
  };
}
function component(r) {
  const name = r.string(),
    size = r.u32(),
    alignment = r.u32(),
    count = r.u16(),
    creatable = r.u8() === 1;
  const fields = {};
  for (let i = 0; i < count; i++) {
    const name = r.string(),
      offset = r.u32(),
      size = r.u32(),
      alignment = r.u32(),
      kind = r.u8();
    let value;
    if (!creatable) value = undefined;
    else if (kind === 1) value = r.f32();
    else if (kind === 2 || kind === 4) value = r.u64();
    else if (kind === 3) value = r.u32();
    else if (kind === 5) value = r.string();
    else if (kind === 6) value = [...r.raw(r.u32())];
    else if (kind === 7) {
      const v = r.u8();
      assert.ok(v <= 1);
      value = v === 1;
    } else throw new Error("unknown fixture kind");
    fields[name] = { offset, size, alignment, kind, default: value };
  }
  return { name, size, alignment, creatable, fields };
}
function contract(bytes) {
  assert.equal(new TextDecoder().decode(bytes.slice(0, 4)), "IPPB");
  const r = reader(bytes.slice(4));
  assert.equal(r.u32(), 2);
  const hash = r.u64();
  let actual = 0xcbf29ce484222325n;
  for (const b of bytes.slice(16))
    actual = BigInt.asUintN(64, (actual ^ BigInt(b)) * 0x100000001b3n);
  assert.equal(hash, actual);
  assert.equal(r.u16(), 4);
  const arch = r.string(),
    os = r.string(),
    pointerBits = r.u8();
  const features = [];
  for (let i = 0, n = r.u8(); i < n; i++)
    features.push({ id: r.u8(), enabled: r.u8() === 1, name: r.string() });
  const count = r.u16();
  const components = [];
  for (let i = 0; i < count; i++) {
    const entry = { id: r.u16(), ...component(r) };
    const dynamicProperties = r.u8();
    assert.ok(dynamicProperties <= 1);
    components.push({ ...entry, dynamicProperties: dynamicProperties === 1 });
  }
  assert.equal(r.u16(), 1);
  const capabilities = [];
  for (let i = 0, n = r.u8(); i < n; i++)
    capabilities.push({ id: r.u8(), enabled: r.u8() === 1, name: r.string() });
  const conventions = [];
  for (let i = 0, n = r.u16(); i < n; i++)
    conventions.push([r.string(), r.string()]);
  const layouts = [];
  for (let i = 0, n = r.u16(); i < n; i++) {
    const name = r.string(),
      capability = r.u8(),
      fields = [];
    for (let j = 0, count = r.u16(); j < count; j++)
      fields.push({
        name: r.string(),
        encoding: r.u8(),
        limit: r.u32(),
        target: r.string(),
      });
    layouts.push({ name, capability, fields });
  }
  const tags = [];
  for (let i = 0, n = r.u16(); i < n; i++)
    tags.push({
      name: r.string(),
      space: r.u8(),
      capability: r.u8(),
      value: r.u8(),
      layout: r.string(),
    });
  const assetFormats = [];
  for (let i = 0, n = r.u16(); i < n; i++)
    assetFormats.push({
      name: r.string(),
      capability: r.u8(),
      typeId: r.u16(),
      format: r.string(),
    });
  const reasons = [];
  for (let i = 0, n = r.u16(); i < n; i++) reasons.push(r.string());
  r.done();
  return {
    hash,
    arch,
    os,
    pointerBits,
    features,
    components,
    capabilities,
    conventions,
    layouts,
    tags,
    assetFormats,
    reasons,
  };
}
function fixtureContract(bytes) {
  const r = reader(bytes);
  assert.equal(new TextDecoder().decode(r.raw(4)), "IPPF");
  const pointerBits = r.u8(),
    layout = component(r),
    bound = component(r);
  r.done();
  return { pointerBits, ...layout, bound };
}
const native = contract(await readFile(nativePath)),
  target = contract(wasm);
assert.equal(target.arch, "wasm32");
assert.equal(target.pointerBits, 32);
assert.equal(
  BigInt.asUintN(64, e.ipp_schema_hash()),
  target.hash,
  "final host hash agrees with export",
);
assert.deepEqual(target.features, native.features);
assert.notEqual(target.hash, native.hash);
assert.deepEqual(
  target.components.map((c) => [c.id, c.name]),
  native.components.map((c) => [c.id, c.name]),
);
assert.deepEqual(target.tags, native.tags);
assert.deepEqual(target.capabilities, native.capabilities);
assert.deepEqual(target.conventions, native.conventions);
assert.deepEqual(target.layouts, native.layouts);
assert.deepEqual(target.assetFormats, native.assetFormats);
assert.deepEqual(target.reasons, native.reasons);
for (const schema of [native, target]) {
  for (const entry of schema.components) {
    assert.equal(
      entry.dynamicProperties,
      ["CustomMaterial", "Surface", "GuiRoot"].includes(entry.name),
    );
    if (["ParticleEmitter", "ParticlePlayback"].includes(entry.name)) {
      assert.equal(Object.hasOwn(entry.fields, "runtime"), false);
    }
  }
}
assert.equal(target.components[0].fields.value.default, 0);
for (const name of ["MeshInstance", "UnlitTexture"]) {
  const nativeComponent = native.components.find((c) => c.name === name);
  const targetComponent = target.components.find((c) => c.name === name);
  if (!nativeComponent || !targetComponent) continue;
  for (const component of [nativeComponent, targetComponent]) {
    assert.deepEqual(Object.keys(component.fields), ["source", "variant"]);
    assert.equal(component.fields.source.kind, 5);
    assert.equal(component.fields.source.default, "");
    assert.equal(component.fields.variant.default, 0);
  }
  if (native.pointerBits === 64) {
    assert.notEqual(
      nativeComponent.fields.source.size,
      targetComponent.fields.source.size,
    );
    assert.notEqual(
      nativeComponent.fields.variant.offset,
      targetComponent.fields.variant.offset,
    );
  }
}
const nf = fixtureContract(await readFile(fixturePath)),
  wf = fixtureContract(fixture);
assert.equal(wf.pointerBits, 32);
assert.equal(Object.hasOwn(wf.fields, "internal"), false);
for (const f of [nf, wf]) {
  assert.equal(f.creatable, true);
  assert.equal(f.bound.creatable, false);
  assert.deepEqual(Object.keys(f.bound.fields), ["label"]);
  assert.equal(f.bound.fields.label.default, undefined);
  assert.equal(f.fields.marker.default, 17);
  assert.equal(f.fields.label.default, "fixture");
  assert.deepEqual(f.fields.bytes.default, [1, 2, 3]);
  assert.equal(f.fields.visible.kind, 7);
  assert.equal(f.fields.visible.default, true);
  assert.equal(f.fields.source.default, 0x1_0000_0007n);
  assert.equal(f.fields.value.default, 1.25);
}
if (nf.pointerBits === 64) {
  assert.notEqual(
    nf.fields.value.offset,
    wf.fields.value.offset,
    "ignored pointer changes target field offsets",
  );
  assert.notEqual(
    nf.fields.label.size,
    wf.fields.label.size,
    "owned type sizes come from each target",
  );
}
if (minimalPath) {
  const minimal = contract(await readFile(minimalPath));
  assert.notEqual(
    minimal.hash,
    native.hash,
    "feature selection participates in compatibility",
  );
  assert.deepEqual(
    minimal.features.find((feature) => feature.name === "skeletal-animation"),
    { id: 16, enabled: false, name: "skeletal-animation" },
  );
  assert.deepEqual(
    native.features.find((feature) => feature.name === "skeletal-animation"),
    { id: 16, enabled: true, name: "skeletal-animation" },
  );
  assert.equal(
    minimal.components.some((c) => c.name === "Skeleton"),
    false,
  );
  assert.equal(
    native.components.some((c) => c.name === "Skeleton"),
    true,
  );
  assert.equal(
    native.components.some((c) => c.name === "Skin"),
    true,
  );
}
const report = { native, wasm: target, nativeFixture: nf, wasmFixture: wf };
await writeFile(
  path.join(out, "target-report.json"),
  JSON.stringify(
    report,
    (_, v) => (typeof v === "bigint" ? v.toString() : v),
    2,
  ),
);
console.log(
  "Native/WASM executed contracts, final hash, defaults, ignored/interior/type rejection and owned writes passed.",
);
