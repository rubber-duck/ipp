# Native WebGPU gallery and GUI measurements

## Cockpit and particles on Apple M4

On 27 November 2026, direct Metal led native WebGPU on the Metal adapter and GLES through ANGLE on both scenes. The release comparison used the real gallery at 1600 × 900 with the Glass skin. The table gives median uncapped offscreen Host submissions per second.

| Scene | Metal before | Metal after | WebGPU | GLES | Metal gain | Metal / WebGPU | Metal / GLES |
| --- | --: | --: | --: | --: | --: | --: | --: |
| Cockpit | 1,072 | 2,218 | 1,872 | 976 | 106.8% | 1.18× | 2.27× |
| Particles | 2,176 | 3,419 | 2,462 | 1,666 | 57.1% | 1.39× | 2.05× |

The three release backends ran serially with rotated backend order and alternating scene order, three repetitions per backend. Each window lasted at least three seconds and 96 completed-frame requests after warmup. The previous Metal executable from `742112b` ran three more times against the same asset hashes. Captures and profiling ran outside the release timing windows.

### Where the time went

**Cockpit.** WGPU reuses the paint of stable curved panels in screen pixels. Direct Metal used to walk and shade those Canvas primitives for each submission. It now retains the projected paint when the view, lighting, target size and Canvas content match, and tests the panel’s depth against current scene geometry each frame. The root Canvas can also reuse a stable overlay above a live Camera. With panel reuse disabled in a diagnostic release build, the new renderer reached 1,111 submissions/s. Enabling it reduced median submitted draws from the original 80 to 15.

**Particles.** A sampled CPU profile of the Metal Host showed per-sprite billboard math, matrix multiplication and transparent draw preparation in the Camera pass. WGPU groups ordered sprite instances separately from general meshes. Direct Metal now sorts compact draw references and uploads a 48-byte instance per sprite. The vertex shader expands four corners using one shared Camera basis. Depth ordering, texture changes and alpha/additive blend boundaries are preserved. The particle scene went from 11 to 4 median draws, including the gallery overlay.

| Scene | Metal GPU frame before | Metal GPU frame after | Available samples before / after |
| --- | --: | --: | --: |
| Cockpit | 990 µs | 457 µs | 123 / 123 |
| Particles | 178 µs | 79 µs | 124 / 124 |

The GPU numbers are median command-buffer durations from separate instrumented runs. Release throughput also includes World evaluation, CPU preparation and submission.

### Pixel checks

Each pair below compares three matched release captures. Values give the median and maximum percentage of pixels whose absolute difference exceeded 16/255 in any RGB channel. The gallery uses independent Host clocks, so particle positions and animated instruments vary between captures. The last column compares successive captures of the previous Metal binary to show that timing variation.

| Scene | Metal / WebGPU | Metal / GLES | WebGPU / GLES | Metal / previous Metal | Previous Metal repeat |
| --- | --: | --: | --: | --: | --: |
| Cockpit | 0.893% / 0.903% | 1.729% / 1.911% | 2.535% / 2.730% | 0.070% / 0.077% | 0.042% / 0.044% |
| Particles | 1.826% / 1.917% | 3.335% / 3.340% | 3.329% / 3.334% | 0.902% / 2.093% | 2.361% / 2.560% |

The native Camera suites also checked fixed pixels on all three backends. The tests exercise alpha and additive sprites over a red mesh, occlusion of a sprite behind it, and a green mesh crossing a curved painted panel. Moving the mesh away restored the complete previous panel capture byte for byte.

Local reports, binary identities, sampled CPU profiles and PNGs are under `target/performance/metal-gallery-iuno/`. The [benchmark guide](README.md#native-webgpu-comparison) describes the maintained runner.

## Direct Metal path coverage on Apple M4

On 27 November 2026, direct Metal reached 8,391 offscreen submissions/s on the live Glass terrain, compared with 7,283 before retaining projected path coverage (15.2% faster). Native WebGPU used the Metal adapter and GLES used ANGLE. The scene, render targets and assets were identical across backends. All three submitted 37,422 triangles per steady frame.

| Glass terrain at 960 × 760 | Steady submissions/s | Edit to frame p50 | Edit to frame p95 |
| --- | --: | --: | --: |
| Previous direct Metal | 7,283 | 6.07 ms | 6.74 ms |
| Direct Metal | 8,391 | 6.06 ms | 6.87 ms |
| WebGPU on Metal | 7,150 | 7.29 ms | 8.46 ms |
| GLES through ANGLE | 5,363 | 8.83 ms | 9.51 ms |

Direct Metal now shares WebGPU’s screen-space path bounds and retains the exact four-sample coverage of stable Plot paths in the root Camera. Each renderer owns its GPU buffer. Metal builds the mask after the projected paths repeat, draws changed paths analytically and applies the current depth test, clip and tint on each draw. The coverage buffer is capped at 4 MiB. The GUI board exercises the existing Canvas and Surface redraw paths.

The three release backends ran serially with rotated order. Terrain used three repetitions and pooled 48 measured 1,024-row edits per backend. The gallery and the 74-specimen GUI board used two repetitions per configuration. Each throughput window ran for at least three seconds and 96 frames. Captures were taken outside timing. The previous Metal executable was rerun for terrain and gallery with matching asset hashes. The board reference column uses its archived runs with the same board asset hashes.

### Demo gallery

Median steady offscreen submissions/s, with equal weight per scene in the aggregate.

| Scene            | Previous Metal |  Metal | WebGPU |  GLES |
| ---------------- | -------------: | -----: | -----: | ----: |
| charts           |          4,028 |  3,952 |  2,810 | 1,129 |
| charts-streaming |          2,424 |  2,374 |  1,989 |   913 |
| cockpit          |          1,074 |  1,073 |  1,845 |   973 |
| lighting         |          7,506 |  7,515 |  5,213 | 2,963 |
| particles        |          2,157 |  2,154 |  2,462 | 1,648 |
| platformer       |          4,443 |  4,446 |  3,367 | 2,973 |
| shapes           |         12,315 | 12,374 |  9,907 | 5,167 |

The geometric mean of the per-configuration Metal throughput ratios was 1.11× WebGPU and 1.98× GLES. WebGPU beat GLES in all seven scenes and led Metal on particles and cockpit. Across the gallery, Metal measured 0.5% below the uncached Metal rerun.

### GUI rendering stress board

Median steady offscreen submissions/s on the 74-specimen board at 2016 × 1152.

| Skin / projection  | Previous Metal | Metal | WebGPU | GLES |
| ------------------ | -------------: | ----: | -----: | ---: |
| cartoon/canvas     |            512 |   505 |    507 |  344 |
| cartoon/cylinder   |            511 |   509 |    505 |  209 |
| cartoon/flat       |            509 |   507 |    505 |  345 |
| cartoon/sphere     |            509 |   507 |    503 |  205 |
| cybertron/canvas   |            539 |   540 |    534 |  373 |
| cybertron/cylinder |            537 |   534 |    532 |  223 |
| cybertron/flat     |            537 |   536 |    531 |  367 |
| cybertron/sphere   |            536 |   536 |    530 |  222 |
| glass/canvas       |            517 |   519 |    512 |  350 |
| glass/cylinder     |            516 |   514 |    509 |  210 |
| glass/flat         |            516 |   517 |    507 |  347 |
| glass/sphere       |            514 |   513 |    507 |  185 |

The geometric mean of the per-configuration Metal throughput ratios was 1.01× WebGPU and 1.91× GLES. WebGPU beat GLES in all 12 configurations. Metal measured 0.3% below the archived uncached Metal board runs.

### Pixel checks for the Metal cache

All six new Metal terrain captures (before and after live edits) matched the previous Metal pixels exactly. WebGPU differed by at most 1/255 per RGB channel with matching alpha. The WebGPU/GLES count above 16/255 remained 20,245 pixels before the edits and 21,315 afterwards.

The table gives the median and maximum percentage of pixels above the same threshold of 16/255 in any RGB channel. The animated gallery scenes and the GUI board’s animated mark have independent Host clocks. The largest Metal/WebGPU gallery difference was 2.52% on particles. The raw results include all three backend pairs, repeated Metal captures, and the comparison to the old Metal binary.

| Workload | Pairs per comparison | Metal / WebGPU | Metal / GLES | WebGPU / GLES | Metal / previous Metal |
| --- | --: | --: | --: | --: | --: |
| Glass terrain | 6 | 0.0000% / 0.0000% | 2.8481% / 2.9215% | 2.8481% / 2.9215% | 0.0000% / 0.0000% |
| Gallery | 14 | 0.2160% / 2.5181% | 0.8413% / 3.2003% | 0.5535% / 3.1594% | 0.0338% / 2.5843% |
| GUI board | 24 | 0.0022% / 0.0023% | 1.9570% / 3.4243% | 1.9570% / 3.4243% | 0.0021% / 0.0023% |

Local release reports, build identities and PNG captures are under `target/performance/metal-path-coverage/{terrain,gallery,board}/`. Each directory includes its pixel comparisons. The [benchmark guide](README.md#native-webgpu-comparison) gives the maintained entry points.

## Glass terrain on Apple M4

These earlier measurements preceded the direct Metal path cache.

On 27 November 2026, the updated native WebGPU renderer reached **7,146 offscreen submissions/s** on the live 32 × 32 Glass height surface at 960 × 760. That was **1.33× GLES** and **98.1% of direct Metal**. WebGPU beat GLES in every repetition of the final three-backend comparison and in all five repetitions of the preceding implementation check.

| Renderer | Steady submissions/s | Edit to frame p50 | Edit to frame p95 |
| --- | --: | --: | --: |
| Previous WebGPU | 4,793 | 7.76 ms | 8.49 ms |
| WebGPU on Metal | 7,146 | 7.46 ms | 8.80 ms |
| GLES through ANGLE | 5,371 | 8.85 ms | 9.45 ms |
| Direct Metal | 7,285 | 6.08 ms | 6.80 ms |

The table uses median throughput from three serial release runs per renderer with 3-second timing windows. The three current backends ran in rotated order. The previous WebGPU executable, verified against its recorded hash, ran separately with the same fixture and assets. Edit percentiles pool 48 measured 1,024-row updates per renderer. The earlier five-repetition run measured 7,147 WebGPU, 5,389 GLES and 7,281 Metal submissions/s.

In a diagnostic build, hardware timestamps sampled the Camera render pass every 128 frames. Its median interval was 401 µs with both terrain and annotations, 169 µs with only the mesh, and 140 µs with only the projected annotations. Replacing just path coverage with a constant in a diagnostic shader reached 6,913 submissions/s and changed the image. With the exact coverage cache and blended-material specialization, the full pass measured 188 µs. GPU timestamps include overlapping pipeline stages across submissions. Production throughput and edit measurements ran with the probes removed.

WebGPU now keeps the four-sample coverage of stable projected Plot paths in GPU memory. A changed projection, draw stream, path binding or target size uses the analytic shader immediately. Stable frames rasterize the original triangles and apply the current scene depth, tint, clip and blend. In steady state all three backends submitted 37,422 triangles per frame. WebGPU issued 21 draws. Direct Metal issued 10 and GLES issued 74.

All six final WebGPU captures, before and after edits, matched the previous WebGPU image byte for byte. The largest RGB difference from direct Metal was 1/255. Compared with GLES, the count of pixels differing by more than 16/255 in any RGB channel stayed at 20,245 before edits (2.77%) and 21,315 after edits (2.92%). Alpha matched across backends.

Release reports, executable hashes, captures and pixel comparisons are under `target/performance/glass-wgpu-profile/final/`. The five-repetition comparison is under `comparison/`, and the previous executable's rerun is under `control/`. The [benchmark guide](README.md#native-webgpu-comparison) gives the maintained entry point.

## Native Metal adapter on 27 November 2026

The release WebGPU Host used wgpu 30.0.2 with its hardware Metal adapter on Apple M4. Direct Metal and GLES through ANGLE 2.1.28252 ran the same generated-client fixtures. Each workload ran twice per backend in serial, with rotated backend order. FPS counts uncapped offscreen Host submissions. Readback ran outside the timing window. Both presentation test suites passed, including a byte-for-byte comparison of a sparse sphere edit against a fresh full redraw. The WebGPU suite ran with the EGL and dynamic library environment variables unset.

| Workload | WGPU faster than GLES | WGPU / GLES | WGPU / optimized Metal | WGPU / committed Metal |
| --- | --: | --: | --: | --: |
| Demo gallery | 7 / 7 | 1.79× | 0.90× | 0.90× |
| 74-specimen GUI board | 12 / 12 | 1.89× | 0.99× | 1.12× |
| Live 32 × 32 terrain, steady | 2 / 3 | 1.30× | 0.73× | 0.73× |

Ratios are equal-weight geometric means of per-workload median throughput ratios. The committed Metal column uses the earlier matched two-repetition measurements from a clean build of `211e3fb` on the same Mac. The optimized Metal and GLES columns use the final three-backend sweep.

### Gallery at 1600 × 900

| Workload         | WGPU FPS | Metal FPS | GLES FPS |
| ---------------- | -------: | --------: | -------: |
| shapes           |    9,939 |    12,307 |    5,123 |
| lighting         |    5,204 |     7,496 |    2,955 |
| platformer       |    3,377 |     4,444 |    2,960 |
| particles        |    2,455 |     2,163 |    1,658 |
| charts           |    2,862 |     4,048 |    1,142 |
| cockpit          |    1,839 |     1,076 |      971 |
| charts-streaming |    2,014 |     2,421 |      934 |

### GUI board at 2016 × 1152

| Workload           | WGPU FPS | Metal FPS | GLES FPS |
| ------------------ | -------: | --------: | -------: |
| glass/canvas       |      509 |       517 |      348 |
| glass/flat         |      509 |       516 |      348 |
| glass/cylinder     |      510 |       516 |      209 |
| glass/sphere       |      506 |       514 |      187 |
| cybertron/canvas   |      532 |       539 |      370 |
| cybertron/flat     |      530 |       537 |      369 |
| cybertron/cylinder |      530 |       537 |      224 |
| cybertron/sphere   |      526 |       536 |      223 |
| cartoon/canvas     |      506 |       512 |      347 |
| cartoon/flat       |      504 |       509 |      343 |
| cartoon/cylinder   |      504 |       511 |      210 |
| cartoon/sphere     |      502 |       509 |      204 |

A separate Glass sphere control alternated the committed and optimized Metal binaries in main/optimized/optimized/main order. Median throughput went from 354 to 515 submissions/s. The sampled draw count went from 880 to 7.

### Live terrain at 960 × 760

Each edit changed 1,024 rows and waited for the commit and a newer frame. Edit p50 and p95 pool 32 measured updates per skin and backend.

| Skin | WGPU FPS | Metal FPS | GLES FPS | WGPU edit p50 / p95 (ms) | Metal | GLES |
| --- | --: | --: | --: | --: | --: | --: |
| glass | 4,801 | 7,294 | 5,382 | 7.83 / 8.46 | 6.18 / 6.73 | 8.89 / 9.72 |
| cybertron | 8,896 | 11,496 | 5,786 | 6.47 / 7.11 | 5.57 / 6.26 | 7.16 / 7.93 |
| cartoon | 8,821 | 11,572 | 5,501 | 5.90 / 6.88 | 5.04 / 5.76 | 6.75 / 7.20 |

### Capture comparison

The percentage counts pixels with an absolute difference above 16/255 in any RGB channel. Animated gallery scenes and the GUI mark run on independent Host clocks. Terrain compares matching dataset states before and after live edits.

| Workload | WGPU / Metal capture pairs | Median difference | Largest difference |
| --- | --: | --: | --: |
| gallery | 14 | 0.100% | 2.463% |
| board | 24 | 0.002% | 0.002% |
| terrain | 12 | 0.000% | 0.000% |

The local comparison, binary identities, raw samples, completed captures and before/after controls are under `target/performance/native-metal-wgpu-efw6/`. The prior clean-main run is under `target/performance/wgpu-main-211e3fb/`. The [benchmark guide](README.md#native-webgpu-comparison) gives the maintained commands.

## Historical comparison through ANGLE

Measured on 26 November 2026 on an Apple M4. The native WebGPU Host used Rust wgpu 30.0.2 over the GLES adapter and ANGLE 2.1.28252. The GLES Host used the same ANGLE Metal driver. The Metal Host called Metal directly. All three release Hosts ran uncapped, offscreen, with the same generated-client workloads, viewport and assets. FPS counts Host presentation submissions per wall-clock second. Readback and PNG encoding ran outside timing. [Reproduce the comparison](README.md#native-webgpu-comparison).

### Demo gallery

Median FPS from three serial, order-rotated repetitions at 1600 × 900. Each scene warmed for at least 2.5 seconds and 30 requests, then measured at least three seconds and 96 requests. Streaming charts had live dataset updates.

| Scene | WebGPU FPS | GLES FPS | Metal FPS | WebGPU / GLES | WebGPU / Metal |
| --- | --: | --: | --: | --: | --: |
| Shapes | 7,287 | 5,084 | 11,458 | 1.43× | 0.64× |
| Lighting | 4,466 | 2,955 | 7,402 | 1.51× | 0.60× |
| Platformer | 3,502 | 2,951 | 3,935 | 1.19× | 0.89× |
| Particles | 2,272 | 1,653 | 1,993 | 1.37× | 1.14× |
| Charts | 1,811 | 1,144 | 1,968 | 1.58× | 0.92× |
| Streaming charts | 1,299 | 896 | 1,357 | 1.45× | 0.96× |
| Cockpit | 1,540 | 968 | 1,033 | 1.59× | 1.49× |

WebGPU exceeded GLES in all 21 paired windows. The geometric mean of the per-scene median throughput ratios was 1.44× GLES and 0.91× Metal. Metal had its largest leads in lighting and shapes. WebGPU led on cockpit and particles.

### GUI rendering stress board

The same 74-specimen board used for the Metal comparison ran at 2016 × 1152. A Host animation moved a mark across otherwise fixed controls and plots. Each skin and projection started a fresh Host. WebGPU retained unchanged pixels and repainted the damaged region with its overlapping content. The table gives FPS from the final three-backend sweep. [Board workloads and settings](gui-rendering.md).

| Skin and projection | WebGPU FPS | GLES FPS | Metal FPS | WebGPU / GLES | WebGPU / Metal |
| --- | --: | --: | --: | --: | --: |
| Glass canvas | 498 | 350 | 430 | 1.43× | 1.16× |
| Glass flat | 496 | 345 | 427 | 1.44× | 1.16× |
| Glass cylinder | 451 | 189 | 369 | 2.39× | 1.22× |
| Glass sphere | 455 | 159 | 328 | 2.86× | 1.39× |
| Cybertron canvas | 475 | 344 | 480 | 1.38× | 0.99× |
| Cybertron flat | 514 | 367 | 506 | 1.40× | 1.02× |
| Cybertron cylinder | 512 | 222 | 419 | 2.31× | 1.22× |
| Cybertron sphere | 511 | 223 | 415 | 2.29× | 1.23× |
| Cartoon canvas | 485 | 343 | 434 | 1.42× | 1.12× |
| Cartoon flat | 482 | 341 | 432 | 1.41× | 1.12× |
| Cartoon cylinder | 480 | 207 | 361 | 2.32× | 1.33× |
| Cartoon sphere | 479 | 204 | 361 | 2.34× | 1.33× |

WebGPU exceeded GLES in 12 of 12 final GUI configurations. The geometric mean was 1.84× GLES and 1.18× Metal. The preceding three-backend sweep measured 2.00× GLES and 1.51× Metal. An earlier two-repetition run, before the final gallery optimizations, measured 1.81× GLES and 1.19× Metal.

### Validation and evidence

The native Metal and WebGPU tests exercised nested Cameras, Canvas clipping and ordering, separated layers, shadows, custom paints, resizing and damaged-region redraws through the real client and independently checked captured pixels. Across the final GUI captures, at most 0.079% of pixels differed from Metal by more than 16/255 in any RGB channel. Animated gallery scenes ran on independent Host clocks.

After a clear-value correction for unpainted texels on separated layers, all three backends ran the full gallery and GUI board again. WebGPU again exceeded GLES in all seven scenes. It showed a wider spread in absolute rates than the order-rotated repetitions. The timed workloads use coincident layers. The separated-layer fixture passed on the corrected build.

Local reports under `target/performance/` retain executable and asset hashes, run order, raw samples and captures. The three-repetition gallery report is `webgpu-gallery-m4/report.json`. The final source confirmation reports are `webgpu-gallery-m4-owner/report.json` and `webgpu-gui-m4-owner/report.json`. The combined tables and pixel differences are in `webgpu-composition-m4/`.
