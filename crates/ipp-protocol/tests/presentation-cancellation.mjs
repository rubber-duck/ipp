import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { encodeManifestLayout, manifestVariant } from "./generated-client.mjs";

const root = resolve(import.meta.dirname, "../../..");
const product = resolve(root, "target/presentation-wire-host");
const executable = resolve(
  product,
  process.platform === "win32" ? "host-tests.exe" : "host-tests",
);
const filter = "services::presentation::tests::connection_cancel_";
const digest = (value) => createHash("sha256").update(value).digest("hex");

function sourceIdentity() {
  const paths = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo"];
  for (const crate of [
    "crates/ipp-core",
    "crates/ipp-protocol",
    "crates/ipp-host-session",
    "tools/ipp-schema-derive",
    "tools/ipp-schema-gen",
  ])
    paths.push(crate);
  const files = [];
  const visit = (name) => {
    const path = resolve(root, name);
    if (statSync(path).isDirectory()) {
      for (const entry of readdirSync(path).sort()) visit(`${name}/${entry}`);
    } else files.push([name, digest(readFileSync(path))]);
  };
  for (const name of paths.sort()) visit(name);
  return digest(JSON.stringify(files));
}

function environmentIdentity() {
  return Object.fromEntries(
    [
      "RUSTFLAGS",
      "CARGO_ENCODED_RUSTFLAGS",
      "CARGO_BUILD_TARGET",
      "RUSTUP_TOOLCHAIN",
    ].map((name) => [name, process.env[name] ?? null]),
  );
}

function buildHost() {
  mkdirSync(product, { recursive: true });
  const source = sourceIdentity();
  const result = spawnSync(
    "cargo",
    [
      "test",
      "-p",
      "ipp-host-session",
      "--no-default-features",
      "--lib",
      "--no-run",
      "--locked",
      "--message-format=json",
    ],
    {
      cwd: root,
      encoding: "utf8",
      timeout: 180_000,
      maxBuffer: 32 * 1024 * 1024,
    },
  );
  writeFileSync(resolve(product, "build.stdout.log"), result.stdout ?? "");
  writeFileSync(resolve(product, "build.stderr.log"), result.stderr ?? "");
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  const binaries = result.stdout
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line))
    .filter(
      (message) =>
        message.reason === "compiler-artifact" &&
        message.target.name === "ipp_host_session" &&
        message.profile.test &&
        message.executable,
    );
  assert.equal(binaries.length, 1, "Missing unique Rust Host test executable");
  assert.equal(
    sourceIdentity(),
    source,
    "Sources changed during Host test compilation",
  );
  copyFileSync(binaries[0].executable, executable);
  writeFileSync(
    resolve(product, "identity.json"),
    JSON.stringify(
      {
        source,
        binary: digest(readFileSync(executable)),
        environment: environmentIdentity(),
      },
      null,
      2,
    ) + "\n",
  );
}

function checkedProduct() {
  const identity = JSON.parse(
    readFileSync(resolve(product, "identity.json"), "utf8"),
  );
  assert.equal(
    identity.source,
    sourceIdentity(),
    "Stale Host proof build; use the maintained manifest-codec regression selection",
  );
  assert.equal(
    identity.binary,
    digest(readFileSync(executable)),
    "Host proof executable changed",
  );
  assert.deepEqual(
    identity.environment,
    environmentIdentity(),
    "Host proof build environment changed",
  );
  return identity;
}

function union(client, name, space, spaceId, fields = {}) {
  const descriptor = client.manifest.WIRE_TAG_LAYOUTS[name];
  assert.equal(descriptor.space, spaceId);
  assert.deepEqual(client.manifest.WIRE_LAYOUTS[descriptor.layout].fields[0], {
    name: "tag",
    encoding: "variant",
    target: space,
    limit: 0,
  });
  const encoded = encodeManifestLayout(client, descriptor.layout, {
    tag: { space, value: client.codec.WIRE[name] },
    ...fields,
  });
  return { layout: space, bytes: encoded.bytes };
}

function hostEnvelope(client, response, request, body) {
  return encodeManifestLayout(
    client,
    response ? "host-response-presentation" : "host-request-presentation",
    {
      magic: response ? 0x0000000241505049n : 0x0000000248505049n,
      connection: 7n,
      request_id: request,
      tag: manifestVariant(
        client,
        response ? "HOST_RESPONSE_PRESENTATION" : "HOST_REQUEST_PRESENTATION",
      ),
      body,
    },
  ).bytes;
}

function verifyTranscript(client, transcript, identity, cancel) {
  assert.deepEqual(
    transcript.identity,
    identity,
    "Host proof transcript is missing or stale",
  );
  assert.equal(
    transcript.schemaHash,
    client.manifest.SCHEMA_HASH.toString(),
    "Rust Host and current manifest differ",
  );
  assert.deepEqual(transcript.cancel, [...cancel]);
  assert.deepEqual(transcript.captureReservation, [24, 0]);
  assert.deepEqual(transcript.replyEntries, [2, 2, 2, 1, 0]);
  const charges = transcript.replyBytes;
  assert.equal(charges.length, 5);
  assert.ok(charges[0] > 0);
  assert.equal(charges[1], charges[0]);
  assert.equal(charges[2], charges[0]);
  assert.ok(charges[3] > 0 && charges[3] < charges[0]);
  assert.equal(charges[4], 0);
  const fields = transcript.view.map(BigInt);
  assert.equal(fields.length, 13);
  assert.deepEqual(fields.slice(0, 4), [1n, 1n, 4096n, 4096n]);
  const layout = (name, values) => encodeManifestLayout(client, name, values);
  const view = layout("presentation-view", {
    surface: layout("presentation-surface", {
      id: fields[0],
      context: fields[1],
      max_width: 4096,
      max_height: 4096,
    }),
    selection: fields[4],
    binding: layout("root-binding", {
      output: layout("output-reference", {
        world: layout("world-reference", {
          id: fields[5],
          incarnation: fields[6],
        }),
        target: union(client, "OUTPUT_TARGET_CAMERA", "output-target", 54, {
          entity: fields[7],
          incarnation: fields[8],
        }),
      }),
      width: Number(fields[9]),
      height: Number(fields[10]),
      device_pixel_ratio: 1,
      generation: layout("presentation-identity", {
        host: fields[11],
        serial: fields[12],
      }),
    }),
  });
  assert.deepEqual(fields.slice(9, 11), [2n, 3n]);
  const frame = union(
    client,
    "PRESENTATION_REQUEST_FRAME",
    "presentation-request",
    36,
    {
      view,
      after_sequence: 0xffffffffffffffffn,
      publication: null,
      capture: true,
      after_outputs: [],
    },
  );
  assert.deepEqual(transcript.frame, [
    ...hostEnvelope(client, false, 1000n, frame),
  ]);
  const complete = union(
    client,
    "PRESENTATION_RESPONSE_COMPLETE",
    "presentation-response",
    37,
  );
  assert.deepEqual(transcript.complete, [
    ...hostEnvelope(client, true, 1001n, complete),
  ]);
  const unavailable = union(
    client,
    "PRESENTATION_RESPONSE_ERROR",
    "presentation-response",
    37,
    {
      error: union(
        client,
        "PRESENTATION_ERROR_UNAVAILABLE",
        "presentation-error",
        38,
      ),
    },
  );
  assert.deepEqual(transcript.unavailable, [
    ...hostEnvelope(client, true, 1000n, unavailable),
  ]);
}

export function executeCancellationConformance(client) {
  const built = checkedProduct();
  const parent = resolve(root, "target/presentation-wire-runs");
  mkdirSync(parent, { recursive: true });
  const directory = mkdtempSync(resolve(parent, "case-"));
  const identity = {
    ...built,
    manifest: digest(client.manifestSource),
    nonce: randomUUID(),
  };
  const request = union(
    client,
    "PRESENTATION_REQUEST_CANCEL_FRAME",
    "presentation-request",
    36,
    { request: 1000n },
  );
  const cancel = hostEnvelope(client, false, 1001n, request);
  writeFileSync(resolve(directory, "request.bin"), cancel);
  writeFileSync(resolve(directory, "identity.json"), JSON.stringify(identity));
  const result = spawnSync(
    executable,
    [filter, "--test-threads=1", "--nocapture"],
    {
      cwd: root,
      env: { ...process.env, IPP_PRESENTATION_WIRE_CASE: directory },
      encoding: "utf8",
      timeout: 60_000,
    },
  );
  writeFileSync(resolve(directory, "stdout.log"), result.stdout ?? "");
  writeFileSync(resolve(directory, "stderr.log"), result.stderr ?? "");
  assert.ifError(result.error);
  assert.equal(
    result.status,
    0,
    `Rust Host cancellation failed; ${directory}\n${result.stdout}\n${result.stderr}`,
  );
  assert.match(result.stdout, /test result: ok\. 4 passed; 0 failed;/);
  const transcript = JSON.parse(
    readFileSync(resolve(directory, "transcript.json"), "utf8"),
  );
  assert.deepEqual(
    checkedProduct(),
    built,
    "Sources or executable changed during the Host proof",
  );
  verifyTranscript(client, transcript, identity, cancel);
  for (const invalid of [
    {},
    { ...transcript, identity: { ...identity, nonce: "old-run" } },
    { ...transcript, identity: { ...identity, source: "old-source" } },
    { ...transcript, identity: { ...identity, manifest: "old-manifest" } },
    { ...transcript, schemaHash: "0" },
    { ...transcript, cancel: [...cancel, 0] },
    { ...transcript, complete: transcript.unavailable },
    { ...transcript, unavailable: transcript.complete },
    { ...transcript, captureReservation: [24, 24] },
    { ...transcript, replyEntries: [2, 0] },
    { ...transcript, replyBytes: [0, 0, 0, 0, 0] },
  ])
    assert.throws(() => verifyTranscript(client, invalid, identity, cancel));
  writeFileSync(
    resolve(directory, "verified.json"),
    JSON.stringify(
      {
        identity,
        tag: "PRESENTATION_REQUEST_CANCEL_FRAME",
        tests: 4,
        boundary:
          "Rust Host wire; deterministic rendering-edge provider; not transport or rendering evidence",
      },
      null,
      2,
    ) + "\n",
  );
  console.log(`Host cancellation transcript verified: ${directory}`);
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  assert.deepEqual(process.argv.slice(2), ["--build"]);
  buildHost();
}
