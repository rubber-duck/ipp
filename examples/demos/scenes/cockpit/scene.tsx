import { startBrowserTerminal, type BrowserTerminal } from "@ipp/terminal";
import { GALLERY_RUNTIME } from "../../shared/runtime.js";
import type { GalleryDiagnosticsMonitor } from "../../frame-rate.js";
import type { FrameRateContract } from "../../frame-rate-panel.js";
import type {
  AnimationWorldClient,
  AnimationMotionControl,
  PickingWorldClient,
  CameraWorldClient,
  Client,
  Command,
  WorldReference,
  GuiTarget,
} from "@ipp/client";
import {
  CanvasWorld,
  Children,
  CylinderSurface,
  Entity,
  FlatSurface,
  Transform,
  type CanvasWorldHandle,
} from "@ipp/react";
import { mountGalleryScene } from "../../shared/scene-mount.js";
import type {
  GalleryOptions,
  GallerySceneDefinition,
  GallerySceneContext,
  GallerySceneMount,
} from "../../shared/scene.js";
import {
  COCKPIT_DEFAULTS,
  COCKPIT_PANE_COUNT,
  cockpitOptions,
  type CockpitOptions,
} from "./options.js";
import {
  CockpitControls,
  CockpitPane,
  DashboardInstrument,
  PANEL_DENSITY,
} from "./panels.js";
import { CockpitSkin, COCKPIT_FONT } from "./skin.js";
import {
  INSTRUMENT_RESOURCES,
  type InstrumentContract,
} from "./instruments.js";
import { CockpitExterior } from "./exterior.js";
import { CockpitLighting } from "./lighting.js";
import { validateRuntime, type CockpitRuntime } from "./runtime.js";
import {
  carouselScreenMounts,
  readPose,
  TRANSFORM_FIELDS,
  type Pose,
} from "./placement.js";

import {
  CarouselRotation,
  CarouselQuality,
  CAROUSEL_ASSET,
  CAROUSEL_SPRING_FREQUENCY,
  CAROUSEL_SPRING_DAMPING,
  CAROUSEL_ENTITY,
  carouselStep,
} from "./carousel.js";
import { CockpitCarouselInput } from "./carousel-input.js";
import type { CockpitNavigationSource } from "./control-screen.js";

export const COCKPIT_WORLD = "/target/demos-cockpit-assets/cockpit.ipp";
const PANEL_SYSTEMS = [
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.data-bindings",
  "ipp.plot",
] as const;
const SURFACE_NAMES = [
  "cockpit-screen-0",
  "cockpit-screen-1",
  "cockpit-screen-2",
  "cockpit-screen-3",
  "cockpit-screen-4",
  "cockpit-screen-5",
  "cockpit-pulse-surface",
  "cockpit-scanner-surface",
] as const;
interface Anchor {
  readonly id: bigint;
  readonly pose: Pose;
  readonly extent: readonly [number, number];
}

function CockpitWorld({
  context,
  anchors,
  material,
  state,
  update,
  report,
  ready,
  startTerminal,
  monitor,
  wheelTarget,
  runtime,
  navigation,
}: {
  navigation: CockpitNavigationSource;
  runtime: CockpitRuntime;
  monitor?: GalleryDiagnosticsMonitor | undefined;
  context: GallerySceneContext;
  anchors: readonly Anchor[];
  material: number;
  state: CockpitOptions;
  update(patch: GalleryOptions): void | Promise<void>;
  report(error: unknown): void;
  startTerminal?: (() => Promise<void>) | undefined;
  ready(index: number, handle: CanvasWorldHandle | null): void;
  wheelTarget(
    index: number,
    role: "grip" | "content",
    target: GuiTarget | null,
  ): void;
}) {
  const contract = context.contract as unknown as InstrumentContract;
  const placements = carouselScreenMounts(runtime.bridge, state.projection);
  const transform = context.canvas.client.components.Transform!;
  const pane = (index: number) => {
    const dashboard = index >= 6;
    const anchor = dashboard
      ? anchors[index - 3]!
      : anchors[Math.min(index, 2)]!;
    const placement = dashboard ? undefined : placements[index]!;
    const size = placement?.extent ?? anchor.extent;
    const density = dashboard ? 280 / size[0] : PANEL_DENSITY;
    const width = size[0] * density,
      height = size[1] * density;
    const name = SURFACE_NAMES[index]!;
    const identity = index < 3 || dashboard ? { bindTo: name } : { id: name };
    return (
      <Entity key={name} {...identity}>
        {!dashboard && <Transform {...placement!.pose} />}
        {!dashboard && (
          <CarouselQuality
            index={index}
            navigation={navigation}
            caching={state.surfaceCaching}
            refreshHz={state.surfaceRefreshHz}
            textureSize={state.surfaceTextureSize}
          />
        )}
        {!dashboard && state.surface === "curved" ? (
          <CylinderSurface
            width={size[0]}
            height={size[1]}
            curvature={placement!.curvature}
            layer_spacing={0}
          />
        ) : (
          <FlatSurface width={size[0]} height={size[1]} layer_spacing={0} />
        )}
        <CanvasWorld
          presentation={{ anchor: name }}
          create={{ selectedSystems: PANEL_SYSTEMS, temporary: true }}
          extent={[width, height]}
          unitsPerMetre={density}
          ref={(handle) => ready(index, handle)}
          onReady={(handle) => ready(index, handle)}
          onError={report}
        >
          <CockpitSkin
            contract={contract}
            assets={context.assets}
            state={state}
          >
            {dashboard ? (
              <DashboardInstrument
                width={width}
                height={height}
                radar={index === 7}
                contract={contract}
                assets={context.assets}
                state={state}
                report={report}
              />
            ) : (
              <CockpitPane
                index={index}
                startTerminal={startTerminal}
                browserHost={context.canvas.host}
                navigation={navigation}
                wheelTarget={(role, target) => wheelTarget(index, role, target)}
                width={width}
                height={height}
                state={state}
                update={update}
                monitor={monitor}
                application={context.application}
                contract={context.contract as unknown as FrameRateContract}
              />
            )}
          </CockpitSkin>
        </CanvasWorld>
      </Entity>
    );
  };
  return (
    <>
      <CockpitExterior runtime={runtime} component={material} state={state} />
      <CockpitLighting runtime={runtime} component={material} state={state} />
      <Entity bindTo={runtime.bridge.carouselMount}>
        <Children>
          <Entity id={CAROUSEL_ENTITY}>
            <Transform />
            <CarouselRotation
              component={transform.id}
              offsets={["qx", "qy", "qz", "qw"].map(
                (field) => transform.fields[field]!.offset,
              )}
              step={carouselStep(placements[0]!.extent[0], state.projection)}
            />
            <Children>
              {Array.from({ length: COCKPIT_PANE_COUNT }, (_, index) =>
                pane(index),
              )}
            </Children>
          </Entity>
        </Children>
      </Entity>
      {pane(6)}
      {pane(7)}
    </>
  );
}

function compact(inspection: Awaited<ReturnType<Client["inspect"]>>) {
  return {
    ...inspection,
    entities: inspection.entities.map((entity) => ({
      ...entity,
      components: entity.components.map((component) => ({
        ...component,
        fields: Object.fromEntries(
          Object.entries(component.fields).filter(
            ([, value]) => !(value instanceof Uint8Array),
          ),
        ),
        propertyDescriptors: undefined,
      })),
    })),
  };
}

async function waitResources(
  client: Client,
  signal: AbortSignal,
): Promise<void> {
  const deadline = performance.now() + 30_000;
  for (;;) {
    signal.throwIfAborted();
    const inspection = await client.inspect();
    const failed = inspection.resources.find(
      (resource) => resource.status === "failed",
    );
    if (failed)
      throw new Error(
        `Cockpit resource failed: ${failed.source}: ${failed.error}`,
      );
    if (inspection.resources.every((resource) => resource.status === "loaded"))
      return;
    if (performance.now() > deadline)
      throw new Error("Cockpit resource readiness timed out");
    await new Promise<void>((resolve) => setTimeout(resolve, 25));
  }
}

export const cockpitScene: GallerySceneDefinition = {
  id: "cockpit",
  label: "Starship bridge",
  shortLabel: "Cockpit",
  description:
    "Adjust the bridge displays, send a pulse and explore sample telemetry.",
  cameraNavigation: false,
  controls: CockpitControls,
  defaultOptions: { ...COCKPIT_DEFAULTS },
  actions: [
    "pulse",
    "scan",
    "carousel",
    "projection",
    "atmosphere",
    "atmospherePlayback",
    "illumination",
    "lightsPlayback",
    "cities",
    "stars",
    "resetCamera",
  ],
  resources: [{ kind: 17, path: COCKPIT_FONT }, ...INSTRUMENT_RESOURCES],
  world: (assets) => ({ load: { url: assets.url(COCKPIT_WORLD) } }),
  async initialize(client, signal, _host, assets) {
    const manifest = await assets.readJson<{
      ambientLight: [number, number, number];
    }>("/target/demos-cockpit-assets/manifest.json", signal);
    (client as CameraWorldClient).sendCommand({
      type: "RenderStateUpdateCommand",
      changes: { ambientLight: manifest.ambientLight },
    });
  },
  async mount(context, options) {
    const client = context.canvas.client;
    const inspection = await client.inspect();
    const runtime = validateRuntime(
      await context.assets.readJson<CockpitRuntime>(
        "/target/demos-cockpit-assets/manifest.json",
        context.signal,
      ),
    );
    const named = (name: string) => {
      const entity = inspection.entities.find(
        (entity) => entity.metadata.symbolicId === name,
      );
      if (!entity)
        throw new Error(`Exported cockpit entity is missing: ${name}`);
      return entity;
    };
    const transform = client.components.Transform!;
    const anchors = SURFACE_NAMES.filter(
      (_, index) => index < 3 || index >= 6,
    ).map((name) => {
      const entity = named(name);
      const fields = entity.components.find(
        (component) => component.component === transform.id,
      )?.fields;
      if (!fields)
        throw new Error(`Cockpit anchor has no exported transform: ${name}`);
      const size = runtime.surfaces[name];
      if (!size)
        throw new Error(`Cockpit export has no surface extent: ${name}`);
      return {
        id: entity.id,
        pose: readPose(fields),
        extent: [size.width, size.height] as const,
      };
    });
    for (const body of runtime.lighting.bodies) named(body.entity);
    for (const body of runtime.exterior.bodies) named(body.entity);
    const carriers = [
      ...runtime.lighting.carrierEntityNames,
      ...runtime.exterior.carrierEntityNames,
    ];
    if (carriers.length) {
      const removed = await client.batch(
        carriers.map((name) => ({
          kind: "delete",
          entity: { kind: "handle", id: named(name).id },
        })),
      );

      if (!removed.ok) throw new Error(removed.error.reason);
    }
    const camera = named("cockpit-camera");
    named(runtime.bridge.entity);
    named(runtime.bridge.carouselMount);
    const cameraProjection = camera.components.find(
      (component) => component.component === client.components.Camera!.id,
    )?.fields;
    if (!cameraProjection)
      throw new Error("Exported cockpit Camera projection is missing");
    const cameraPose = camera.components.find(
      (component) => component.component === transform.id,
    )?.fields;
    if (!cameraPose)
      throw new Error("Exported cockpit Camera transform is missing");
    if (!client.worldReference)
      throw new Error("Cockpit requires an explicit World");
    const output = await context.canvas.host.bindOutput(
      client.worldReference,
      camera.id,
      "camera",
    );
    const panels = new Map<number, WorldReference>();
    const observers: Client[] = [];
    let terminal: BrowserTerminal | undefined;
    let terminalStarting: Promise<void> | undefined;
    let terminalDisposed = false;
    const startTerminal =
      typeof Worker === "undefined"
        ? undefined
        : () => {
            if (terminal) return terminal.ready;
            return (terminalStarting ??= (async () => {
              if (terminalDisposed)
                throw new Error("Cockpit terminal is disposed");
              const world = panels.get(2);
              if (!world) throw new Error("Terminal panel is not ready");
              const observer = await context.canvas.host.openWorld(world);
              try {
                const target = (await observer.inspect()).entities.find(
                  (entity) => entity.metadata.symbolicId === "cockpit-terminal",
                );
                if (!target)
                  throw new Error("Terminal input target is missing");
                if (terminalDisposed)
                  throw new Error("Cockpit terminal is disposed");
                const asset = (name: string) =>
                  context.assets.url(`/target/terminal-build/${name}`);
                terminal = startBrowserTerminal(
                  context.canvas.host,
                  asset("terminal-worker.js"),
                  new URL(
                    `${GALLERY_RUNTIME}generated.js`,
                    window.location.href,
                  ).href,
                  {
                    world,
                    target: target.id,
                    fontSource: context.assets.url(COCKPIT_FONT),
                    fontMetricsUrl: asset("terminal-font.json"),
                    fontSize: 16,
                    lineHeight: 20,
                    transparentBackground: true,
                    wasm: asset("ghostty-vt.wasm"),
                    callbacksWasm: asset("ghostty-callbacks.wasm"),
                    bashkit: asset("bashkit.wasm"),
                    shell: {
                      cwd: "/home/pilot",
                      files: {
                        "/home/pilot/bridge.txt":
                          "Welcome aboard IPP Explorer.\nThis shell runs locally in your browser.\nTry pipelines, vi notes.txt, or ANSI colors with printf.\n",
                      },
                    },
                  },
                  (error) => context.onError?.(error),
                );
                await terminal.ready;
              } catch (error) {
                await terminal?.close().catch(() => {});
                terminal = undefined;
                throw error;
              } finally {
                await observer.close();
              }
            })().finally(() => {
              terminalStarting = undefined;
            }));
          };
    const gripIds: bigint[] = [];
    const wheelTargets = new Map<string, GuiTarget>();
    const panelHandles = new Map<number, CanvasWorldHandle>();
    const listeners = new Set<() => void>();
    const navigationListeners = new Set<() => void>();
    let resolvePanels!: () => void;
    let rejectPanels!: (error: unknown) => void;
    const panelReady = new Promise<void>((resolve, reject) => {
      resolvePanels = resolve;
      rejectPanels = reject;
    });
    void panelReady.catch(() => {});
    const report = (failure: unknown) => {
      rejectPanels(failure);
      if (context.signal.aborted) return;
      context.onError?.(
        failure instanceof Error ? failure : new Error(String(failure)),
      );
    };
    const onReady = (index: number, handle: CanvasWorldHandle | null) => {
      if (!handle) {
        panelHandles.delete(index);
        return;
      }
      panelHandles.set(index, handle);
      panels.set(index, handle.world);
      if (panels.size === COCKPIT_PANE_COUNT + 2) resolvePanels();
    };
    const notifyNavigation = () => {
      for (const listener of navigationListeners) listener();
    };
    const notify = () => {
      for (const listener of listeners) listener();
      notifyNavigation();
    };
    let mounted: GallerySceneMount | undefined;
    const navigation: CockpitNavigationSource = {
      focus: () =>
        Number(
          mounted?.options.carousel ??
            options.carousel ??
            COCKPIT_DEFAULTS.carousel,
        ),
      subscribe(listener) {
        navigationListeners.add(listener);
        return () => {
          navigationListeners.delete(listener);
        };
      },
    };
    const update = (patch: GalleryOptions) => mount.update(patch);
    const monitor = await context.diagnostics?.acquire();
    const base: GallerySceneMount = await (async () => {
      return mountGalleryScene(context, {
        output,
        // The Earth textures can take longer on a cold software renderer.
        // The application's scene-preparation deadline still bounds startup.
        resourceTimeoutMs: 60_000,
        options: { ...cockpitOptions({ ...COCKPIT_DEFAULTS, ...options }) },
        render: (value) => (
          <CockpitWorld
            runtime={runtime}
            context={context}
            anchors={anchors}
            material={client.components.CustomMaterial!.id}
            state={cockpitOptions(value)}
            update={update}
            report={report}
            ready={onReady}
            startTerminal={startTerminal}
            navigation={navigation}
            wheelTarget={(index, role, target) => {
              const key = `${index}/${role}`;
              if (target) wheelTargets.set(key, target);
              else wheelTargets.delete(key);
            }}
            monitor={monitor}
          />
        ),
        actions: {
          pulse: () => mount.update({ pulse: Number(mount.options.pulse) + 1 }),
          scan: (value) => mount.update({ scanning: Boolean(value) }),
          carousel: (direction) =>
            mount.update({
              carouselDragging: false,
              carousel: Math.max(
                0,
                Math.min(
                  COCKPIT_PANE_COUNT - 1,
                  Math.round(Number(mount.options.carousel)) +
                    (Number(direction) || 1),
                ),
              ),
            }),
          projection: (value) => mount.update({ projection: Number(value) }),
          atmosphere: (value) => mount.update({ atmosphere: Number(value) }),
          atmospherePlayback: (value) =>
            mount.update({ atmospherePlaying: Boolean(value) }),
          cities: (value) => mount.update({ cities: Number(value) }),
          stars: (value) => mount.update({ stars: Number(value) }),
          illumination: (value) =>
            mount.update({ illumination: Number(value) }),
          lightsPlayback: (value) =>
            mount.update({ lightsPlaying: Boolean(value) }),
          async resetCamera() {
            if (!cameraPose)
              throw new Error("Exported camera transform is missing");
            const commands: Command[] = TRANSFORM_FIELDS.map((name) => ({
              kind: "setField",
              entity: { kind: "handle", id: camera.id },
              component: transform.id,
              field: {
                offset: transform.fields[name]!.offset,
                value: { kind: "f32", value: Number(cameraPose[name]) },
              },
            }));
            const result = await client.batch(commands);
            if (!result.ok) throw new Error(result.error.reason);
          },
        },
      });
    })().catch(async (failure) => {
      try {
        await monitor?.close();
      } catch (cleanup) {
        throw new AggregateError(
          [failure, cleanup],
          "Cockpit diagnostics setup and release failed",
        );
      }
      throw failure;
    });
    let carouselController: bigint | undefined;
    let carouselReady = false;
    let trackingFocus: number | undefined;
    let trackingEpoch = 0;
    let trackingKind: "drag" | "wheel" | undefined;
    const pendingTracking = new Set<Promise<void>>();
    const animation = client as AnimationWorldClient;
    const boundFocus = (focus: number) =>
      Math.max(0, Math.min(COCKPIT_PANE_COUNT - 1, focus));
    const sendMotion = (
      motion: Omit<AnimationMotionControl, "min" | "max">,
    ) => {
      if (carouselController === undefined)
        throw new Error("Carousel controller is not ready");
      const pending = animation.controlAnimationController(carouselController, {
        action: "motion",
        motion: { ...motion, min: 0, max: COCKPIT_PANE_COUNT - 1 },
      });
      pendingTracking.add(pending);
      void pending.then(
        () => pendingTracking.delete(pending),
        (error) => {
          pendingTracking.delete(pending);
          if (!context.signal.aborted) report(error);
        },
      );
      return pending;
    };
    const spring = (target: number, velocity?: number) =>
      sendMotion({
        mode: {
          kind: "spring",
          target: boundFocus(target),
          frequency: CAROUSEL_SPRING_FREQUENCY,
          damping: CAROUSEL_SPRING_DAMPING,
        },
        ...(velocity === undefined ? {} : { velocity }),
      });
    const readMotion = async () => {
      const page = await client.inspectPage({ collection: "controllers" });
      const state = page.controllers?.find(
        (entry) => entry.id === carouselController,
      );
      if (!state) throw new Error("Carousel controller was retired");
      return state;
    };
    const track = async (
      focus: number,
      kind: "drag" | "wheel",
      current: () => boolean,
      velocity = 0,
    ) => {
      if (!current()) return;
      trackingFocus = boundFocus(focus);
      trackingKind = kind;
      notifyNavigation();
      // Gesture samples update one persistent Host controller. Pane trees never
      // rerender to advance motion, and retargeting preserves native velocity.
      if (kind === "drag")
        void sendMotion({
          mode: { kind: "hold" },
          position: trackingFocus,
          velocity,
        });
      else
        void sendMotion({
          mode: {
            kind: "spring",
            target: trackingFocus,
            frequency: 7,
            damping: 1,
          },
        });
    };
    const settle = async (
      focus: number,
      current: () => boolean,
      velocity?: number,
    ) => {
      const ending = ++trackingEpoch;
      if (pendingTracking.size) await Promise.allSettled([...pendingTracking]);
      if (!current() || ending !== trackingEpoch || context.signal.aborted)
        return;
      const selected = boundFocus(Math.round(focus));
      await spring(selected, velocity);
      if (!current() || ending !== trackingEpoch || context.signal.aborted)
        return;
      trackingKind = undefined;
      trackingFocus = undefined;
      await base.update({
        ...cockpitOptions({
          ...base.options,
          carousel: selected,
          carouselDragging: false,
        }),
      });
      notifyNavigation();
    };
    const ready = Promise.all([base.ready, panelReady]).then(async () => {
      for (const [, world] of [...panels].sort((a, b) => a[0] - b[0])) {
        context.signal.throwIfAborted();
        observers.push(await context.canvas.host.openWorld(world));
      }
      for (const [index, observer] of observers
        .slice(0, COCKPIT_PANE_COUNT)
        .entries()) {
        const { entities } = await observer.inspect();
        const grip = entities.find(
          (entity) => entity.metadata.symbolicId === `cockpit-grip-${index}`,
        );
        const content = entities.find(
          (entity) => entity.metadata.symbolicId === `cockpit-content-${index}`,
        );
        if (!grip || !content)
          throw new Error("Cockpit pane wheel targets are missing");
        gripIds.push(grip.id);
      }
      const rotation = (await client.inspect()).controllers?.find(
        (controller) =>
          controller.description.drivers.some((driver) =>
            driver.source.includes(CAROUSEL_ASSET),
          ),
      );
      if (!rotation) throw new Error("Cockpit carousel rotation is not ready");
      carouselController = rotation.id;
      const animation = client as AnimationWorldClient;
      await animation.controlAnimationController(rotation.id, {
        action: "seek",
        time: Number(base.options.carousel),
      });
      await animation.controlAnimationController(rotation.id, {
        action: "playAtSpeed",
        speed: 0,
      });
      await context.canvas.flush();
      await Promise.all(
        [client, ...observers].map((observer) =>
          waitResources(observer, context.signal),
        ),
      );
      carouselReady = true;
    });
    void ready.catch(report);
    const unsubscribeBase = base.subscribe?.(notify);
    const mount: GallerySceneMount = {
      ...base,
      ready,
      get options() {
        return trackingFocus === undefined
          ? base.options
          : {
              ...base.options,
              carousel: trackingFocus,
              carouselDragging: trackingKind === "drag",
            };
      },
      sceneInput: new CockpitCarouselInput({
        signal: context.signal,
        ready: () => carouselReady,
        camera: readPose(cameraPose),
        client: context.presentationClient ?? (client as PickingWorldClient),
        view: () => {
          if (!context.cameraView)
            throw new Error("Cockpit drag requires a qualified Camera view");
          return context.cameraView();
        },
        panes: () =>
          observers.slice(0, COCKPIT_PANE_COUNT).map((observer, index) => ({
            client: observer,
            grip: gripIds[index]!,
          })),
        ownsWheelHit: ({ hit }) => {
          if (!hit) return false;
          const prefix = context.cameraPath?.() ?? [];
          if (hit.path.length !== prefix.length + 1) return false;
          const sameToken = (
            actual: (typeof hit.path)[number],
            expected: (typeof hit.path)[number],
          ) =>
            actual.parent.id === expected.parent.id &&
            actual.parent.incarnation === expected.parent.incarnation &&
            actual.anchor === expected.anchor &&
            actual.incarnation === expected.incarnation &&
            actual.revision === expected.revision;
          if (
            !prefix.every((token, index) => sameToken(hit.path[index]!, token))
          )
            return false;
          return observers.slice(0, COCKPIT_PANE_COUNT).some((_, index) => {
            const handle = panelHandles.get(index);
            const attachment = handle?.attachment;
            const target = hit.target;
            if (
              !handle ||
              !attachment ||
              target.world.id !== handle.world.id ||
              target.world.incarnation !== handle.world.incarnation ||
              !sameToken(hit.path.at(-1)!, attachment)
            )
              return false;
            return ["grip", "content"].some((role) => {
              const live = wheelTargets.get(`${index}/${role}`);
              return (
                live &&
                target.entity === live.entity &&
                target.component === live.component &&
                target.incarnation === live.incarnation &&
                target.world.id === live.world.id &&
                target.world.incarnation === live.world.incarnation
              );
            });
          });
        },
        focus: () => Number(mount.options.carousel),
        step: () =>
          carouselStep(
            anchors
              .slice(0, 3)
              .reduce((sum, anchor) => sum + anchor.extent[0], 0) / 3,
            Number(mount.options.projection),
          ),
        wheel: (focus, current) => track(focus, "wheel", current),
        releaseWheel: async (focus, current) => {
          if (pendingTracking.size)
            await Promise.allSettled([...pendingTracking]);
          if (!current()) return;
          const state = await readMotion();
          if (!current()) return;
          // The spring's current velocity includes the arriving wheel stream;
          // preserve it once, including a trackpad's own decaying tail.
          await settle(focus + (state.motion?.velocity ?? 0) * 0.12, current);
        },
        begin: async (current) => {
          ++trackingEpoch;
          if (pendingTracking.size)
            await Promise.allSettled([...pendingTracking]);
          if (!current()) return Number(mount.options.carousel);
          await sendMotion({ mode: { kind: "hold" }, velocity: 0 });
          const state = await readMotion();
          if (!current()) return state.time;
          trackingFocus = state.time;
          trackingKind = "drag";
          notifyNavigation();
          return state.time;
        },
        onError: report,
        update: async (
          focus,
          dragging,
          current = () => !context.signal.aborted,
          velocity = 0,
        ) => {
          if (!current()) return;
          if (dragging) await track(focus, "drag", current, velocity);
          else await settle(focus, current, velocity);
        },
      }),
      controller: {
        get panels() {
          return [...panels].map(([index, world]) => ({ index, world }));
        },
      },
      subscribe(listener: () => void) {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
      async resize(viewport) {
        const aspect = viewport.width / viewport.height;
        // Fit a central panel on narrow displays; surrounding scenery and other
        // carousel segments naturally leave the view. World geometry stays fixed.
        const fov =
          2 *
          Math.atan(
            Math.tan(Number(cameraProjection.fov_y) / 2) *
              Math.max(1, 1 / aspect),
          );
        const result = await client.batch([
          {
            kind: "setField",
            entity: { kind: "handle", id: camera.id },
            component: client.components.Camera!.id,
            field: {
              offset: client.components.Camera!.fields.fov_y!.offset,
              value: { kind: "f32", value: fov },
            },
          },
        ]);
        if (!result.ok) throw new Error(result.error.reason);
      },
      async update(patch: GalleryOptions) {
        const value = cockpitOptions({ ...mount.options, ...patch });
        // External navigation retargets the same Host-owned motion. Existing
        // samples precede it in the ordered transport; no component pose is reset.
        ++trackingEpoch;
        trackingFocus = undefined;
        trackingKind = undefined;
        mount.sceneInput?.cancel();
        await base.update({ ...value, carouselDragging: false });
        await ready;
        await spring(value.carousel);
      },
      async inspect() {
        return {
          options: mount.options,
          scene: compact(await client.inspect()),
          panels: await Promise.all(
            observers.map(async (observer) => ({
              world: observer.worldReference,
              ...compact(await observer.inspect()),
            })),
          ),
        };
      },
      async dispose() {
        mount.sceneInput?.cancel();
        panelHandles.clear();
        wheelTargets.clear();
        ++trackingEpoch;
        trackingFocus = undefined;
        trackingKind = undefined;
        await Promise.allSettled([...pendingTracking]);
        rejectPanels(new Error("Cockpit disposed"));
        const failures: unknown[] = [];
        await ready.catch(() => {});
        terminalDisposed = true;
        await terminalStarting?.catch(() => {});
        try {
          await terminal?.close();
        } catch (failure) {
          failures.push(failure);
        }
        for (const observer of observers) {
          try {
            await observer.close();
          } catch (failure) {
            failures.push(failure);
          }
        }
        try {
          await base.dispose();
        } catch (failure) {
          failures.push(failure);
        }
        try {
          await monitor?.close();
        } catch (failure) {
          failures.push(failure);
        }
        unsubscribeBase?.();
        listeners.clear();
        navigationListeners.clear();
        if (failures.length)
          throw new AggregateError(failures, "Cockpit cleanup failed");
      },
    };
    mounted = mount;
    return mount;
  },
};

export default cockpitScene;
