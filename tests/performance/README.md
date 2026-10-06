# Performance benchmarks

Opt-in benchmarks exercise real IPP update, protocol, Blender import and rendering paths. Their timed runs (`python tools/ipp.py benchmark`) are excluded from every regression profile; normal correctness tests still cover the optimized runtime. The [GUI stress workload](gui-stress.md) is also a correctness suite: the `gui` regression group runs it untimed as `test:gui-stress:browser` and `test:gui-stress:native`. Profiling captures live in [`tests/profiling/`](../profiling/).

Use the [stress scene guide](stress.md) for maintained native/GLES and browser commands, scene contents, captures, allocation counters and profiling. The generator creates both the original physics scene and its baked replay, so large binary assets need not be checked in.

The `instrumentation` feature adds stage timing and allocation counters without changing runtime behavior; it is absent from normal builds, which timing runs measure. Timing windows disable counters, and allocation windows run separately. Core measurements identify the Host, World incarnation, composition, System and phase; equally composed Worlds remain distinct, and shared work is explicitly unattributed to a World. Captured metadata survives World retirement until capture release, while live prepared indices remain stable. [Profiling source](../../crates/ipp-core/src/profiling/mod.rs) owns counter semantics, storage retention and the single-evaluation-thread capture contract; consumers must read exported metadata rather than infer identity from offsets. Native hardware GLES timing is the primary rendering baseline; software browser timing does not predict hardware performance.

## Measurements

- [Expanded feature coverage baseline](feature-results.md)
- [Chart and data measurements](chart-data-results.md)
- [Native results and camera culling](native-results.md)
- [Initial stress results](stress-results.md)
- [Joint, material and response reuse](reuse.md)
- [Per-frame allocation sweep](allocations.md)
- [Original pointer experiment](pointer-results.md)

These reports preserve the original machine, revision and methodology. Historical commands refer to the experiment's former tooling; use the current guide for new runs. Their numbered modes and toggles were in-process comparison switches that have since been removed; the runtime keeps only the selected implementation. Retained output records source/build/fixture identity so new measurements can be compared without confusing instrumented and ordinary builds. The September 2026 performance pass also used ephemeral scratch scripts and raw captures that are no longer available. Its artifact inventory and dispositions are recorded in Beads task `ipp-2pv8.2`; maintained workloads below produce new evidence with current runtime behavior, not reproductions of the missing scripts or exact historical numbers. Patched generated WebGL bridges and CPU-side GPU waits from those experiments are not maintained device diagnostics or GPU timer measurements.

The [retained Surface rendering guide](retained-gui.md) compares analytic and retained browser text presentation and defines the pending iPhone 14 Pro GUI procedure.

The [React GUI stress benchmark](gui-stress.md) runs one GUI-enabled build per revision through a fixed logical workload on real browser/WebGL or native/GLES paths, with separate timing, capture, work and allocation windows.

The [chart and data stress benchmark](chart-data.md) measures bounded source/binding scales and dense Plot frames through a normal release native Host, with separate memory observations and paired local allocation diagnostics.

The [gallery CPU trace](../profiling/gallery-gui-trace.md) captures bounded World, System and fixed CPU spans through the real worker or native Host. Its maintained command exercises a physical browser slider drag or a native public gallery option update, retains completed images, and exports Chrome Trace/Perfetto JSON with exact identities, dropped-span counts and clock-correlation bounds. Optional worker CPU profiles remain separate until their clock relation is measured. Record the actual renderer and compositor arrangement; hardware WebGL does not establish hardware page composition.

The [Blender streaming harness](blender-stream.md) compares full and indexed imports, checks command page bounds and pending source delivery, and captures matching completed frames. [Stress profiling](stress.md) supports worker CPU/allocation sampling and save/load stage measurements on verified hardware WebGL.
