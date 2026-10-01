# Ordinary skin motion transport and pixels

This fixture authors ordinary Canvas, GUI control, skin and motion components through each runtime's generated client. Paint keys and row layouts come from that exact target contract; animation assets use the generated immutable clip encoder. No GuiRoot, React adapter, private input admission, raw World mutation or client clock advancement participates.

Run the maintained selections:

```sh
python tools/ipp.py regression --only test:gui-motion:development
python tools/ipp.py regression --only test:gui-motion:production
IPP_EGL_LIBRARY_DIR=/lib64 python tools/ipp.py regression --only test:gui-motion:native
```

The two Chromium cases bundle the actual worker entry point in development and production modes and execute the GUI-enabled WASM runtime with WebGL. Native uses the existing WebSocket/GLES Host and its completed-presentation channel. The build typechecks the shared scenario against the public SDK without pulling the unrelated legacy whole-suite TypeScript frontier into its prerequisite. The environment runner uses the established browser/native lifecycle and artifact harness, and executes rather than substituting a mock transport or renderer.

## Causality and assertions

A child World owns the animated Canvas. A parent World presents it through a Surface attachment, first inside Canvas and later through an orthographic Camera. Every action terminal is followed by a distinct authored marker color in the same child Canvas. A capture only passes its fence when that marker appears in the actual pixels: the control and marker then belong to the same completed child paint publication, whose mutation order includes the action. Evidence records the real effect identity/tick, marker mutation tick, physical capture sequence and root publication independently. It does not invent a child observed tick from a fresh parent draw or from render statistics.

The open-batch case deliberately retains the previous marker and animated appearance. It asserts frozen pixels and absence of the unfinished marker; those captures are not counted as new motion completions. The resumed capture must contain the new marker and the current static appearance after motion-component withdrawal.

Color assertions sample an independently positioned 128-pixel background region outside the checkbox indicator. Marker expectations apply the standard linear-to-sRGB transfer function independently of the renderer. Focus assertions cover the authored border, including continuous Blur and interrupted refocus. PNGs and exact capture metadata are written as each assertion succeeds; the last capture is also saved on a pixel timeout. Terminal and mutation evidence is persisted before later assertions can fail.

Interruption checks consume the **first** capture containing the post-action marker, save it, and assert continuity without retrying a failed motion sample. Only marker readiness can be polled for those captures. A prior child frame observation precedes a separate origin marker; an ordinary marker mutation after the first capture supplies the upper frame threshold. These observations conservatively enclose both sampled child publications without claiming either frame event is the capture's exact tick. The linear-channel travel bound is that entire Host-time interval divided by the authored linear transition duration, plus 0.025 pixel/transfer tolerance. The permitted composite-origin interval must be disjoint from a full-blue restart interval (background reversal) or a zero restart interval (refocus). If latency makes those intervals overlap, the test fails as insufficient timing evidence, even if the pixels look plausible. No frame request advances the Host clock.

The override case starts unchecked and red, installs the blue override and motion destination, and requires mixed red/blue pixels: an immediate static-blue fallback cannot pass. It then removes only the motion component while retaining the override and requires static blue in the first included capture. Removing the override separately reveals unchecked red. Control lifetime and value must remain unchanged throughout those appearance edits.

The scenario also checks source-ready replacement, same-batch theme retarget plus old-theme deletion, per-control override precedence, withdrawal, component-incarnation replacement and stale action rejection. Focus changes preserve committed values. A camera-only mutation must move the child's image by the independently calculated two pixels without rebuilding, reallocating or uploading its retained GUI geometry.

Retention measurements begin after the authored transition duration has elapsed on actual child frame events and the renderer's single compatible box run has coalesced. The renderer intentionally keeps recently changed primitives in volatile batches before merging them; that one-time merge is not an unchanged-frame regression. During the measured interval, subsequent child frames and completed captures must leave cumulative rebuild/allocation/upload counters unchanged. Diagnostics are observations, never capture fences. These renderer counters do not expose ordinary GUI preparation, sampling or text-measurement counts; the separate core motion suite owns those exact sparse-work assertions.

## Resource and coverage boundaries

Workers additionally use a real HTTP animation asset whose response is held by the existing artifact-server hook. The test observes the actual request, proves last-ready appearance across completed captures, then releases the bytes and observes an intermediate transition and endpoint. The native test Host deliberately has only its existing builtin/client-source providers, not HTTP streaming; its replacement is ready, and it makes no pending-network claim. Adding native streaming would require a separately agreed Host test-provider seam, not a fabricated ready/pending signal.

Explicit provider revocation and motion-owned demand release remain covered by the real headless core lifecycle tests. The public client source release operation preserves actual consumers and must not be relabelled as revocation. No new production revocation API is introduced for this fixture. The pixel gate case establishes frozen/resumed presentation, not private resource-owner counters.

This is ordinary semantic SDK input, not composed physical routing, native capture/IME, React callbacks, scrolling, VirtualList, projection-query acceptance or a performance benchmark. Failure logs retain these distinctions. Existing core motion tests supplement this real transport/rendering evidence.
