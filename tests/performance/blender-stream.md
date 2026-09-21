# Blender streaming measurements

[Performance harnesses](README.md) · [Blender development](../../docs/development/blender.md)

The maintained [stream scenario](../render/blender-stream.test.ts) compares a refreshed full export with a streamed export into a fresh browser World. Both paths use real Blender, HTTPS/WSS, the generated client, worker/WASM and completed WebGL frames. The ordinary Blender suite runs a 2,409-entity capacity fixture, compares exact pixels, and rejects a later chunk before repairing the same World with a full revision. It also repairs a local encoding error after thousands of commands have already committed.

Run the standard suite to build all prerequisites and verify correctness:

```sh
IPP_BROWSER_ANGLE=vulkan ipp-browser-env python tools/ipp.py test blender
```

For the animated 900-cube workload, generate the maintained stress scene and repeat only the scenario after building those prerequisites:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python tests/blender/stress_scene.py -- \
  --output target/stress-benchmark/stream-medium --grid 30

IPP_BROWSER_ANGLE=vulkan \
IPP_BLENDER_STREAM_BLEND=target/stress-benchmark/stream-medium/benchmark.blend \
IPP_BLENDER_STREAM_SIZES=0,100,100,0 \
ipp-browser-env node --test-concurrency=1 dist/tests/render/blender-stream.test.js
```

`0` requests a fresh full export; positive values select the maximum entities or completed clip sources per streamed group. Each navigation creates a fresh runtime. The addon process and its immutable asset store remain warm. Initial Blender startup, file opening, and the server's initial export happen before the measured connection; both compared paths then request another export. This measures export/import overlap, not Blender process startup or loading an already cached snapshot. A configured hardware run rejects software renderer identities.

Evidence goes to `target/integration-artifacts/blender-stream/`: `timings.json`, completed-frame PNGs, build identities and event logs. Browser timings are milliseconds from `connect()`: first applied chunk, receipt/processing of the final snapshot, completed application and observed resource readiness. Clip conversion, entity application and controller setup also record their completion boundaries. For animated stress fixtures, the comparison captures stop autoplay and withdraw native emitters after measuring readiness, so wall-clock particle simulation cannot obscure an import difference. The standard particle suites retain their animated rendering coverage. Server timings are seconds: export including backpressure, first acknowledged data group and time waiting for the bounded window. Applied revision and resource readiness are separate boundaries. Short differences on the compact fixture are timing noise unless repeated larger measurements support them.

## Streaming contract

Call `BlenderAdapter.connect({ streamBatchSize: 100 })`, or append `&stream=100` to the example viewer's connection fragment, to request a fresh streamed import. The default remains full snapshots. `refresh=1` requests a fresh full export for comparison. Streaming currently applies only to a fresh adapter; subsequent authoring edits use ordinary full revisions.

The stream supports **fresh imports**. It builds entity and asset indexes, reserves immutable names, then sends detached entities referencing pending sources. An explicit terminator ends the entity command batch before geometry, texture and animation production. Pending readers subscribe to the source provider's availability channel; completed clips reach the adapter through the revision stream. The final authoritative snapshot supplies presentation, animation associations and reconciliation. Transfer IDs and sequence numbers fence partial work separately from completed revisions. Extraction checkpoints service detached I/O on Blender's main thread; shared animation sampling is not repeated per entity group.

Failure or disconnect stops extraction at its next checkpoint. Sampling restores authoring state, and acknowledged runtime identities and referenced assets survive for a corrected full revision. There is no rollback or implicit retry. Long synchronous extraction can still delay Blender's UI and networking until its next checkpoint; background integration evidence does not establish GUI responsiveness.

## Command storage and shared world sampling

Command pages and retained core command capacity are capped at **256 commands / 128 KiB**, including framing. The shared client writer consumes commands lazily and pipelines up to eight pages, retaining aliases at the Host across the entire logical batch. Applications receive one aggregate result after the explicit terminator; individual page acknowledgements provide internal backpressure and surviving identities. Short and empty pages never terminate the batch. The Host's two-second deadline begins with the first page and never resets on continuations. Timeout retains partial effects and reports an error. Oversized individual commands reject explicitly; bulk assets use the separate data plane.

The default streaming fixture includes 2,100 empty entities in addition to the rendering capacity scene. It asserts a peak of 256 commands, identical rendered output, pending source reads before production completes, and partial-failure repair. Native and worker scenarios additionally assert bounded overlapping writes, byte-limit splits, cross-page aliases, failure operation offsets and correction. This bounds command storage without limiting retained scene identities or immutable assets.

Animation sampling merges compatible active transform, rig, light, shape-key and baked-particle jobs by required frame. Baked particles preserve integer sequential warmup; fractional action ranges run separately when simulation is present. Alternate action bindings use their isolated restoration path. The real Blender `world-sampling` check asserts the evaluation schedule and compares particle samples against a fresh-file simulation, avoiding reuse of exporter-populated caches. Cancellation and reusable action equivalence remain covered by `action-sampling`.
