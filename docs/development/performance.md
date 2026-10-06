# Performance measurement

[Workflow](workflow.md) · [Build guide](building.md) · [Performance workloads](../../tests/performance/README.md) · [Diagnostic contract](../architecture/runtime.md#diagnostic-logging)

Measure an identified workload and build before choosing an optimization. Keep ordinary runtime latency separate from the diagnostics used to explain it, and preserve the scene's observable work. A faster run with missing resources, fewer entities or skipped input is a failed comparison.

## Choose the measurement

| Question | Evidence |
| --- | --- |
| How long does an application update take? | Ordinary release build, generated client and real transport, acknowledgement and completed-frame intervals |
| Which World or System consumes CPU or allocations? | Separate instrumentation capture with semantic Host, World incarnation, composition, System and phase identities |
| Where does browser or driver work occur? | Separate sampled CPU profiles and bounded timeline capture with recorded clock correlation |
| How long does GPU work take? | Available hardware query samples with originating frame/context identities; unsupported or disjoint samples stay explicit |
| What memory is retained? | IPP-accounted resource bytes and separately scoped external observations; allocator requested bytes and WASM capacity answer different questions |
| How large is the delivered build? | Identified distribution/artifact measurements with attribution limits; no default artifact-size ceilings |

The `instrumentation` build gates detailed profiling and testing controls. Ordinary logging and renderer/resource/ingress/lifecycle statistics remain available in every build. Build choice never changes a target's schema identity. Instrumented latency is diagnostic evidence, not an ordinary-build baseline. The [GUI stress guide](../../tests/performance/gui-stress.md) owns its native/browser build selection and report semantics; the [stress guide](../../tests/performance/stress.md) owns other maintained workloads.

## Establish comparable runs

Verify the actual renderer, native release build and compositor arrangement before collecting timing. Hardware rendering in the worker does not prove hardware composition of the browser page. Record source revision and uncommitted identity, runtime/client/asset hashes, selected features, workload, viewport/DPR, warmup, browser/driver, device and power settings. Keep full pipeline run evidence with the report. The shared development Host is a debug, concurrently used development tool and is not a performance baseline.

Run measurements serially. Alternate the compared revisions on the same machine and retain every repeat, including failures, together with system load and thermal observations. Do not compare a quiet run with a contended one or substitute desktop results for a phone. Concurrent graphics correctness runs use the pipeline's software selection; those results prove their software environment only. Missing equipment or a rejected hardware renderer leaves hardware evidence pending.

Await asset readiness and actual completed output before measuring. A command acknowledgement or a statistics counter is not a render-readiness barrier. The [presentation guide](rendering.md) describes completed-frame requests and causal output cuts. Keep pixel readback, PNG encoding, inspection, CPU/heap sampling, memory dumps and trace collection outside ordinary timing windows. Record the interval actually measured: application-to-acknowledgement, application-to-completed-frame, Host CPU and GPU command duration are distinct quantities.

## Interpret diagnostics

Read exported identities, units, clock domains and availability. Never infer a System from its current schedule position or compare anonymous offsets across builds. A recreated World is a new incarnation even if a client gives it the same name. Shared Host or renderer work remains shared; a composition total is a derived aggregate of its individual Worlds. Capture retention and release follow the [profiling implementation](../../crates/ipp-core/src/profiling/mod.rs), including its evaluation-thread ownership and quiescent readback boundaries.

Counter sums cannot reconstruct a chronological trace. Timeline captures need recorded spans, measured clock alignment and explicit dropped-event counts. GPU-process CPU activity is not GPU duration. A GPU query belongs to its original frame and context even when read later; unavailable, pending, disjoint, lost and dropped results are not zero-duration samples. Optional external profilers stay outside runtime dependencies and their observation cost belongs in the capture record.

Use `python tools/ipp.py trace browser --scene gallery-gui --output target/performance/gallery-trace --max-events 100000 --max-artifact-bytes 134217728 --software` for an instrumented gallery timeline, or select `trace native` and the device's `--egl-dir` for the native Host. The [gallery trace guide](../../tests/profiling/gallery-gui-trace.md) owns the workload and export details. These are diagnostic captures, separate from ordinary benchmark timing. Retain the raw capture and clock-correlation evidence beside the Chrome Trace/Perfetto export. Event retention and serialized artifact capacity are separate bounds; neither limits World work. A full event buffer reports dropped spans, while insufficient artifact capacity fails the export explicitly. Missing browser tracks and uncalibrated GPU durations remain explicit.

Physical GL-call counts describe the device context's actual observation window after retained-state suppression. Profiler query calls are reported separately. Context loss can end that window before the CPU capture stops; retain its original context, timestamps and stop reason instead of treating the partial counts as a full CPU-window total.

Keep memory providers separate. IPP-accounted bytes, allocator calls/requested bytes, WASM linear-memory capacity, Chrome backing allocations and DRM-client memory have different scopes and overlap. Core allocation captures cover the owning evaluation thread, excluding native background transport allocations; they are not process-wide totals. Do not sum nested browser allocator categories or subtract unrelated providers to declare a leak. The [GUI report](../../tests/performance/gui-stress.md#report) owns exact provider fields and raw evidence. Preserve missing-provider reasons and identify sequential observations instead of claiming an atomic process snapshot.

The [size reporting commands](building.md#release-and-size-experiments) measure raw and compressed distributions, code/data, JavaScript and embedded shaders without counting the same bytes twice. Stripped or optimized functions can remain unattributed; retain that remainder rather than guessing crate ownership. Runtime resource isolation budgets for individual Worlds are separate policy, not artifact-size thresholds.

## Preserve and validate evidence

Use maintained pipeline commands and reusable fixtures. Keep report JSON, trace/profile files, completed-frame images, raw provider observations, logs, source/build manifests and the pipeline summary together. The scenario event log is bounded: do not put PNG data URLs, full inspections or raw dump arrays in it. Write those as separate artifacts, and inspect truncation and dropped-record indicators before treating a capture as complete. Diagnostic failure must not erase earlier evidence.

Own every process and capture session you start. Retain exact process handles/PIDs, stop only those participants, and clean up on timeout, cancellation and failure as well as success. A broad process-name match can terminate unrelated work or the invoking shell. An already active browser trace belongs to its existing owner.

Correctness uses the maintained generated-client/native/worker/rendering scenarios and meaningful image/state assertions under the [integration policy](integration-testing.md). Benchmarks stay outside regression; do not add hardware timing thresholds to ordinary correctness runs. Report the selected checks and any omitted environment honestly. The [physical-device procedures](../../tests/performance/retained-gui.md#iphone-14-pro-procedure) separate Safari/mobile setup from actual device evidence. Record the resulting handoff and next action in Beads, with no second performance task queue in documentation.
