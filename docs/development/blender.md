# Blender Development Environment

[Strategy](../plans/blender-integration.md) · [Authoring](../architecture/authoring.md) · [Addon guide](../../integrations/blender/README.md)

This guide owns development prerequisites, local launchers and certificate setup. The addon guide owns installation and operation; [exporter scope](../../integrations/blender/ipp_blender/EXPORTER.md) owns translation support. Environment probes supplement the real addon → browser → IPP rendering suite.

## Local Blender installation

Use the selected Blender 5.2 LTS. [Pipeline setup](../../tools/pipeline/toolchains.json) pins the tested Linux x64 archive and checksum used by CI; [addon wheels](../../integrations/blender/wheels-linux-x64.json) target Blender Python 3.13. General repository tools use the [build-guide toolchain](building.md). Set `BLENDER_BIN` when `blender` is not on PATH; the invoking Python interpreter is propagated to maintained harnesses.

```sh
blender --version
blender --background --factory-startup --python-exit-code 1 --python-expr 'import bpy; print(bpy.app.version_string, bpy.app.background)'
python tools/ipp.py setup blender
```

On the development VM, `/home/dev/.local/bin/blender` wraps `/home/dev/.local/opt/blender-5.2.1-linux-x64` with private support libraries from `/home/dev/.local/opt/ipp-blender-support`. These paths are local setup, not repository-installed dependencies.

Background extraction uses main-thread data APIs. Blocking command loops do not run Blender timers; explicitly pump the same addon export/flush logic used by the GUI. GUI behavior needs a normal application event loop.

## Running the maintained suites

After `python tools/ipp.py setup node`, install Chromium/dependencies as described in the [build guide](building.md) and prepare [trusted test certificates](#local-certificates-and-viewer-onboarding):

```sh
python tools/ipp.py test blender
python tools/ipp.py test particles-blender
python tools/ipp.py dev blender-viewer
```

The [suite registry](../../tools/pipeline/suites.json) builds viewer/addon prerequisites, checks isolated extension installation and runs real Blender HTTPS/WSS export with browser state/frame assertions. It also includes disk export/restore and separate particle export coverage. The [fixture guide](../../tests/fixtures/blender/README.md) owns source provenance and regeneration.

On this VM, `/home/dev/.local/bin/ipp-browser-env python tools/ipp.py test blender` supplies private Chromium libraries/fonts from `/home/dev/.local/opt/ipp-browser-support`. Ordinary installations use Playwright's system dependencies. This wrapper changes the child environment only.

## GUI on a virtual display

The VM's `/home/dev/.local/bin/blender-xvfb` owns an Xvfb display and Blender's GUI loop, selects Mesa/llvmpipe and cleans up both processes on exit/cancellation:

```sh
/home/dev/.local/bin/blender-xvfb --factory-startup
```

Use the checked native framebuffer capture rather than assuming `Window.screenshot` contains rendered pixels. Repeat the maintained local smoke probe with an isolated configuration:

```sh
IPP_BLENDER_PROBE_OUTPUT=/home/dev/.local/state/ipp-blender/validation-5.2.1 \
BLENDER_USER_CONFIG=/home/dev/.local/state/ipp-blender/validation-5.2.1/config \
/home/dev/.local/bin/blender-xvfb --factory-startup -noaudio --window-geometry 0 0 1280 800 \
  --python-exit-code 17 \
  --python /home/dev/.local/opt/ipp-blender-support/probes/gui_smoke.py
```

The probe writes a native image and structured results. Software rendering establishes event-loop/fixture behavior; it does not establish hardware acceleration or artist responsiveness. Check actual VM/container topology and host GPU use before configuring shared access or passthrough. Display-free `gpu.init()` alone does not prove viewport capture.

## Installed Blender Lab MCP

The VM has an isolated Blender Lab MCP installation under `/home/dev/.local/opt/ipp-blender-mcp`, pinned to revision `5181fa06d5c601e910eb680a0012d20d1203b8d5`. `/home/dev/.local/bin/ipp-blender-mcp` launches its stdio bridge and a persistent background Blender with the profile under `/home/dev/.local/share/ipp-blender-mcp/blender-profile`. Logs live under `/home/dev/.local/state/ipp-blender-mcp/session-*`.

This is optional interactive tooling, outside production and CI. Preserve its isolated environment; similarly named third-party packages are not interchangeable. The launcher supplies required online mode and stops its children when the connection closes. Save `.blend` files explicitly to retain edits across sessions.

Use persistent `execute_blender_code` with a JSON-serializable `result`; `*_for_cli` operations start separate processes. Background mode has no normal editor navigation and rejects deferred `check_is_finished`. Inspect both `isError` and structured `status`, since transport success alone does not prove Python success.

```sh
/home/dev/.local/opt/ipp-blender-mcp/venv/bin/python /home/dev/.local/opt/ipp-blender-mcp/validate.py
python3 /home/dev/.local/opt/ipp-blender-support/probes/gui_mcp_smoke.py
```

These local probes check persistence, save/reopen, expected failures, image content and child cleanup. The GUI probe copies the isolated profile. Convert useful investigations into maintained repository scenarios; Blender-rendered thumbnails do not prove IPP output.

## Local certificates and viewer onboarding

The addon uses configured certificate/key paths when supplied and reports invalid credentials. Otherwise it generates and retains a self-signed localhost certificate. Artists need no certificate-generation tools. A browser-visible HTTPS origin serves onboarding, assets and WSS; browser trust, CORS/Origin policy and local-network permission are separate requirements.

Automated tests use mkcert and actual browser trust. Install mkcert and the platform trust-store support (Linux Chromium commonly needs NSS/certutil), then:

```sh
python tools/ipp.py setup certificates
# Explicit one-time trust installation in the browser's environment:
python tools/ipp.py setup certificates --install-trust
```

[Certificate tooling](../../tools/blender.py) creates/reuses ignored `target/blender/certificates`. `MKCERT_BIN` selects mkcert; `CAROOT` overrides the default `~/.local/share/ipp/blender-ca`. Never commit private keys. A half-present certificate/key pair fails explicitly; restore it or select a new directory. CI demonstrates its isolated trust-store setup.

Interactive onboarding opens the addon's HTTPS page so the user can accept a permitted certificate warning before redirecting to the viewer. This neither installs a CA nor grants trust in other profiles. Policies forbidding exceptions require configured trusted credentials. External browsers need their own trust and a tunnel preserving the certificate name. Certificate bypass flags do not validate onboarding.

Validate redirect, page/worker fetch, WSS, ETag/If-Match access and certificate replacement in the actual browser. Historical VM probes live under `/home/dev/.local/state/ipp-blender/`; their success is scoped to the recorded build/profile. Repeat affected checks after environment upgrades, and retain browser/Blender identities, logs and captures under the [testing policy](integration-testing.md).

The [opt-in performance scene](../../tests/performance/stress.md) combines baked rigid bodies, Rigify walking, parented lights and live/baked particles. Generate and profile it with `python tools/ipp.py benchmark`; it is excluded from regression runs.

For hardware WebGL on this VM, select `IPP_BROWSER_ANGLE=vulkan` (ANGLE/RADV) or `IPP_BROWSER_ANGLE=gl-egl` (ANGLE/radeonsi) with the `ipp-browser-env` wrapper. Both have been verified on the Radeon RX 9070 XT. Follow the [hardware profiling commands](../../tests/performance/stress.md#hardware-webgl-and-addon-profiles) to repeat the renderer check; a successful default headless launch alone can still use SwiftShader.

The [streaming performance harness](../../tests/performance/blender-stream.md) exercises the opt-in fresh-import path and compares full and streamed timing and images. Ordinary live updates continue to use full revisions.
