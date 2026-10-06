# GUI skin motion transport and pixels

This fixture authors ordinary Canvas, GUI control, theme and motion components through each runtime's generated client, and drives the built-in looks' transitions with physical pointer input. Paint keys, built-in looks and row layouts come from that exact target contract. Its last part declares a World through `@ipp/react` and its GUI kit, as an application does. No private input admission, raw World mutation or client clock participates, and no client timer measures motion.

```sh
python tools/ipp.py test gui-motion --egl-dir /lib64 --software
```

The two Chromium cases bundle the actual worker entry point in development and production modes and execute the GUI-enabled WASM runtime with WebGL. Native uses the existing WebSocket/GLES Host and its completed-presentation channel. The build typechecks the shared scenario against the public SDK. The environment runner uses the established browser/native lifecycle and artifact harness, and executes rather than substituting a mock transport or renderer; the page loads the shared GUI font from the `font-assets` product for the kit's spinner.

## Default transitions over Host time

An unthemed button and a checkbox themed from the exported switch look, rows and motion rows alike, sit in a canvas presented as the root, under an inherited font four times the looks' `em`, so every line and part is several pixels wide. Physical pointer input hovers, presses and releases the button and clicks the switch on and off; a preference command then turns on reduced motion and the hover lands at once.

Each capture names the World tick it drew, and the scenario records every frame event's tick and Host time. A transition's elapsed time at a capture lies between the frame before the first capture that changed and the last frame observed before the input; each sample must lie inside the eased interpolation of the settled ends over that interval: the button's line for hover in (80 ms) and out (120 ms), its fill for the immediate press and the 100 ms release, and the switch block's position for its 160 ms ease-out cubic travel either way. An immediate change has no intermediate capture; a timed one must have one. Every switch capture is saved with its tick, Host time and bounds.

## A theme's own transitions

A child World owns a checkbox themed with plain boxes and motion rows of four seconds, linear. A parent World presents it through a Surface attachment, first inside Canvas and later through an orthographic Camera. Every action terminal is followed by a distinct authored marker color in the same child Canvas. A capture only passes its fence when that marker appears in the actual pixels: the control and marker then belong to the same completed child paint publication, whose mutation order includes the action. Evidence records the real effect identity/tick, marker mutation tick, physical capture sequence and root publication independently.

Color assertions sample an independently positioned 128-pixel background region outside the checkbox indicator. Marker expectations apply the standard linear-to-sRGB transfer function independently of the renderer. Focus assertions cover the authored border, including continuous Blur and interrupted refocus.

Interruption checks consume the **first** capture containing the post-action marker, save it, and assert continuity without retrying a failed motion sample. A prior child frame observation precedes a separate origin marker; an ordinary marker mutation after the first capture supplies the upper frame threshold. The linear-channel travel bound is that entire Host-time interval divided by the transition duration, plus 0.025 pixel/transfer tolerance. The permitted composite-origin interval must be disjoint from a full-blue restart interval (background reversal) or a zero restart interval (refocus); otherwise the test fails as insufficient timing evidence.

The scenario also checks a same-batch theme retarget with old-theme deletion, that a per-control override restyles a settled control in the first capture that includes it and so does its removal, that reduced motion snaps a transition under way and reads back through the `guiPreferences` query, component-incarnation replacement and stale action rejection. Focus changes preserve committed values. A camera-only mutation must move the child's image by the independently calculated two pixels without rebuilding, reallocating or uploading its retained GUI geometry.

Retention measurements begin after the transition duration has elapsed on actual child frame events and the renderer's single compatible box run has coalesced. During the measured interval, subsequent child frames and completed captures must leave cumulative rebuild/allocation/upload counters unchanged. Diagnostics are observations, never capture fences. The core motion suite owns the exact sparse preparation and sampling work assertions.

## One reduced-motion setting

A third World's React root declares the kit's `GuiKit`, an unthemed button at the default-motion button's rectangle and a kit spinner beside it, and sets `reducedMotion` on that `GuiKit` alone. Turned on, the kit sends the World's preference, which reads back through the `guiPreferences` query; the World holds no animation controller, the spinner's lit quarter stays from twelve to three o'clock, byte for byte, over more than a turn of Host time, and the hover lands without an intermediate capture. Turned off, the preference reads back off, one controller turns the spinner until a capture differs, and the hover takes its 80 ms inside the same eased envelope as the unthemed button's. The spinner's still and turning captures are saved.
