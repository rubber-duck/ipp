# Generalized 2D Plot rendering on Apple M4

Measured on 2026-10-31 with ordinary release-small worker/WASM builds, Chromium 155.0.8059.84, ANGLE Metal on Apple M4, a 1600×1000 viewport and DPR 1. Each case warmed for two seconds, then recorded two two-second windows with presentation uncapped. These are completed-frame throughput observations, including Host/browser scheduling, not isolated GPU timings or phone/VR measurements. The [maintained workload](plot-coverage.md) owns the reproduction commands and interpretation.

| Existing feature  | Samples | Before FPS | After FPS |  Ratio |
| ----------------- | ------: | ---------: | --------: | -----: |
| Smooth line       |     600 |      699.0 |     693.5 |  0.99× |
| Smooth line       |   2,400 |      594.9 |     590.0 |  0.99× |
| Four smooth lines |     600 |      264.2 |     263.5 |  1.00× |
| Four smooth lines |   2,400 |      157.9 |     157.9 |  1.00× |
| Line with markers |     600 |      517.5 |     701.4 |  1.36× |
| Line with markers |   2,400 |       97.8 |     487.4 |  4.98× |
| Bars              |     600 |     1029.3 |    1037.5 |  1.01× |
| Bars              |   2,400 |      681.6 |     682.4 |  1.00× |
| Pie sectors       |     600 |       48.0 |     690.1 | 14.39× |
| Pie sectors       |   2,400 |       12.1 |     402.3 | 33.12× |

Areas and bands are new authoring capabilities, so there is no prior implementation for a before comparison. All filled cases retain their upper line; the four-band workload draws four fully overlapping transparent series.

| New fill combination             | 600 samples, FPS | 2,400 samples, FPS |
| -------------------------------- | ---------------: | -----------------: |
| Area to baseline                 |            375.2 |              231.7 |
| Band between columns             |            282.4 |              166.4 |
| Area with markers                |            366.3 |              222.7 |
| Four overlapping bands           |             75.2 |               42.3 |
| Band with authored contour paint |            150.0 |               70.0 |

The initial 2,400-sample four-band case was below 60 FPS; the dense-fill follow-up below improves it. Its four layers cover the same large region; this implementation preserves independent transparency and does not reduce samples. Efficient coverage does not make additional visible geometry, overlap or authored paint free. Mobile and headset hardware remain unmeasured.

Every final case reported zero unchanged-frame GPU upload bytes, GUI rebuilds and retained GUI allocations. Those counters are not a global allocator measurement: the separate Host allocation test observed zero allocations and zero bytes across 30 unchanged frames with smooth filled multiline plots and markers. Data edits and loading may allocate.

The renderer retains triangle unions and a conservative spatial index, removes shared interior edges, blends overlapping intervals once per paint and keeps distinct series in painter order. Direct local leaf lists avoid repeated tree traversal; wide queries retain spatial pruning. Circles and sectors use shared instanced shapes, and narrow planar sectors use tighter coverage rectangles. Generic Canvas paths remain available for arbitrary outlines.

Validation includes native GLES and worker/WebGL frames, baseline crossings, reversed/repeated coordinates, clipping, subpixel integrated opacity, custom-paint outer edges and overlapping interiors, curved/rear Surface views, mirrored pie picking, live data edits and context recovery. Existing dense lines, glass charts, percentage interpolation, 3D callers, target-correct contracts and strict workspace Clippy also passed. Regression was scoped to affected checks, not the full matrix.

The before product was preserved at task start. It records revision `5b121ffbd7515fc42c379476c1b7d953ef7daa56` with source hash `7233cea91a1d605182fb6b21344f87beff457fb2dd3abd9d2b640071c60bf0bf`; it is an artifact comparison, not a claim that a clean checkout of that commit produced it. The after product records base `be2fb0f2b012f5ffaad4dcb035291a09051de3b9` with the uncommitted implementation source hash `6c06ebabedd9c0ecd0215ee44b488187b084d87fa482926c061b860424fe4776`. Both use the same fixture hash `0c73f5b98735965ce7aa930ac8ae8f5f288b8d25c2395a6eaec14a2a4a22b9c7` and each product’s own generated contract.

| Identity | Before | After |
| --- | --- | --- |
| Runtime SHA-256 | `5928e771e6ef79fb166b00fc47c9d648a46f2f86b959a5f6b0f805e8ff66af39` | `f67f888ea2621fb39d5470095f2416ca924e5c90daca56784880ea3dd1aba010` |
| Contract SHA-256 | `8131be5abde4c165983c7f4a025e1840bd293c10e81375cab395fb960682d9c6` | `b0e6e781f0676a7a6714b278df036c04e371a8104d10eb0ca1d9cc23826461fc` |

Raw reports, captures and source snapshots are under `target/performance/plot-generalization-20261031/{before-final-full,after-final-full}/`. Timing pipeline reports are `run-zh6n5dza` and `run-_c9p_pen`. Correctness evidence spans `run-bfm4yy8u` and its passing retry `run-5bt9o9cq`; renderer packing and unchanged-frame allocation evidence is in `run-0c2i5lqu` (its unrelated lint/catalog failures were corrected in the later runs). Earlier developmental timing directories are retained but are not the final results above.

## Dense fill follow-up, 2026-11-01

The same M4, Chromium/ANGLE Metal, ordinary release-small runtime, 1600×1000 DPR-1 viewport and uncapped workload were compared again after optimizing filled-pixel coverage. Each case warmed for two seconds and measured three two-second windows. The desktop Host and its CEF processes were paused for timing; builds and correctness runs were separate. These remain completed-frame throughput observations, not isolated GPU durations.

| Case         | Samples | Before FPS | After FPS | Ratio |
| ------------ | ------: | ---------: | --------: | ----: |
| area         |     600 |      378.1 |     455.2 | 1.20× |
| band         |     600 |      281.8 |     349.6 | 1.24× |
| multiband    |     600 |       75.0 |      98.2 | 1.31× |
| painted-band |     600 |      150.0 |     167.9 | 1.12× |
| area         |   2,400 |      230.9 |     312.4 | 1.35× |
| band         |   2,400 |      166.1 |     239.3 | 1.44× |
| multiband    |   2,400 |       42.9 |      64.3 | 1.50× |
| painted-band |   2,400 |       70.3 |      80.1 | 1.14× |

At 2,400 samples the plot has about 1,508 horizontal pixels. Every source sample is still retained. The main improvement merges a pixel's triangle intervals during one traversal instead of repeatedly fetching them to discover successive interval ends. Eight local interval slots cover ordinary queries; overflow restarts the complete exact sweep without dropping coverage. A retained conservative grid also certifies wholly filled or empty pixel footprints. It only skips work away from possible boundaries, and wider/minified footprints use the detailed query. The grid uses at most 8 KiB within the existing index budget and is rebuilt only with changed geometry; its preparation cost is outside unchanged-frame timing.

The four-band case improves from roughly 23.3 to 15.5 milliseconds per completed frame. This clears 60 FPS for this M4 workload, with little headroom; it does not establish phone or stereo VR performance. The grid alone barely helped the dense bands: their many steep, narrow crossings kept pixels near boundaries. Removing repeated interval searches produced the main gain.

Six of eight comparison captures match byte-for-byte after decoding. Each of the other two differs by one color level in a single pixel, with no differences above one level. The independent integration oracle also squeezes 32 separated filled intervals into one pixel, in both horizontal directions, and checks their integrated linear-light opacity. Native GLES, worker/WebGL and actual context recovery pass, including existing paints, gaps, overlap, curved/rear views and picking. A pie-fixture readiness race discovered during validation was fixed by waiting for its expected projected values before capture.

Unchanged-frame GPU upload bytes, GUI rebuilds and retained GUI allocations remain zero in every case. The Host allocation suite still observes zero allocations for unchanged smooth filled multiline plots with markers. Focused renderer tests cover whole certified footprints, holes, overlap, narrow spikes, coordinate scale/translation, degenerate bounds and storage limits. Strict workspace Clippy and TypeScript checks pass. This is affected coverage, not the full regression matrix.

The preserved before runtime SHA-256 is `f67f888ea2621fb39d5470095f2416ca924e5c90daca56784880ea3dd1aba010`; the final after runtime is `ba938bfe9ce6e8a21d14e04cd7caf720259175cb3ee5b21b2ebd1bcf1b85e42b`. Both use fixture `0c73f5b98735965ce7aa930ac8ae8f5f288b8d25c2395a6eaec14a2a4a22b9c7` and contract `b0e6e781f0676a7a6714b278df036c04e371a8104d10eb0ca1d9cc23826461fc`. The after source identity is `0af5fcb8de47c53608f650ff1481fb0e8b38a9f97d2ac7f4b0c163718084ea81` on base `be2fb0f2b012f5ffaad4dcb035291a09051de3b9`, including uncommitted implementation. These are artifact comparisons, not clean-commit builds.

Raw reports, captures and source snapshots are in `target/performance/plot-fill-grid-20261101/{before-final,after-final}/`; decoded-image differences are in `final-image-comparison.json` beside them. Final timing runs are `run-l8znlaxa` and `run-jg8bq37f`. Correctness evidence is `run-cc361vxe` and its passing retry `run-h1iqudpz`; the first run's allocation and Clippy results remain applicable. Earlier `after-run` and `interval-run` timings are developmental evidence, not the final numbers above.
