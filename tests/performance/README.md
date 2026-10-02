# Performance benchmarks

Opt-in benchmarks exercise real IPP update, protocol, Blender import and rendering paths. They are deliberately excluded from every regression profile; normal correctness tests still cover the optimized runtime.

Use the [stress scene guide](stress.md) for maintained native/GLES and browser commands, scene contents, captures, allocation counters and profiling. The generator creates both the original physics scene and its baked replay, so large binary assets need not be checked in.

The `instrumentation` feature adds stage timing and allocation counters without changing runtime behavior; it is absent from normal builds, which timing runs measure. Timing windows disable counters, and allocation windows run separately. Stage counters use separate ranges: the first 30 scheduled Systems time each frame phase in their own slots, and fixed commit and animation timers follow in a range of their own, so no System position aliases a fixed timer; [profiling.rs](../../crates/ipp-core/src/profiling.rs) owns the slot layout and names. Native hardware GLES timing is the primary rendering baseline; software browser timing does not predict hardware performance.

## Measurements

- [Expanded feature coverage baseline](feature-results.md)
- [Native results and camera culling](native-results.md)
- [Initial stress results](stress-results.md)
- [Joint, material and response reuse](reuse.md)
- [Per-frame allocation sweep](allocations.md)
- [Original pointer experiment](pointer-results.md)

These reports preserve the original machine, revision and methodology. Historical commands refer to the experiment's former tooling; use the current guide for new runs. Their numbered modes and toggles were in-process comparison switches that have since been removed; the runtime keeps only the selected implementation. Retained output records source/build/fixture identity so new measurements can be compared without confusing instrumented and ordinary builds.

The [retained Surface rendering guide](retained-gui.md) compares analytic and retained browser text presentation and defines the pending iPhone 14 Pro GUI procedure.

The [React GUI stress benchmark](gui-stress.md) runs one GUI-enabled build per revision through a fixed logical workload on real browser/WebGL or native/GLES paths, with separate timing, capture, work and allocation windows.

The [gallery GUI trace](gallery-gui-trace.ts) records counters, a Chrome trace of every thread and worker and main-thread CPU profiles for the GUI demo at idle, under hover, slider drag, exploded layers and camera orbit. Headless Chromium on Vulkan composites the page in software, reading the WebGL canvas back every frame and pacing frames near 36 per second whatever the content; `IPP_BROWSER_GPU_COMPOSITING=1` composites on the GPU as a desktop browser does.

The [Blender streaming harness](blender-stream.md) compares full and indexed imports, checks command page bounds and pending source delivery, and captures matching completed frames. [Stress profiling](stress.md) supports worker CPU/allocation sampling and save/load stage measurements on verified hardware WebGL.
