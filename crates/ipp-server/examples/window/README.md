# Development Host window

This Host-owned adapter supplies a native EGL window, a bounded physical-input queue and main-thread event pumping. It does not add a renderer backend, advance World time or link a window toolkit into `ipp-core`. The normal GLES product remains usable offscreen.

On macOS the pipeline compiles `macos.m` with Apple clang and the system AppKit/QuartzCore frameworks into `libipp-window.dylib`, beside `gles_host`. The shared-host launcher copies and fingerprints it with the executable. `macos.rs` loads the shim only for `--window`. The renderer drops first, then EGL, then the window and shim. ANGLE owns Metal rendering through its borrowed CAMetalLayer. No new Cargo dependencies or features are needed.

## Linux handoff

Replace `linux.rs` with the actual platform implementation. Its current `open` returns an explicit error; the offscreen Linux path is unchanged. Preserve the `Window` interface declared by the two platform modules: open on the Host thread, lend the native EGL surface handle, resize the drawable for the acknowledged viewport (including render scale), pump owned events, and release only after EGL teardown. Extend the shared EGL loader's display/config selection for the chosen Linux window system; its existing surfaceless display is only suitable for offscreen work. Keep any toolkit/library packaging in this Host distribution.

`WindowEvent` in `mod.rs` is the platform-neutral record. The development channel explicitly encodes it, independently of C struct layout. Coordinates have a top-left origin and are normalized to the content area; wheel deltas are logical pixels. Resize reports physical dimensions and display scale. Key codes and text routing are in `tools/shared-host/window.ts`. Queues are bounded at 256 records and overflow clears physical capture through blur. Window close signals the existing server loop to return and release owned resources. New sessions receive current metrics.

The gallery client holds the existing presentation lock while the window is visible. It sends input through the generated Host client, observes frame completion for readiness, and resizes the selected root through the common presentation API. The Host continues drawing independently; no framebuffer streaming or client simulation stepping is involved.

Instrumentation builds expose test events through the development channel, delivered through the actual platform event queue. Production builds reject them. The maintained `native-window` suite verifies pixels across the drawable, GUI selection by native clicks, resize, render scale, reload and close; extend its process/platform driver for Linux while preserving those observations. macOS currently handles committed keyboard text, without marked-text IME composition or clipboard integration.
