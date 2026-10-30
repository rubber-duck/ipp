# Native CEF engine

This optional native GLES Host engine owns CEF browser objects and helper processes. The [Host adapter](../examples/cef/provider.rs) owns scoped source names, exact producer requests, input and message pumping. [Web-content architecture](../../../docs/architecture/web-content.md) owns the lifetime and immutable-publication contracts.

The implemented platform is macOS Apple Silicon with ANGLE Metal. Linux keeps its separate maintained DMA-BUF prototype; connecting that producer to the native demo remains separate work. No CEF Cargo feature, new renderer backend or production Rust dependency is introduced. The engine is loaded only when explicitly selected.

```sh
brew install cmake ninja
python tools/ipp.py build cef-engine gles-host
node tools/shared-host/shared-host.mjs host start --gallery --window --cef
node tools/shared-host/shared-host.mjs gallery cockpit --name demos
node tools/shared-host/shared-host.mjs session action demos carousel 3
python tools/ipp.py regression --only test:native-cef:macos --only check:cef-cache
```

Setup follows CEF's [macOS application/helper bundle requirements](https://github.com/chromiumembedded/cef/blob/master/docs/tutorial.md) and the pinned binary distribution's CMake instructions. The [installer](../../../tools/pipeline/cef.py) verifies the archive checksum and keeps one immutable runtime installation under a shared lease. Source worktrees compile their own wrapper and engine. Launchers verify the shared runtime, copy the small engine/helper bundle into their private snapshot and retain a shared installation lock until shutdown. Browser profiles are process-owned temporary directories, removed after orderly shutdown. No runtime download occurs.

The renderer imports CEF's callback-borrowed IOSurface through [EGL_ANGLE_iosurface_client_buffer](https://chromium.googlesource.com/angle/angle/+/main/extensions/EGL_ANGLE_iosurface_client_buffer.txt), normalizes premultiplied sRGB into private immutable textures, and finishes GPU reads before returning. It never uses CPU OnPaint. The callback only fills pre-reserved GPU storage; Host-boundary admission rechecks request and context authority. Capture readback belongs to the test/presentation API, not the producer path.

The development bridge accepts local HTML beneath the explicitly configured page root. Each registration has a fresh source authority; source or extent replacement creates a new browser incarnation. Pages receive pointer, wheel, committed text and logical key input through the existing client-managed GUI target. Navigation stays client-owned. Native popup composition, IME and clipboard integration are not implemented. The current producer renders at one pixel per logical page unit, independently of desktop display density.

The maintained native scenario runs the actual helper processes, generated WebSocket client, GUI input routing, CanvasHtml and GLES captures. It checks orientation and alpha, nested canvas, pointer/text/key input, client-owned navigation, the cockpit's form behavior, replacement, odd-sized resize, context recovery, withdrawal, reconnect and orderly shutdown. Its artifacts live under `target/integration-artifacts/native-cef/`. The macOS workflow also covers the window and ordinary offscreen clients; Linux execution is separate evidence.

The initial arm64 Release engine is about 892 KiB and its helper about 856 KiB; the shared CEF framework is about 320 MiB on disk. CEF's C++ wrapper is statically linked into these optional artifacts, not the Rust Host. Re-measure after changing the pinned runtime or toolchain.
