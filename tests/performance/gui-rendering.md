# Native GUI rendering benchmark

The board exercises the real GUI kit and Data Service plots on native Metal, hardware GLES and WebGPU. It keeps every specimen at the same coordinates and size across individual, mixed and removal measurements. A Host-clock animation moves an eight-pixel mark along the lower edge to invalidate the Canvas while the controls and source data stay fixed. Each case starts a fresh Host on each backend. Cartoon notifications use the same authored tilt across mounts. Steady redraw windows hold component and dataset values fixed. The live-chart cases also measure dataset edits through the next Host frame. Use the [React GUI stress benchmark](gui-stress.md) for interaction, layout churn and action latency.

## Run a comparison

Run these on a Mac with a hardware Metal device. Runs that include GLES also require the ANGLE libraries. The full board runs with every gallery theme (Glass, Cybertron and Cartoon) on a direct Canvas and on flat, cylindrical and spherical Surfaces. Each Surface run uses one orthographic camera and one attached board. It has no other scene content. The flat Surface controls for camera and attachment costs. The board is four metres wide with coincident layers. Both curved geometries use a curvature of 0.45 per metre. The camera keeps the pixel scale at the centre constant, so the projected screen area decreases toward the edges.

```sh
export IPP_EGL_LIBRARY_DIR=/path/to/angle/lib
python tools/ipp.py benchmark native --scene gui-rendering --cases all --repetitions 3
python tools/ipp.py benchmark native --scene gui-rendering --themes glass --projections cylinder,sphere --cases blank,all,charts,without-charts,band,without-band --repetitions 3 --output target/performance/gui-chart-attribution
python tools/ipp.py benchmark native --scene gui-rendering --cases all --instrumented --frames 80 --repetitions 1 --output target/performance/gui-rendering-gpu
```

Use `--renderer all` to rotate all three backends, or select one with `--renderer metal`, `--renderer gles` or `--renderer webgpu`. Match the fixtures, capture dimensions and reference executable identity across runs.

With no filters, the default sweep measures the full theme and projection matrix, then includes an empty board, each family and each open overlay on the direct Glass Canvas. Pass comma-separated `--themes`, `--projections` and `--cases` to select a cross product. The projection keys are `canvas`, `flat`, `cylinder` and `sphere`. Each selected specimen keeps its original position, size and caption. A `without-<id>` case removes the specimen and its caption. Include `all` and its `without-` case in the same run to measure the difference.

## Workloads

| Family | Specimens |
| --- | --- |
| actions | Buttons, secondary buttons, checkbox, switch, toggle, toggle group, radio group, segmented control, toolbar icons and check mark |
| editing | Text field, textarea, password, OTP, token input, file input, form, section, field and label |
| values | Labelled slider with scale, range slider, knob, numeric stepper, colour picker and rating |
| selection | Dropdown, searchable dropdown, multi-select, autocomplete, combobox, listbox, tree and pagination |
| navigation | Tabs, stepper, carousel, accordion, collapsible, expander, menubar and navigation menu |
| containers | Panel with header, body, footer, separators and window controls, scroll area, 256-row virtual data grid, resizable panes and empty state |
| feedback | Progress bar, progress ring, spinner at rest, meter, status badge, alert, toast, timeline and waterfall |
| dates-media | Calendar, date and time fields, date picker, bitmap avatar and image cropper |
| overlays | Menu, context menu, popover, tooltip, hover card and dialog triggers |
| charts | Straight and smooth lines, markers, area, lower-bound band, bars and pie. Each line uses 128 deterministic rows, bars use 24 and pie uses nine. |
| paint | Vector waveform drawing and a large skinned panel |

Window-wide overlays have separate cases so they can be measured open without obscuring the mixed board. They include `command-open`, `confirmation-open`, `dialog-open`, `drawer-open`, `popover-open`, `context-menu-open`, `tooltip-open`, `hover-card-open`, `trigger-menu-open` and `toast-stack`. Hover, secondary-click and caret-triggered cases use native input before timing and verify the overlay became visible.

Optional spatial chart cases select one chart-only camera at its native 960 × 760 resolution per theme. They run once per selected theme independently of the board projection. Select `3d-grid-bars`, `3d-single-row`, `3d-height-surface`, `3d-point-plot` and `3d-variable-pie` explicitly. The GUI score includes only 2D plots.

The optional `data-grid-scroll` case also measures native wheel input after the steady-redraw window. It alternates 16 down/up wheel inputs of 144 logical units over the virtual grid, verifies that new rows appear and the initial range returns, and waits for a completed frame after each change. The per-case JSON reports individual latencies, p50, p95 and the furthest first realized row. These end-to-end measurements include client observation of the realized rows.

Append `-live` to `area`, `band`, `all` or a spatial chart name to measure changing source data. Each update edits every row, preserves its identity, waits for the new evaluated value in every participating binding, and observes a later completed frame. The full board edits all seven chart sources. The harness records 16 consecutive updates after two warm-ups, with separate commit and update-to-frame latencies. The timed endpoint is the offscreen Host acknowledging a newer presentation that includes the evaluated output. The Metal queue can still have submitted GPU work in flight. Source row generation and pixel readback fall outside these timings. GPU-synchronized captures before and after the edit series verify visible changes inside every chart. Instrumented runs also retain a CPU trace and GPU frame timestamps during the updates.

The larger `area-1k-live`, `band-1k-live`, `3d-point-plot-1k-live` and `3d-height-surface-1k-live` cases change 1,024 rows per update. The height surface uses a 32 × 32 grid with wireframe and a missing height at its center. Isolated spatial charts use their native camera for each skin.

## Read the results

`comparison.csv` gives the median of the per-repetition mean submission times in milliseconds on each backend. For removal cases the saved columns give the median paired difference against the full board at the same theme and projection. Positive values mean the removal saved time. Batching and composition mean isolated case costs need not add up to the full frame.

Each ordinary timing sample counts Host presentation sequence changes over a wall-clock window. The Host runs uncapped. Each release case samples for at least three seconds. The report includes FPS, elapsed time, draws and triangles, and the p50/p95 of window-average submission times. A separate instrumented run collects GPU durations for at least two seconds so asynchronous timer results can arrive. Its CSV reports the median of each repetition's GPU-frame p50 and the number of available GPU samples and retained frame records per backend. The profile artifacts retain per-record status and dropped-record counts. Use the release comparison to assess throughput when few queries resolved or a timer median exceeds the observed frame interval by a large margin. ANGLE reported this discrepancy on the spherical board even with a full set of retained samples.

`report.json` records hardware, binaries, generated contracts, asset identities, repetition order and failures. Each backend directory contains a PNG and raw samples per case. Its `catalog.json` gives every selectable widget and its content rectangle. Captures check visible content below each selected caption and four registration marks against the expected projection. The chart cases wait for the actual source bindings to become ready. Inspect the PNG when evaluating a renderer change.

## Apple M4 release baseline

The full board ran at 2016 × 1152 on 25 November 2026 using native Metal and GLES through ANGLE 2.1.28252. Each cell gives Metal / GLES milliseconds per completed presentation, using the median of two release sweeps in opposite order.

| Theme     | Direct Canvas | Flat Surface |    Cylinder |      Sphere |
| --------- | ------------: | -----------: | ----------: | ----------: |
| Glass     |   2.31 / 2.85 |  2.31 / 2.86 | 4.56 / 4.78 | 6.05 / 5.35 |
| Cybertron |   1.96 / 2.69 |  1.95 / 2.71 | 4.39 / 4.44 | 4.42 / 4.50 |
| Cartoon   |   2.21 / 2.81 |  2.23 / 2.83 | 4.47 / 4.69 | 4.64 / 4.87 |

The Glass sphere took 0.70 ms longer on Metal. Metal reported 870 draws and 9,748 triangles on both the Glass cylinder and sphere, yet the sphere took 1.49 ms longer than the cylinder.

In two additional release sweeps on the Glass sphere, removing the seven 2D charts saved 1.99 ms on Metal and 1.25 ms on GLES. The remaining controls took 4.06 / 4.13 ms. Removing just the date and media controls saved 0.37 / 0.89 ms. Removing the waveform drawing and large panel saved 0.04 ms on each backend. The curved Glass chart workload accounts for the measured Metal disadvantage in this fixture.

## Curved board after Metal optimization

The same Apple M4 reran the release comparison after Metal began bounding each spherical paint primitive by its own chart coordinates and retaining separate draw batches for the two ray intersections. Each cell gives Metal / GLES milliseconds per completed presentation over two sweeps.

| Theme     | Direct Canvas | Flat Surface |    Cylinder |      Sphere |
| --------- | ------------: | -----------: | ----------: | ----------: |
| Glass     |   2.32 / 2.85 |  2.32 / 2.85 | 3.82 / 4.79 | 3.42 / 5.37 |
| Cybertron |   1.98 / 2.69 |  1.96 / 2.69 | 2.42 / 4.45 | 2.55 / 4.51 |
| Cartoon   |   2.24 / 2.81 |  2.23 / 2.82 | 3.25 / 4.71 | 2.71 / 4.88 |

The previous Glass sphere took 6.05 ms on Metal. Tighter scheduling bounds alone reduced it to 4.70 ms. With the two intersections retaining their own batches it took 3.42 ms. All seven isolated Glass sphere charts were faster on Metal in a separate paired sweep. The filled area chart had the smallest margin, at 1.46 ms on Metal versus 1.53 ms on GLES. The comparison exercises steady redraw with fixed chart data and camera placement.

## Independent coverage trials on Apple M4

Three standalone builds used the same starting Metal executable, chart data and assets. The 74-specimen board ran twice per build in opposing orders across Glass, Cybertron and Cartoon on direct Canvas, flat, cylinder and sphere. Isolated cases ran once per configuration. Each release window observed at least 96 completed frames and eight intervals across 1.2 seconds after warm-up. Percentages below give reductions in time using the geometric mean of within-configuration ratios. GLES ran through ANGLE on the same Apple M4.

| Change | Isolated release time vs original Metal | Full board vs original Metal | Full board vs GLES |
| --- | --- | --- | --- |
| Conservative local grid for filled triangles | Area 49.55% lower, band 27.14% lower | 5.05% lower | 32.2% lower |
| Straight glyph spans and implicit quadratic boundaries | Stationary data grid 0.24% higher | 0.03% lower | 28.6% lower |
| Precomputed stroke axes and gradient projection | Value controls 0.26% higher | 0.06% higher | 28.6% lower |

The chart build beat GLES in all 24 isolated area/band cases and all twelve full-board configurations. The original Metal build beat GLES in 13 of the 24 isolated cases. Glass cylinder benefited most on the full board at 3.85 to 2.81 ms. The chart build was slower than original Metal in seven full-board configurations, by up to 1.8%. A verification build that skips redundant band construction and bounds temporary grid storage took 2.78 ms on Glass cylinder and 3.01 ms on Glass sphere. That single verification sweep beat GLES in all twelve configurations.

All 24 isolated area and band captures were byte-identical to original Metal outside the moving redraw mark. Full boards also matched on Glass and Cybertron. Cartoon controls varied between fresh mounts, including between two unchanged reference runs. The largest chart-build difference outside the marker was 1,994 full-board pixels with a channel difference above eight. At the same threshold, the unchanged Cartoon Canvas reference differed at 2,676 pixels between mounts.

For a 128-point area, preparing one new 254-triangle path on a warmed Metal device took 67.00 µs with the retained grid versus 14.54 µs with the original bands. Occupied path records fell from 31,536 to 16,640 bytes. Preparing 94 font glyphs took 89.29 µs with the glyph prototype versus 79.13 µs originally. Packing a two-segment gradient stroke took 33.38 ns versus 16.74 ns. The retained renderer uses the filled-chart grid.

## Live chart preparation on Apple M4

Each live edit replaces values in every retained source row and keeps the row IDs. The full board edits 673 rows across seven independent producers. For each source, the harness checks the committed delta count and waits for the expected f32 value in the evaluated binding. It then requests a newer frame fenced on that output. The before/after GPU readbacks must differ inside every chart's projected body. The 3D cases use each theme's gallery camera and chart material.

On the Glass sphere with 1,024 edited rows, area edit-to-frame p50 went from 113.22 ms on original Metal to 21.20 ms with the filled-chart grid. GLES took 48.02 ms. Band went from 48.34 to 20.82 ms, with GLES at 48.72 ms. In separate instrumented edit windows, area GPU-frame p50 fell from 7.12 to 1.25 ms and band from 2.88 to 1.23 ms. These GPU windows include frames between source edits.

Metal now packs the selected 3D chart triangles into 24-byte unlit vertices or 44-byte material vertices. The previous path supplied 96-byte skinnable vertices and indices for each mesh variant. Over eighteen Glass terrain edits, the 1,024-row fixture incurred 54 real mesh uploads. Total traced upload time per edit fell from 1.89 to 0.60 ms. For the 1,024-row point fixture, all eighteen edits uploaded geometry and mean upload time fell from 0.54 to 0.32 ms. A separate shared Plot change computes each face normal once and sorts and deduplicates wireframe edges in contiguous storage. On a fixed 32 × 32 wireframe grid, its isolated preparation median fell from 1.21–1.22 to 1.08 ms.

Four 1,024-row Glass charts also replayed dataset states A, B, A, B. The area and band used the sphere. Terrain and points used the native camera. Each transition from A to B changed at least 3,933 pixels by more than eight channel levels. Every return to a previous state reproduced its pixels exactly on optimized Metal, and all sixteen optimized captures matched the corresponding original Metal capture outside the animated board mark. Spatial comparisons include the entire framebuffer.

A separate Glass experiment selected a dedicated affine path shader for depth-tested labels. Splitting paths from neighboring primitives added four draws in all seven spatial workloads. In two opposite-order pairs, redraw took 4.57% longer and edit-to-frame p50 took 3.04% longer overall. All 28 paired screenshots matched. The retained renderer continues batching those labels with neighboring primitives.

### Live comparison across themes

Times are the geometric means of the per-configuration edit-to-frame p50 in milliseconds. Planar 128-row charts and the 673-row board include three themes and four projections. The 1,024-row planar cases also include all three themes and four projections. Each spatial case includes all three themes. For spatial cases the final Metal and GLES builds both include shared geometry preparation.

| Workload                    | Original Metal | Final Metal |   GLES |
| --------------------------- | -------------: | ----------: | -----: |
| 128-row area                |          18.26 |        9.82 |  18.65 |
| 128-row band                |          12.74 |        9.82 |  17.86 |
| Full board, seven sources   |         188.68 |      186.54 | 262.06 |
| 1,024-row area              |         129.09 |       14.84 |  29.98 |
| 1,024-row band              |          59.04 |       14.42 |  31.11 |
| Grid bars                   |           3.26 |        3.21 |   2.94 |
| Single row                  |           3.45 |        3.47 |   2.77 |
| Height surface              |           4.39 |        4.20 |   4.35 |
| Point plot                  |           3.59 |        3.59 |   2.78 |
| Variable pie                |           1.83 |        1.75 |   2.23 |
| 1,024-row wireframe terrain |           8.96 |        8.11 |   8.60 |
| 1,024-row point plot        |           8.44 |        7.97 |  11.79 |

Across all 42 spatial pre/post-edits captures, 42 matched original Metal exactly. The largest final Metal versus GLES spatial mean absolute RGB error was 1.135 on the 0–255 scale. GLES orders translucent chart triangles for the camera. Metal preserves their published order.

Both dense spatial workloads beat GLES in all three themes. Small point plots and single-row bars favored GLES in all three themes. The source protocol delivers independent producer updates in order. For the seven-source board, the reported latency includes every source commit, binding observation and the next Host frame.

The full 1,024-row planar comparison covered 72 release windows. Across all twelve theme and projection combinations, 48 of 48 optimized Metal before/after captures matched original Metal outside the redraw marker.

## Rust Metal bindings and GPUI batching on Apple M4

Three release Metal executables used the same Host, paint and coverage shaders, GUI fixtures and shared scene extraction from the working checkout at `6eaa3b0`. The baseline used IPP's Rust-to-Objective-C bridge. The second moved command encoding into Rust using `objc2-metal`, retaining the original six-vertex quads and upload allocation strategy. The third used four-vertex instanced strips, pooled transient uploads and compact immutable buffers for large retained batches. Splitting the experiment this way measures the Rust port separately from the GPU submission changes.

The 27 configurations included the complete board with unchanged chart data, the complete board with 673 changed rows per update, and a 1,024-row live 3D terrain chart. The boards ran under Glass, Cybertron and Cartoon on direct Canvas, flat, cylinder and sphere. The spatial chart used its own camera. Each configuration ran in a fresh offscreen Host for at least 96 frames and 1.5 seconds, followed by 16 acknowledged data edits on the live workloads. Each set rotated the old Metal, Rust port, optimized Metal and GLES executables. The second Metal round reversed configuration order. The fixture selected the same authored Cartoon tab, segment and notification tilt on every mount. The comparison uses geometric means of per-configuration measurements. Frame intervals measure steady redraw throughput. Edit-to-frame includes the client commit and Host publication.

A separate instrumented run measured the GPU command-buffer duration, CPU scene preparation and encoding before frame finalization, and new Metal upload-buffer allocations. It sampled the Glass board on direct Canvas, cylinder and sphere. Allocation counts cover Canvas batches, mesh uniforms and palettes, including new buffers for recordings. They exclude static assets, textures, readbacks and the cylinder intersection map.

| Workload and metric (ms) | Original Metal | Rust port | Rust port with pooled uploads and strips | GLES |
| --- | --: | --: | --: | --: |
| Full board, steady frame interval | 2.479 | 2.283 | 2.209 | 3.678 |
| Live board, steady frame interval between edits | 2.488 | 2.285 | 2.211 | 3.699 |
| Live board, edit-to-frame p50 | 191.10 | 171.45 | 162.16 | 265.96 |
| Live board, edit-to-frame p95 | 196.30 | 176.48 | 166.66 | 274.38 |
| Live 3D terrain, steady frame interval | 0.234 | 0.235 | 0.228 | 0.183 |
| Live 3D terrain, edit-to-frame p50 | 8.08 | 8.16 | 8.37 | 8.70 |
| Live 3D terrain, edit-to-frame p95 | 8.94 | 8.95 | 10.51 | 9.50 |

Both Metal changes reduced the full-board frame interval in all 12 theme/projection combinations. The Rust port reduced it by 7.9%. Pooled uploads and instanced strips reduced it by another 3.2% relative to the port. The improvement over the original renderer was 11.1% in the first round and 10.7% in the second. The live board spent 148.10 ms of its 162.16 ms median edit-to-frame interval waiting for the edit commit. Terrain edit latency increased most on Glass, from 9.99 ms to 11.71 ms at the median.

| Glass workload | Draws per frame | Median CPU preparation and encoding, old → optimized (ms) | Median GPU command buffer, old → optimized (ms) | Mean new upload buffers per frame, old → optimized |
| --- | --: | --: | --: | --: |
| Direct Canvas | 365 | 0.336 → 0.133 | 1.276 → 1.203 | 11.00 → 0.00 |
| Cylinder | 882 | 0.716 → 0.339 | 2.605 → 2.508 | 3.95 → 0.00 |
| Sphere | 880 | 0.722 → 0.349 | 3.019 → 2.901 | 4.56 → 0.23 |
| Live 3D terrain, steady redraw | 11 | 0.129 → 0.071 | 0.402 → 0.391 | 5.00 → 0.00 |

The old and optimized renderers submitted the same draw and triangle counts for these boards. CPU samples exclude the final batch flush, commit and GPU wait. GPU scopes cover the full command buffer and synchronize the diagnostic Host. Release timings above use the normal in-flight queue. All times refer to offscreen rendering on this M4.

Across the two Metal rounds, all 84 optimized captures matched the original renderer pixel for pixel outside the animated redraw marker. This includes every pre-edit and post-edit board. All 12 spatial pre/post captures matched over the full frame. For each of the 42 first-round images, the measured difference against GLES outside the marker was identical before and after the migration. The largest mean absolute RGB difference against GLES was 1.97 on the 0–255 scale on the Glass direct Canvas board.

A separate two-round terrain trial changed only the upload strategy for large mesh uniforms and joint palettes. Allocating a new Metal buffer for each upload gave an 8.47 ms median edit-to-frame time and a 9.98 ms p95, versus 8.52 ms and 9.57 ms with pooling. The original bridge measured 8.02 ms and 8.84 ms in the same trial. Pooling stayed enabled. The remaining Glass terrain edit regression is examined in the follow-up below. A final run of the shipping Host across all 12 full boards measured a 2.22 ms geometric mean and matched the original pixels outside the marker in all 12 captures.

## Profiling the live 3D terrain

On Apple M4, the 960 × 760 offscreen chart replaced 1,024 source rows per edit. Each theme ran three fresh release Hosts for each of the three builds in the table, with at least 1.5 seconds of steady measurement per run. The table gives the median of the three steady submission intervals and the pooled p50 and p95 of 48 acknowledged edits. Each edit waited for the changed binding and a newer Host frame. GPU times came from separate instrumented runs. Each skin used the chart camera. Board projections do not apply to this fixture.

| Theme | Previous Metal (redraw / edit p50 / edit p95, ms) | Updated Metal (ms) | Updated GLES (ms) | GPU p50, previous Metal / updated Metal / updated GLES (ms) |
| --- | --: | --: | --: | --: |
| Glass | 0.330 / 12.00 / 14.48 | 0.137 / 6.19 / 6.85 | 0.187 / 8.90 / 9.81 | 0.387 / 0.164 / 0.358 |
| Cybertron | 0.200 / 8.26 / 8.95 | 0.087 / 5.58 / 6.47 | 0.173 / 7.22 / 8.04 | 0.261 / 0.081 / 0.178 |
| Cartoon | 0.179 / 6.00 / 6.57 | 0.086 / 4.98 / 6.38 | 0.183 / 6.64 / 7.47 | 0.198 / 0.078 / 0.231 |

The costliest layer was the spatial Canvas geometry around the data. Omitting the chart planes in a profiling build reduced the previous Glass GPU median from 0.389 to 0.182 ms. Omitting just the mesh wireframe saved 0.064 ms, and replacing the optical fragment with flat color saved 0.068 ms. The chart planes include many thin grid and axis paths. Expanding each path by at least one local unit gave the four coverage taps dozens of pixels of empty space to test on either side. Metal now maps one framebuffer pixel back through the plane and local transform to bound that work for orthographic world planes. Perspective and curved paths keep their original envelopes.

Live wireframe edits also used to generate and upload 36 vertex records per edge. Plot now emits eight corners with the same 36 indices and triangle winding. Metal keeps the indices where they reduce upload size and uses a position-and-color pipeline for unlit marks. Glass keeps its full optical material. On Glass, the geometry change alone preserved both captured frames byte for byte. With indexed geometry and the unlit pipeline, GPU time was 0.339 ms. Tightening the chart-plane bounds reduced it to 0.164 ms with the same ten draws.

The release comparison uses GLES rebuilt with the same shared Plot change. Original GLES measured 0.190 / 10.98 / 11.79 ms on Glass in the reference run. The pre-port Objective-C bridge measured 0.338 / 9.88 / 10.53 ms across three Glass runs. In a separate GLES diagnostic, adding a flush after each offscreen draw changed the interval from 0.189 to 0.199 ms, while finishing the GPU after every frame raised it to 0.736 ms. The normal submission interval and the separately sampled GPU duration measure different boundaries.

A separate release trace timed dataset receipt, geometry preparation, mesh upload, and command submission on real edits. The native Host handles dataset requests, evaluates the World, presents, and then flushes the reply. This couples the client commit time to the cost of drawing the chart. Values below are medians in milliseconds over 32 edits per build. Stage medians can be taken from different edits.

| Glass edit stage | Original bridge | Previous Metal | Updated Metal |
| --- | --: | --: | --: |
| Dataset handling before World evaluation | 0.473 | 0.412 | 0.347 |
| Terrain geometry construction | 1.079 | 1.327 | 0.440 |
| All chart mesh uploads | 0.441 | 0.548 | 0.191 |
| Final encoder flush, queue retirement and commit | 0.005 | 0.004 | 0.003 |
| Host receives FINISH to reply flush | 2.339 | 2.605 | 1.186 |
| Client FINISH request to credit | 3.047 | 3.428 | 1.511 |

Seven additional Glass runs sampled the CPU executing the identical JavaScript dataset encoder. The table reports the range of run medians. Effective GHz divides process cycles by active process CPU time during encoding. CPU IDs sampled at both ends of the encode identify the core class on this M4.

| Glass client encoding | Release runs | Time (ms) | Million instructions | Million cycles | Effective GHz |
| --- | --: | --: | --: | --: | --: |
| Original bridge | 2 | 2.41 | 54.84–55.03 | 9.43–9.46 | 3.91–3.92 |
| Previous Metal | 3 | 3.33–3.58 | 54.93–55.06 | 9.38–9.58 | 2.61–2.86 |
| Updated Metal | 2 | 2.44–2.46 | 55.05–55.20 | 9.61–9.77 | 3.94 |

The client began 112/112 measured encodes and ended 112/112 on performance cores. During the slower previous-Metal runs, the same encoding work took longer at a lower CPU operating rate. The Host geometry interval also tracked active thread time. Together with the cost of reconstructing the mesh and waiting for replies between redraws, this explains why faster CPU-side draw encoding could coexist with slower end-to-end edits. The CPU operating-rate observation is specific to this machine and uncapped workload.

Across all six before/after Metal captures, 213 of 4,377,600 RGBA pixels differed from previous Metal. The largest frame difference was 54 pixels. The largest channel delta was 55/255. The shared Plot change altered 0 pixels across the six GLES captures. Updated Metal and GLES still differ in 12,973–32,933 pixels per frame.

On the full flat Glass board, the previous and updated Metal builds measured 2.109 and 2.109 ms per submission. All pixels outside the animated marker matched. A separate live 1,024-point Glass plot improved from 0.336 to 0.179 ms per redraw and from 11.49 to 6.78 ms per edit in one run.
