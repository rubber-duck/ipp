# Blender Integration Strategy

[Authoring architecture](../architecture/authoring.md) · [Runtime strategy](runtime-and-rendering.md) · [Development environment](../development/blender.md)

## Translation approach

Extend the real addon → browser adapter → worker/WASM/WebGL path with compact generated scenes and a licensed character fixture. Inspect the [exporter](../../integrations/blender/ipp_blender/EXPORTER.md) and receiving runtime before extending translation: runtime capability alone does not prove export fidelity.

Keep extraction, identity/diffing, detached conversion, server lifecycle and UI adapters separable. Explicit export and automatic updates use the same logic. Start with asymmetric geometry and an explicit camera to expose coordinate, winding and baking errors; add materials, rigs, attachments and deformation against independently sampled Blender references.

Follow the [single evaluation owner](../architecture/authoring.md#single-evaluation-owner) contract. Preserve supported local relationships and operation inputs; diagnose or deliberately bake unsupported semantics. Use ordinary producer batches, generated target contracts and immutable asset references. Exporter source owns exact translation rules.

## Main-thread execution and transport

Use detached values and release temporary Blender data before asynchronous work. Dirty handlers schedule extraction; GUI callbacks yield and background runners explicitly drive the same export/network loop. Measure callback latency and audit libraries for hidden executors before offloading compute.

Extend the existing HTTPS/WSS server and browser adapter together. Test immutable revision identity, partial application, backpressure and session fences through actual network delivery. Runtime handles and export identities have different lifetimes. Reuse the [certificate/onboarding procedure](../development/blender.md#local-certificates-and-viewer-onboarding); automated trust does not prove interactive browser onboarding.

## Reusable actions and disk output

Export reusable clips independently of current playback and restore temporary sampling state on every exit path. Use runtime hierarchy for attachments and leave application routes/interactions with the application. Disk publication reuses exporter bytes, target-correct adapter translation and ordinary reference-only World serialization; companion metadata describes asset associations and presentation selections.

Use the [KayKit platformer](../../examples/world-gallery/worlds/platformer/README.md) as a maintained authored-scene fixture with licensed provenance and reproducible Blender assembly. Export ordinary scene and animation assets while leaving route interaction to the application. Generate runtime exports from the packed source during the gallery build, using the receiving target contract. Validate the result through the generated-client gallery harness without requiring third-party downloads; Blender remains a build-time dependency.

## Particle implementation approach

Extend supported native recipes or sequential baked-cache export through ordinary immutable assets and animation tracks. Compare against independently sampled Blender particles, verify authoring timeline restoration and observe viewer frames while seeking in both directions. Keep unsupported semantics explicit under the [particle export contract](../architecture/authoring.md#particle-export).

## Maintained integration harness

Extend the `blender` and `particles-blender` [suites](../../tools/pipeline/suites.json) with isolated real Blender/addon and Chromium participants. Reuse local generated geometry/texture/rig fixtures, [shared scenarios](../../tests/integration/scenarios), [browser drivers](../../tests/browser/environment.ts) and [frame assertions](../../tests/render/image-assertions.ts).

| Evidence | Observable result |
| --- | --- |
| Export/edits | Stable identities, immutable revisions, correct converted state and changed pixels |
| Readiness/failure | Pending resources recover without resubmission; failed revisions retain applied scope |
| Ownership/sessions | Blender base survives overlay removal; stale export/runtime work is rejected |
| Transport | Slow reads, cancellation, denied origins and real trust flows remain observable |
| Cleanup | Disable, restart and exit settle work and release owned listeners, connections and processes |

Await listener, export/session, committed scene, camera/resource readiness and completed frames under the [testing policy](../development/integration-testing.md). Keep scenario intent independent of launch and wire details. GUI runners add editor/timer/undo evidence; background execution does not establish responsiveness. Software rendering proves its environment, while hardware profiles must verify actual renderer identity.

The [development guide](../development/blender.md) owns installation, virtual-display and MCP tooling. MCP remains an interactive aid outside production/CI; useful experiments become maintained scenarios. Recorded exports and Blender thumbnails aid diagnosis but do not replace real addon/viewer evidence.

For the fresh-import stream, compare the same real Blender scene with a refreshed full snapshot and bounded entity/clip groups over WSS. Preserve shared animation sampling, service detached I/O at extraction checkpoints, and measure first acknowledgement, final export, applied revision and resource-ready capture separately. Extend the existing browser driver with cancellation, dependency-order and corrected-revision checks; retain hardware renderer identity and equal final image/state evidence.

Bound command storage independently of import size: generate adapter commands incrementally, preserve aliases across pipelined buffers within a Host-issued logical batch, and check retained decoder capacity at the protocol limit. Merge compatible Blender sample jobs into a shared world-frame schedule; validate authored fractional samples and sequential particle simulation against fresh-file references, retaining isolated passes where evaluation bindings or cadence differ.

Exercise chained command buffers over the existing native WebSocket and browser worker drivers. Assert no intermediate evaluation or presentation, same-World queue isolation, healthy peer-World progress, Host-time expiry, stale identity rejection and correction of retained partial effects. Gate real asset delivery independently to verify that command completion does not wait for baking or loading.

Build the pending source index before command delivery and complete the entity batch before running producers. Verify geometry and texture publication precedes shared animation sampling, that declared references recover through source-provider notifications without resubmission, and that cancellation settles pending sources while preserving completed immutable content. Exercise the same pending/ready/failed source channel independently of rendering; browser coverage must assert both pending observations and final images.
