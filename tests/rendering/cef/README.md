# Chromium Surface prototype

A real CEF browser renders local HTML, CSS and an HTML canvas into IPP's native GLES Surface bitmap program. This continues the earlier `ipp-fnir.12` synthetic-pixel experiment with actual Chromium accelerated paint. The browser's temporary DMA-BUF is imported, copied on the GPU into an IPP-owned sRGB texture, and released before the paint callback returns. CPU `OnPaint` fails explicitly; framebuffer readback is used only for verification captures.

The experiment is an isolated test executable with its own pinned Cargo lockfile. CEF is not added to production crates, the generated protocol or browser/WASM builds. The production `CanvasHtml` leaf, dynamic texture registry, World/session lifecycle and WebContentService are **not integrated by this prototype**. See [rendering ownership](../../../docs/architecture/rendering.md) and Beads `ipp-fnir.10`–`ipp-fnir.14` for the separate integration work.

## Run

Requires Linux, Rust from the repository toolchain, a hardware GLES driver supporting EGL DMA-BUF modifier import, and Weston 15 with its headless GL renderer and kiosk shell. CEF's ordinary Linux shared-library dependencies must be installed. The maintained runner resolves one exact, verified CEF installation under `IPP_CEF_CACHE` (default `~/.cache/ipp-cef`). It adopts the existing `~/.cache/ipp-cef-154` distribution in place, or a matching directory named by `IPP_CEF_RUNTIME`/`CEF_PATH`. A missing installation downloads the pinned archive once; the initial download is large.

```sh
python tools/ipp.py regression --suite cef-surface --egl-dir /usr/lib64
```

For an extracted Weston installation, set `IPP_CEF_WESTON_ROOT` to its root directory (containing `usr/bin/weston` and `usr/lib64`). On the development machine used for this prototype:

```sh
IPP_CEF_WESTON_ROOT="$HOME/.cache/ipp-cef-display/root" \
  python tools/ipp.py regression --suite cef-surface --egl-dir /usr/lib64
```

The runner starts its own private display/socket, supplies a fake input seat, and tears down its browser and compositor process groups on success, error or timeout. Chromium 154's Wayland cursor teardown needs a seat; Weston `--fake-seat` supplies it. Private, unavailable D-Bus addresses and `--password-store=basic` isolate it from desktop credential/session services. It never connects to or stops another development Host. A pre-existing desktop display is unnecessary.

The suite belongs to the opt-in `cef` regression group and is also selectable through `python tools/ipp.py regression --suite cef-surface --egl-dir /usr/lib64`. It is separate from the `gles` and `rendering` groups because it requires the separate Chromium distribution and Weston environment. `--full` includes this suite and its standalone formatting, Clippy and cache checks. Changes to its own sources or the exact included shaders/EGL support select it through affected checks; ordinary unrelated renderer edits do not.

Completed-frame RGBA/PNG captures, Chromium/Weston logs, source/environment identity and machine-readable producer/runner results are retained under `target/cef-surface/<run>/`. `initial.png` and `clicked.png` show the button-driven DOM and canvas update. `resized.png`, `recovered.png` and `transparent.png` show resize, navigation recovery and alpha composition. The runner prints the exact evidence directory, including on failure.

## Evidence and boundaries

The scenario verifies independently specified colour regions and orientation, a real forwarded mouse click changing the DOM and canvas chart, texture reallocation after resize, a real failed file navigation followed by recovery, and premultiplied sRGB conversion followed by linear alpha blending. Browser closure and zero CPU paint callbacks are required. The scenario assertions are separate from launch, CEF handlers and EGL import so another driver can reuse the same observations.

Validated with CEF 154.0.34 / Chromium 154.0.8037.98, Mesa 26.1.8 and AMD Radeon RX 9070 XT. This establishes correctness in that environment, not throughput, cross-platform behavior or full runtime integration. The renderer uses IPP's actual bitmap shader, native DMA-BUF device adapter and completion fences; this is a direct-renderer experiment, not generated-client/transport coverage.

The callback copies the full frame and waits with `glFinish` before returning the pooled CEF buffer. This conservative synchronization establishes ownership; a production adapter needs measured synchronization, frame scheduling and resource budgeting. Inserting a fence and immediately returning is insufficient: CEF can reuse the borrowed buffer before the copy completes. The fixture accepts full-view images up to 4096 pixels per axis, restricted to the proven **BGRA, one plane, linear modifier** path. RGBA, multi-plane/DCC and external-only modifiers are rejected explicitly. Cropped buffers, browser popups, navigation UI and general input/IME are outside this fixture. CPU pixel transport and fallback are absent.

The local fixture runs with CEF's sandbox disabled for container execution. It is not a general browser for untrusted pages. The runner accepts no arbitrary page URL.

Chromium's own headless backend failed to allocate shared capture buffers on this machine. To diagnose that path explicitly (a nonzero exit means unavailable, not a passing test):

```sh
python tests/rendering/cef/run.py --headless
```

## Source and checks

The [driver](driver.rs) owns CEF callbacks and lifecycle, [renderer](render.rs) converts borrowed CEF metadata and invokes the production GLES device adapter before ordinary IPP draws, and [scenario](scenario.rs) owns pixel assertions. The test reuses the renderer's maintained EGL context support. The `cef` regression group includes dedicated standalone formatting, Clippy and cache mechanics checks; these can be selected without launching CEF:

```sh
python tools/ipp.py regression --only check:cef-format --only check:cef-clippy --only check:cef-cache
```

The shared [installation manager](../../../tools/pipeline/cef.py) verifies archive identity and headers, compares adopted files in place with the checksum-verified archive, records initial content hashes and checks file metadata on warm use. It publishes a completed installation atomically under a process lock, keeps versions side by side, and holds shared leases throughout each build/run. Concurrent worktrees have independent Cargo targets and browser profiles. Normal cleanup never prunes an installation. The explicit `python tests/rendering/cef/run.py --prune` operation refuses live consumers; it retains repair archives and never deletes adopted external files. Use `--setup --offline` to verify warm setup without a CEF download. `--build --offline` and `--clippy --offline` also require Cargo's own crates/index to be available offline.

The checked-in [pin](runtime.json) and [small upstream build-script patch](cef-dll-sys-build.patch) materialize one checksum-verified patched `cef-dll-sys` package in the shared cache. The patch only skips Linux runtime staging when `IPP_CEF_EXTERNAL_RUNTIME=1`; normal upstream validation, linking and other platform behavior remain. Cargo receives an explicit path patch for both build and Clippy. Registry files are untouched, generated bindings are not vendored, and multi-gigabyte runtime files are never copied into Cargo targets. Use the runner for compilation/linting so its lease and patch configuration apply. After root path dependencies change, run `python tests/rendering/cef/run.py --relock --offline` (omit `--offline` if Cargo needs new crate metadata). This refreshes the standalone lock with the same patch configuration and aligns unambiguous shared registry versions to the root lockfile; incompatible constraints fail for review. The root lockfile is never changed. Runtime acquisition retains its verified archive for offline reinstallation; legacy archives are reused in place.

Paint callbacks carry no request/navigation token. The prototype's expected-pixel waits verify its fixture, but coded dimensions and load counters alone cannot fence a same-size replacement or A→B→A resize in production. Capture timestamps/counters are retained in `run.log` as observations, without claiming request correlation. The [device adapter](../../../crates/ipp-render-gl/src/services/render/device/gles/dma_buf/mod.rs) caches Host-supplied EGL entrypoints and verifies the current display/context. Each copy fills a newly allocated private texture, preserves touched GL state and completes borrowed-buffer reads before callback return. The [state sentinels](state_sentinels.rs) assert immediate state preservation on successful copies and rejected metadata/destinations/imports, plus real completion-fence retirement; the subsequent bitmap captures verify actual pixels. These checks do not establish throughput or full Host/World integration.

CEF's [accelerated paint contract](https://github.com/chromiumembedded/cef/blob/master/include/cef_render_handler.h) requires reopening the handle and copying into application-owned storage before callback return. The pinned Rust bindings carry that contract. The [CEF Rust project](https://github.com/tauri-apps/cef-rs) documents runtime distribution setup.
