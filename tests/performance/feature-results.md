# Expanded scene baseline — 15 September 2026

The [version 2 stress scene](stress.md) was generated from Blender 5.2.1 and imported through the native WebSocket Host with its matching generated client. The full saved World contains 10,138 entities, 159 controllers, 40,151 animation drivers and 10,063 assets. All 22 compiled production scene component types are represented, including 44 mesh-pose components across the deforming panels, material splits and four skinned humans.

## Native hardware replay

Ordinary release, Rust 1.98.1, AMD Radeon RX 9070 XT, Mesa 26.1.8 GLES 3.2, 800×600, overview camera, 60 frames per window:

| Measurement                                      |    Median |       p95 |
| ------------------------------------------------ | --------: | --------: |
| Moving complete frame                            | 12.878 ms | 19.827 ms |
| Update within that rendered window               |  5.866 ms |         — |
| Render preparation/submission within that window |  6.535 ms |         — |
| Isolated moving nonrender update                 |  4.488 ms |  9.404 ms |
| Later held pose-animation update window          |  3.075 ms |  3.265 ms |

The first moving frame submitted 8,470 surface draws and 35 shadow draws. Stage medians need not sum to the median complete frame. These are observations from one shared-host run, not an improvement comparison or a frame-budget guarantee; earlier fixtures have different workloads.

A separate instrumented run retained every driver and reported **zero Rust allocation/reallocation calls and zero requested bytes** in all six warmed measurement windows, five counted frames per window. This excludes loading, initial growth and graphics-driver allocations. The standalone unit-weight curve comparison is inapplicable to this fixture's composed/dynamic drivers and is explicitly omitted.

Both native runs passed twelve independent Blender position/rotation probes, 82 independently sampled deformed-vertex enclosure checks, automatic/authored bounds capture equivalence, completed GLES frames and World teardown. The import also checked repeat, weighted/additive/Bézier/step values, dynamic material parameters, animated projection, LookAt and all four picking shape kinds.

## Browser and exporter evidence

The small fixture passed through the real worker/WASM/WebGL Host. It verified the same 22 component types and expected values at 0.5, 1.5 and 2.5 seconds, including the short-driver repeat boundary. Holding every other input fixed, vertex-pose changes affected 53,002 panel pixels and 37,356 skinned-human pixels. Its warmed moving Rust allocation sample was also zero. Browser timing is not used as a hardware performance claim.

The extension exposed an exporter omission: active shape-key actions were absent from the reusable clip catalog, leaving clips-only imports static. The exporter now publishes those actions in both active playback and the catalog. Focused numerical exporter checks cover rigid and skinned cases; the benchmark verifies the imported weights and visible deformation.

Full export still resamples shared shape-key actions per instance. This repeats full-scene timeline evaluation during authoring; it is outside runtime frame timings. Reusing the prepared scene avoids that cost on subsequent measurements.

Local evidence from this run:

- Generated scene: `target/stress-benchmark/features-full/`.
- Native release, captures, independent probes, identities and coverage: `target/stress-benchmark/features-native-full/` (pipeline run `run-7b2x90oc`).
- Native allocation/stage evidence: `target/stress-benchmark/features-native-allocations/` (run `run-nn6ca2de`).
- Browser completed frames and `profile.json`: `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-b9gqkP/` (run `run-6em1riqb`).

Focused exporter/pipeline checks, TypeScript checking, native example Clippy, pinned formatting and repository checks accompany the runtime evidence. The stress benchmark remains opt-in; no full regression or full-size browser run was performed for this extension.
