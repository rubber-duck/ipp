# IPP demos

The demos are one engine-rendered application shared by the browser and persistent native GLES sessions. Its Canvas GUI selects the scene, appearance, text font and rendering resolution, with compact controls contributed by each scene. Display offers Shure Tech Mono and proportional Noto Sans independently of the skin. The font follows scene navigation and native session reload, including scene controls, cockpit panels and chart labels; icons and terminal cells retain their dedicated monospaced font. The application prepares a replacement scene before changing the visible Camera Surface and explicitly retires the previous scene and its owned Worlds.

The browser provides its canvas, physical input, connection and asset reads. Native development uses the same application declarations, assets and controllers through the [shared development Host](../../docs/development/shared-host.md). Hosts own clocks and animation; applications observe acknowledged state and completed output frames.

Display → FPS history opens a rolling chart of completed engine frames. Its ordinary Data Service stream exposes elapsed seconds, FPS and mean frame interval; sampling uses presentation metadata without pixel readback. The monitor includes its own small drawing cost and stops when hidden.

The cockpit keeps its screens on one fixed cylinder around the pilot’s seat. Drag the bridge background left or right to look around the vertical Y axis; the camera stays seated and level. Use Reset view in Controls to face forward again. Camera gestures leave zoom and pitch unchanged. Grabbing a header or unused outer chrome moves the carousel; scrolling over a header or unused panel space moves it too. Hovered sliders and dials adjust first, and scrollable content consumes movement before the carousel. Previous and Next remain available in scene controls. Screen distance is the radius from the camera in metres, shared by curved surfaces and flat tangent panels. GUI controls expose optional Surface texture caching, disabled by default. Auto caches side panels; Force requests caching throughout the carousel. Refresh rate and texture size controls support live performance comparisons. Active controls stay current. Curved geometry still uses rasterized presentation. The integrated dashboard remains dedicated to animated scanner and pulse displays.

Run the browser application from the repository root:

```sh
python tools/ipp.py dev gallery
```

Run a persistent native application after starting a compatible shared Host:

```sh
node tools/shared-host/shared-host.mjs gallery charts --name demos
```

The [development guide](../../tools/shared-host/README.md) describes actions, inspection, frame capture and source reload. Scene implementations stay independent of DOM and filesystem access through their asset adapter and target-matched Host contract. The [authoring architecture](../../docs/architecture/authoring.md#shared-scene-gallery) owns the application and scene lifecycle contract.
