# VESPER scanner

This gallery world is a local sci-fi scanner app projected onto a translucent 3D Surface. It opens with a centred sign-in card, connects through a short simulated terminal sequence, then presents a large radar sweep with receiver gain, sweep rate, a vertical range control and a guarded charge-and-fire action. Any access phrase works: the demo makes no authentication or network request and never saves the phrase in gallery options. All application controls live in the scene; the outer gallery keeps navigation and camera controls.

The [mini app](mini-app.tsx) uses the runtime's retained GUI and [`@ipp/react/gui-kit`](../../../../packages/ipp-react/README.md#gui-kit). Its [local session](app-state.ts) owns phase, progress and terminal history in the shared [scene store](store.ts). Loading progresses on completed Host frames; the login fade and the [radar sweep](radar.tsx) use ordinary Host-owned animation. Reveal changes the text input's masked presentation while preserving its real text and editor. Submitting disables the fading form, and closing the scanner or disposal fences pending work and clears the phrase.

The scanner is a framed window with its own titlebar and Close control. Labeled kit panels provide padded Sweep, Receiver, Pulse Preparation and Interlock groups; the bottom-left footer opens Log and Settings, and the log drawer stays above those actions.

Settings groups display, projection and scene inspection in one modal. DISPLAY controls the window's Surface, render mode, style and exploded layer spacing; PROJECTION adjusts the beam, studio lights and colour; SCENE selects projector parts and visibly highlights their baked textures. Settings retains ordinary modal focus and Escape behaviour. The compact layer controls have a background without a border.

## Presentation and interaction

The default camera frames the holographic Surface in the scene through login, connection and the workspace. Login and connection use opaque dialog backgrounds over the translucent projection. Phase changes and layer inspection preserve the camera pose; Reset camera explicitly frames the scene or exploded view. Physical input routes panel gestures to the GUI and leaves background gestures to the camera.

Layers coincide at rest and preserve their logical paint/input priority. The login fade retains its layer ordering above the progress screen. EXPLODE and the LAYER STEP dial retarget ordinary Host animation from the current physical spacing, with the shield following the same motion. Reduced motion snaps transitions. Occupied groups remain together on flat planes or curved shells; menus and Settings retain their overlay ordering. See the [GUI](../../../../docs/architecture/gui.md) and [rendering](../../../../docs/architecture/rendering.md) architecture for the shared placement and input contracts.

The strength slider sets the next pulse. CHARGE prepares it on completed Host frames and displays its progress; changing the target invalidates the prepared charge. FIRE consumes the charge and scales the radar and projector pulse by that strength. Closing the titlebar returns to login, resets charge and fences unfinished work. The vertical RANGE slider rescales the radar and includes only contacts within the selected distance.

The physical pulse shield becomes visible while the workspace action is present. Its mesh and animation targets remain mounted through phase and display changes. Its front and four side walls cover FIRE PULSE, following the same Surface curvature and spacing as the control. The outside PULSE INTERLOCK checkbox arms or lifts it. While armed, its exact picking geometry is named as a GUI blocker; visual occlusion alone never blocks input. GUI ONLY hides the projector and shield while retaining the app.

The projector's authored beam follows the selected flat, cylindrical or spherical Surface and facing through matching Blender `MeshPose` endpoints. Its volumetric shader uses the same curved terminal boundary. The [authoring provenance](authoring/PROVENANCE.md) owns asset regeneration and correspondence checks. Floor, Base and Stage highlights multiply their baked textures through an explicit linear gain material, preserving colour and alpha within the existing material contracts.

The Surface opts into [texture caching](../../../../docs/architecture/rendering.md#optional-surface-texture-caching). DRAW chooses automatic, forced cached or direct presentation. A single flat image cannot represent separated layers, so a flat Surface with positive spacing draws directly; curved shells use their independent images. The render paths share retained control state and input geometry.

## Recovery and validation

A rejected scene declaration leaves the acknowledged scene and independent GUI World present. An observable warning links to Settings, where the operator can correct the authored value. Only a corrected primary acknowledgement clears the warning. Startup, attachment and session failures retain their separate lifecycle handling; recovery adds no rollback or automatic retry.

Maintained gallery scenarios exercise the actual generated client, worker/WASM/WebGL and native GLES runtime, real GUI input, completed frames and meaningful pixels. They cover sign-in and reveal, Host-clock loading and scrolling, scanner controls, settings, shield, layers, curved presentation, camera ownership and recoverable scene edits. Use the scoped [regression entry point](../../../../docs/development/building.md#regression-entry-point). The [gallery guide](../../README.md) explains building and running the world.
